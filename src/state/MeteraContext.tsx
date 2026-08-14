import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { defaultSettings, type AgentActivity, type AppSettings, type AppStatus, type Filters, type QuotaAccount, type RangeKey, type ScanState, type UsageBucket, type UsageSession } from "../lib/types";

interface MeteraState {
  buckets: UsageBucket[]; sessions: UsageSession[]; rawBuckets: UsageBucket[]; range: RangeKey; filters: Filters;
  customStart: string; customEnd: string; settings: AppSettings; status: AppStatus | null; scan: ScanState; activity: AgentActivity; quotas: QuotaAccount[];
  chartReplay: number; chartReplayActive: boolean; shortageAlerts: string[];
  loading: boolean; rangeLoading: boolean; error: string | null; setRange(v:RangeKey):void; setFilters(v:Filters):void; setCustomStart(v:string):void; setCustomEnd(v:string):void;
  updateSettings(patch:Partial<AppSettings>):Promise<void>; refresh():Promise<void>;
}
const Context = createContext<MeteraState | null>(null);
const sameActivity = (left: AgentActivity, right: AgentActivity) => left.active === right.active
  && left.state === right.state
  && left.source === right.source
  && left.detail === right.detail
  && left.sources.length === right.sources.length
  && left.sources.every((source, index) => source === right.sources[index]);
const localDate = (date: Date) => `${date.getFullYear()}-${String(date.getMonth()+1).padStart(2,"0")}-${String(date.getDate()).padStart(2,"0")}`;
function rangeBounds(range: RangeKey, customStart: string, customEnd: string) {
  const end = new Date(); end.setMilliseconds(end.getMilliseconds()+1000); let start = new Date(end);
  if (range === "today") start.setHours(0,0,0,0);
  else if (range === "24h") start = new Date(end.getTime()-24*3600_000);
  else if (range === "custom") { start = new Date(`${customStart}T00:00:00`); const customEndDate = new Date(`${customEnd}T00:00:00`); customEndDate.setDate(customEndDate.getDate()+1); return { start:start.toISOString(), end:customEndDate.toISOString() }; }
  else { const days = Number.parseInt(range); start.setHours(0,0,0,0); start.setDate(start.getDate()-days+1); }
  return { start:start.toISOString(), end:end.toISOString() };
}

export function MeteraProvider({ children, initialRange = "today" }: { children: React.ReactNode; initialRange?: RangeKey }) {
  const today = localDate(new Date()); const [range,setRange] = useState<RangeKey>(initialRange); const [customStart,setCustomStart]=useState(today); const [customEnd,setCustomEnd]=useState(today);
  const [rawBuckets,setRawBuckets]=useState<UsageBucket[]>([]); const [rawSessions,setRawSessions]=useState<UsageSession[]>([]);
  const [filters,setFilters]=useState<Filters>({hostname:"all",source:"all",provider:"all",model:"all",project:"all"});
  const [settings,setSettings]=useState(defaultSettings); const [status,setStatus]=useState<AppStatus|null>(null); const [scan,setScan]=useState<ScanState>({status:"idle"});
  const [activity,setActivity]=useState<AgentActivity>({active:false,state:"idle",source:null,sources:[]}); const [quotas,setQuotas]=useState<QuotaAccount[]>([]); const [loading,setLoading]=useState(true); const [rangeLoading,setRangeLoading]=useState(false); const [error,setError]=useState<string|null>(null);
  const [chartReplay,setChartReplay]=useState(0); const [chartReplayActive,setChartReplayActive]=useState(false); const replayTimer=useRef<ReturnType<typeof setTimeout>|null>(null);
  const loadSequence=useRef(0);
  const triggerChartReplay=useCallback(()=>{setChartReplay(value=>value+1);setChartReplayActive(true);if(replayTimer.current)clearTimeout(replayTimer.current);replayTimer.current=setTimeout(()=>setChartReplayActive(false),1800);},[]);
  const setSelectedRange=useCallback((next:RangeKey)=>{ if(next===range)return; setRangeLoading(true); setRange(next); triggerChartReplay(); },[range,triggerChartReplay]);
  const load = useCallback(async()=>{ const sequence=++loadSequence.current; try { setError(null); const bounds=rangeBounds(range,customStart,customEnd); const [usage,appStatus,scanState,saved,accountQuotas]=await Promise.all([api.usage(bounds.start,bounds.end),api.status(),api.scanState(),api.settings().catch(()=>defaultSettings),api.quotas().catch(()=>[])]); if(sequence!==loadSequence.current)return; setRawBuckets(usage.buckets); setRawSessions(usage.sessions); setStatus(appStatus); setScan(scanState); setSettings(saved); setQuotas(accountQuotas); triggerChartReplay(); } catch(reason){ if(sequence===loadSequence.current)setError(String(reason)); } finally{ if(sequence===loadSequence.current){setLoading(false);setRangeLoading(false);} } },[range,customStart,customEnd,triggerChartReplay]);
  useEffect(()=>{void load();},[load]);
  useEffect(()=>{let cleanScan:(()=>void)|undefined,cleanSettings:(()=>void)|undefined,disposed=false; api.onScan(s=>{if(disposed)return;setScan(s);if(s.status==="ready"){void load();}}).then(v=>{if(!disposed)cleanScan=v;else v();}); api.onSettings(setSettings).then(v=>{if(!disposed)cleanSettings=v;else v();}); return()=>{disposed=true;cleanScan?.();cleanSettings?.();};},[load]);
  useEffect(()=>()=>{if(replayTimer.current)clearTimeout(replayTimer.current);},[]);
  useEffect(()=>{let mounted=true; const poll=()=>api.activity().then(v=>{if(mounted)setActivity(current=>sameActivity(current,v)?current:v);}).catch(()=>undefined); void poll(); const timer=setInterval(poll,3000); return()=>{mounted=false;clearInterval(timer);};},[]);
  useEffect(()=>{let mounted=true;const refreshQuota=()=>api.quotas().then(value=>{if(mounted)setQuotas(value);}).catch(()=>undefined);void refreshQuota();const timer=setInterval(refreshQuota,60_000);return()=>{mounted=false;clearInterval(timer);};},[]);
  // §16 断粮提醒：预测耗尽 ≤ 3 天或剩余 <20% 时在额度视图提示（每日一次,纯本地）。
  const [shortageAlerts,setShortageAlerts]=useState<string[]>([]);
  useEffect(()=>{let lastShown="";const check=()=>{const alerts=quotas.flatMap(a=>{const i=a.insight;if(!i)return[];const warnings:string[]=[];if(i.projectedExhaustionAt){const days=(i.projectedExhaustionAt-Date.now())/86400000;if(days<=3)warnings.push(`${a.name} 预计 ${Math.max(1,Math.round(days))} 天后额度耗尽`);}const low=a.windows.filter(w=>w.remainingPercent!=null&&w.remainingPercent<20);if(low.length)warnings.push(`${a.name} ${low.map(w=>w.label).join("/")} 剩余不足 20%`);return warnings;});const key=alerts.join("|");if(alerts.length&&key!==lastShown){lastShown=key;setShortageAlerts(alerts);}};check();const timer=setInterval(check,60_000);return()=>{clearInterval(timer);};},[quotas]);
  const buckets=useMemo(()=>rawBuckets.filter(b=>(filters.hostname==="all"||(b.hostname||"unknown")===filters.hostname)&&(filters.source==="all"||(b.source||"unknown")===filters.source)&&(filters.provider==="all"||(b.provider||"unknown")===filters.provider)&&(filters.model==="all"||(b.model||"unknown")===filters.model)&&(filters.project==="all"||(b.project||"unknown")===filters.project)),[rawBuckets,filters]);
  const sessions=useMemo(()=>rawSessions.filter(s=>(filters.hostname==="all"||(s.hostname||"unknown")===filters.hostname)&&(filters.source==="all"||(s.source||"unknown")===filters.source)&&(filters.project==="all"||(s.project||"unknown")===filters.project)),[rawSessions,filters]);
  const updateSettings=useCallback(async(patch:Partial<AppSettings>)=>{setSettings(current=>{const next={...current,...patch};void api.saveSettings(next).catch(err=>{console.error("保存设置失败",err);setError("保存设置失败,请重试");});return next;});},[]);
  // 重新检测：先触发后端全量扫描（trigger_scan），再拉取最新数据。
  // 旧实现 refresh 仅重拉已有数据,扫描周期 5 分钟,点击"重新检测"看似无反应。
  const rescan = useCallback(async()=>{
    try { setError(null); setLoading(true); await api.triggerScan(); await load(); }
    catch(reason){ setError(String(reason)); setLoading(false); }
  },[load]);
  const value=useMemo(()=>({buckets,sessions,rawBuckets,range,filters,customStart,customEnd,settings,status,scan,activity,quotas,chartReplay,chartReplayActive,shortageAlerts,loading,rangeLoading,error,setRange:setSelectedRange,setFilters,setCustomStart,setCustomEnd,updateSettings,refresh:rescan}),[buckets,sessions,rawBuckets,range,filters,customStart,customEnd,settings,status,scan,activity,quotas,chartReplay,chartReplayActive,shortageAlerts,loading,rangeLoading,error,rescan,updateSettings,setSelectedRange]);
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
export function useMetera(){const value=useContext(Context);if(!value)throw new Error("useMetera must be used within MeteraProvider");return value;}
