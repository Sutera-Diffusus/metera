import { Activity, ChevronDown, Coins, DollarSign, JapaneseYen, RefreshCw, Sparkles } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../../lib/api";
import { cacheHitRate, displaySource, formatDuration, formatTokens, totalsOf } from "../../lib/analytics";
import { CNY_PER_USD, estimateCost } from "../../lib/pricing";
import type { RangeKey, UsageBucket } from "../../lib/types";
import { useMetera } from "../../state/MeteraContext";
import { modelBrand, sourceFill } from "../brands";
import { OverviewActivityChart, type OverviewMetric } from "../charts/OverviewActivityChart";
import { OverviewUsageRing, type OverviewDistributionRow } from "../charts/OverviewUsageRing";
import { OverviewQuotaBalance, OverviewQuotaRisk } from "./OverviewQuotaPanels";
import type { DashboardView } from "../DashboardSidebar";
import { activityPoints } from "./overviewActivity";

const RANGE_OPTIONS: Array<[RangeKey, string]> = [["today", "今天"], ["24h", "一天"], ["7d", "一周"], ["30d", "一月"], ["90d", "三个月"]];
const rangeLabel = (range: RangeKey) => RANGE_OPTIONS.find(([key]) => key === range)?.[1] ?? "自定义";
const bucketTokens = (bucket: UsageBucket) => bucket.inputTokens + bucket.cachedInputTokens + bucket.outputTokens + bucket.reasoningOutputTokens;
const valueOf = (bucket: UsageBucket, metric: OverviewMetric, rate: number) => {
  if (metric === "tokens") return bucketTokens(bucket);
  const cost = estimateCost(bucket);
  return cost == null ? null : cost * rate;
};
const compactTokens = (value: number) => value >= 1e9 ? `${(value / 1e9).toFixed(2)}B` : value >= 1e6 ? `${(value / 1e6).toFixed(2)}M` : value >= 1e3 ? `${(value / 1e3).toFixed(2)}K` : Math.round(value).toLocaleString();
const money = (value: number | null, currency: "USD" | "CNY") => value == null || !Number.isFinite(value) ? "--" : `${currency === "CNY" ? "¥" : "$"}${value.toFixed(2)}`;
const MODEL_RING_COLORS = ["#8f7cff", "#46d7c0", "#5f91ff", "#ffb45f"] as const;

function useDailyUsdCnyRate() {
  const [rate, setRate] = useState(CNY_PER_USD);
  useEffect(() => {
    const dateKey = new Date().toISOString().slice(0, 10);
    const cacheKey = "metera-usd-cny-daily";
    try {
      const cached = JSON.parse(localStorage.getItem(cacheKey) ?? "null") as { date?: string; rate?: number } | null;
      if (cached?.date === dateKey && typeof cached.rate === "number" && cached.rate > 0) setRate(cached.rate);
    } catch { /* local storage is optional */ }
    let disposed = false;
    void api.exchangeRate().then(next => {
      if (disposed || !next || !Number.isFinite(next)) return;
      setRate(next);
      try { localStorage.setItem(cacheKey, JSON.stringify({ date: dateKey, rate: next })); } catch { /* optional cache */ }
    }).catch(() => undefined);
    return () => { disposed = true; };
  }, []);
  return rate;
}

function RangeFilter({ range, onChange, pending }: { range: RangeKey; onChange(value: RangeKey): void; pending: boolean }) {
  const [expanded, setExpanded] = useState(false);
  return <div className={`overview-v4-range-filter${expanded ? " expanded" : ""}${pending ? " pending" : ""}`} aria-busy={pending} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setExpanded(false); }}>
    <button type="button" className="overview-v5-range-trigger" aria-label={`时间范围：${rangeLabel(range)}${pending ? "，正在更新" : ""}`} aria-expanded={expanded} aria-haspopup="listbox" onClick={() => setExpanded(value => !value)}>
      <span className="overview-v4-range-current">{rangeLabel(range)}</span><ChevronDown aria-hidden="true"/>
    </button>
    <AnimatePresence initial={false}>
      {expanded && <motion.div className="overview-v4-range-options motion" role="tablist" aria-label="时间范围" initial={{ opacity: 0, y: -5, scale: .985 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: -4, scale: .985 }} transition={{ duration: .17, ease: [0.16, 1, 0.3, 1] }}>
        {RANGE_OPTIONS.map(([key, label]) => <button key={key} type="button" role="tab" aria-selected={range === key} className={range === key ? "active" : ""} onClick={() => { onChange(key); setExpanded(false); }}>{label}</button>)}
      </motion.div>}
    </AnimatePresence>
  </div>;
}

function distributionRows(buckets: UsageBucket[], metric: OverviewMetric, rate: number): OverviewDistributionRow[] {
  const grouped = new Map<string, { tokens: number; cost: number; value: number }>();
  for (const bucket of buckets) {
    const value = valueOf(bucket, metric, rate);
    if (value == null) continue;
    const source = bucket.source || "unknown";
    const row = grouped.get(source) ?? { tokens: 0, cost: 0, value: 0 };
    row.tokens += bucketTokens(bucket);
    row.cost += estimateCost(bucket) ?? 0;
    row.value += value;
    grouped.set(source, row);
  }
  const sorted = [...grouped.entries()].sort((left, right) => right[1].value - left[1].value);
  const top = sorted.slice(0, 4);
  const rest = sorted.slice(4).reduce((row, [, value]) => ({ tokens: row.tokens + value.tokens, cost: row.cost + value.cost, value: row.value + value.value }), { tokens: 0, cost: 0, value: 0 });
  if (rest.value > 0) top.push(["__other", rest]);
  const total = top.reduce((sum, [, row]) => sum + row.value, 0);
  return top.map(([source, row]) => ({ key: source, label: source === "__other" ? "其他" : displaySource(source), value: row.value, share: total > 0 ? row.value / total : 0, tokens: row.tokens, cost: row.cost * rate, iconBrand: source === "__other" ? null : source === "zcode" ? "zhipu" : source, color: source === "__other" ? "var(--brand-unknown)" : sourceFill(source) }));
}

function modelDistributionRows(buckets: UsageBucket[], metric: OverviewMetric, rate: number): OverviewDistributionRow[] {
  const grouped = new Map<string, { source: string; tokens: number; cost: number; value: number }>();
  for (const bucket of buckets) {
    const value = valueOf(bucket, metric, rate);
    if (value == null) continue;
    const name = bucket.model || "unknown";
    const row = grouped.get(name) ?? { source: bucket.source || "unknown", tokens: 0, cost: 0, value: 0 };
    row.tokens += bucketTokens(bucket);
    row.cost += estimateCost(bucket) ?? 0;
    row.value += value;
    grouped.set(name, row);
  }
  const total = [...grouped.values()].reduce((sum, row) => sum + row.value, 0);
  const sorted = [...grouped.entries()].sort((left, right) => right[1].value - left[1].value);
  const top = sorted.slice(0, 4);
  const rest = sorted.slice(4).reduce((row, [, value]) => ({ source: row.source, tokens: row.tokens + value.tokens, cost: row.cost + value.cost, value: row.value + value.value }), { source: "unknown", tokens: 0, cost: 0, value: 0 });
  if (rest.value > 0) top.push(["__other", rest]);
  return top.map(([name, row], index) => {
    if (name === "__other") return { key: name, label: "其他", value: row.value, share: total > 0 ? row.value / total : 0, tokens: row.tokens, cost: row.cost * rate, iconBrand: null, color: "#9099ad" };
    const brand = modelBrand(name);
    return { key: name, label: name, value: row.value, share: total > 0 ? row.value / total : 0, tokens: row.tokens, cost: row.cost * rate, iconBrand: brand ?? (row.source === "zcode" ? "zhipu" : row.source), color: MODEL_RING_COLORS[index % MODEL_RING_COLORS.length] };
  });
}

export function OverviewView({ onNavigate }: { onNavigate?(view: DashboardView): void }) {
  const state = useMetera();
  const [metric, setMetric] = useState<OverviewMetric>(state.settings.widgetMetric === "cost" ? "cost" : "tokens");
  const [currency, setCurrency] = useState<"USD" | "CNY">("USD");
  const rate = useDailyUsdCnyRate();
  const now = new Date();
  const buckets = state.buckets;
  const sessions = state.sessions;
  const totals = useMemo(() => totalsOf(buckets), [buckets]);
  const cacheRate = cacheHitRate(totals.input, totals.cached) * 100;
  const costUsd = totals.priced > 0 ? totals.cost : null;
  const costShown = costUsd == null ? null : currency === "CNY" ? costUsd * rate : costUsd;
  const selectedTotal = metric === "tokens" ? totals.tokens : costShown ?? 0;
  const points = useMemo(() => activityPoints(buckets, sessions, metric, currency === "CNY" ? rate : 1, valueOf), [buckets, sessions, metric, currency, rate]);
  const sessionTotals = useMemo(() => points.reduce((sum, point) => sum + point.activeMinutes * 60, 0), [points]);
  const distribution = useMemo(() => distributionRows(buckets, metric, currency === "CNY" ? rate : 1), [buckets, metric, currency, rate]);
  const models = useMemo(() => modelDistributionRows(buckets, metric, currency === "CNY" ? rate : 1), [buckets, metric, currency, rate]);
  const connectedSources = useMemo(() => state.status?.sources ?? state.activity.sources, [state.status?.sources, state.activity.sources]);
  const dateText = now.toLocaleDateString("zh-CN", { month: "long", day: "numeric", weekday: "long" });
  const updatedAt = state.scan.lastScanAt ? new Date(state.scan.lastScanAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false }) : "--:--";
  const coverage = buckets.length ? Math.round((totals.priced / buckets.length) * 100) : 0;
  const heroValue = metric === "tokens" ? compactTokens(totals.tokens) : money(costShown, currency);
  const heroDetail = metric === "tokens"
    ? `${formatTokens(totals.input + totals.cached)} 输入 · ${formatTokens(totals.output + totals.reasoning)} 输出`
    : `${coverage}% 数据已计价 · ${currency === "USD" ? `1 USD ≈ ¥${rate.toFixed(2)}` : "人民币按今日汇率换算"}`;

  return <div className={`v2-view apple-overview overview-v4 overview-v5${state.chartReplayActive ? " refreshing" : ""}${state.rangeLoading ? " range-loading" : ""}${state.scan.status === "scanning" ? " scanning" : ""}`}>
    <header className="overview-v4-header overview-v5-header">
      <div><p>USAGE INTELLIGENCE</p><h1>总览</h1><span>{dateText} · 今天的 AI 使用节奏</span></div>
      <div className="overview-v4-header-actions"><span className="overview-v4-sync"><i/>上次同步 {updatedAt}</span><button className="overview-v4-refresh" type="button" onClick={() => void api.triggerScan()} disabled={state.scan.status === "scanning"}><RefreshCw className={state.scan.status === "scanning" ? "spin" : ""}/><AnimatePresence mode="wait" initial={false}><motion.span key={state.scan.status === "scanning" ? "scanning" : "ready"} initial={{ opacity: 0, y: 4, filter: "blur(3px)" }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }} exit={{ opacity: 0, y: -3, filter: "blur(3px)" }} transition={{ duration: .16, ease: [0.16, 1, 0.3, 1] }}>{state.scan.status === "scanning" ? "同步中" : "更新数据"}</motion.span></AnimatePresence></button></div>
    </header>

    <section className="overview-v4-toolbar overview-v5-toolbar" aria-label="总览时间范围">
      <RangeFilter range={state.range} onChange={state.setRange} pending={state.rangeLoading}/>
      <AnimatePresence initial={false} mode="wait">
        {state.rangeLoading && <motion.span className="overview-v5-range-status" role="status" aria-live="polite" initial={{ opacity: 0, x: -5, filter: "blur(4px)" }} animate={{ opacity: 1, x: 0, filter: "blur(0px)" }} exit={{ opacity: 0, x: -3, filter: "blur(3px)" }} transition={{ duration: .16, ease: [0.16, 1, 0.3, 1] }}>正在切换范围</motion.span>}
      </AnimatePresence>
    </section>

    <div className="overview-v5-main-grid">
      <section className="overview-v4-panel overview-v5-pulse-panel">
        <header className="overview-v5-pulse-head">
          <div className="overview-v5-heading-with-icon"><span className="overview-v5-panel-symbol pulse"><Activity/></span><div><p>TODAY'S PULSE</p><h2>今日使用脉搏</h2></div></div>
          <div className="overview-v5-pulse-actions">
            <div className="overview-v5-metric-switch" role="tablist" aria-label="主图指标">
              <button type="button" role="tab" aria-selected={metric === "tokens"} className={metric === "tokens" ? "active tokens" : "tokens"} onClick={() => setMetric("tokens")}>{metric === "tokens" && <motion.i className="overview-v5-tab-indicator" layoutId="overview-metric-indicator" transition={{ type: "spring", stiffness: 480, damping: 34, mass: .6 }}/>}<Coins/><span>用量</span></button>
              <button type="button" role="tab" aria-selected={metric === "cost"} className={metric === "cost" ? "active cost" : "cost"} onClick={() => setMetric("cost")}>{metric === "cost" && <motion.i className="overview-v5-tab-indicator" layoutId="overview-metric-indicator" transition={{ type: "spring", stiffness: 480, damping: 34, mass: .6 }}/>}<DollarSign/><span>花销</span></button>
            </div>
            {metric === "cost" && <div className="overview-v5-currency-switch" role="tablist" aria-label="花销币种">
               <button type="button" role="tab" aria-selected={currency === "USD"} className={currency === "USD" ? "active" : ""} onClick={() => setCurrency("USD")}>{currency === "USD" && <motion.i className="overview-v5-tab-indicator" layoutId="overview-currency-indicator" transition={{ type: "spring", stiffness: 480, damping: 34, mass: .6 }}/>}<DollarSign/><span>美元</span></button>
               <button type="button" role="tab" aria-selected={currency === "CNY"} className={currency === "CNY" ? "active" : ""} onClick={() => setCurrency("CNY")}>{currency === "CNY" && <motion.i className="overview-v5-tab-indicator" layoutId="overview-currency-indicator" transition={{ type: "spring", stiffness: 480, damping: 34, mass: .6 }}/>}<JapaneseYen/><span>人民币</span></button>
            </div>}
          </div>
        </header>
        <div className="overview-v5-hero-value" aria-live="polite">
          <AnimatePresence mode="popLayout" initial={false}>
            <motion.strong className={`num ${metric} ${currency.toLowerCase()}`} key={`${metric}-${currency}-${heroValue}`} initial={{ opacity: 0, y: 8, filter: "blur(7px)" }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }} exit={{ opacity: 0, y: -7, filter: "blur(7px)" }} transition={{ duration: .24, ease: [0.16, 1, 0.3, 1] }}>{heroValue}</motion.strong>
          </AnimatePresence>
          <AnimatePresence mode="wait" initial={false}>
            <motion.span key={`${metric}-${currency}`} initial={{ opacity: 0, x: 5, filter: "blur(4px)" }} animate={{ opacity: 1, x: 0, filter: "blur(0px)" }} exit={{ opacity: 0, x: -4, filter: "blur(4px)" }} transition={{ duration: .18, ease: [0.16, 1, 0.3, 1] }}>{metric === "tokens" ? "Token" : currency === "USD" ? "美元" : "人民币"}</motion.span>
          </AnimatePresence>
        </div>
        <AnimatePresence mode="wait" initial={false}>
          <motion.div className="overview-v5-hero-detail" key={`${metric}-${currency}-${heroDetail}`} initial={{ opacity: 0, y: 4, filter: "blur(4px)" }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }} exit={{ opacity: 0, y: -3, filter: "blur(4px)" }} transition={{ duration: .2, ease: [0.16, 1, 0.3, 1] }}>{heroDetail}</motion.div>
        </AnimatePresence>
        <OverviewActivityChart points={points} metric={metric} now={now} range={state.range} rangeLabel={rangeLabel(state.range)} currency={currency}/>
        <div className="overview-v5-micro-stats">
          <div><span>活跃时间</span><strong className="num active">{formatDuration(sessionTotals)}</strong><em>{rangeLabel(state.range)}</em></div>
          <div><span>缓存命中率</span><strong className="num cache">{cacheRate.toFixed(1)}%</strong><em>{formatTokens(totals.cached)} cached</em></div>
          <div><span>会话</span><strong className="num sessions">{sessions.length}</strong><em>{connectedSources.length} 个来源</em></div>
        </div>
      </section>

      <OverviewQuotaRisk quotas={state.quotas} onInspect={() => onNavigate?.("accounts")}/>
    </div>

    <div className="overview-v5-lower-grid">
      <section className="overview-v4-panel overview-v5-composition-panel">
        <header className="overview-v4-panel-header"><div className="overview-v5-heading-with-icon"><span className="overview-v5-panel-symbol composition"><Sparkles/></span><div><p>COMPOSITION</p><h2>来源构成</h2></div></div></header>
        <OverviewUsageRing sourceRows={distribution} modelRows={models} metric={metric} total={selectedTotal} currency={currency}/>
      </section>
      <OverviewQuotaBalance quotas={state.quotas} pinned={state.settings.pinnedQuotaProviders} onInspect={() => onNavigate?.("accounts")}/>
    </div>

    <footer className="overview-v4-footline"><span><i className={state.scan.status === "scanning" ? "busy" : ""}/>{state.scan.status === "scanning" ? "正在同步本地来源" : `已连接 ${connectedSources.length} 个来源`}</span><span>数据保留在本机 · 数值会随时间范围切换平滑刷新</span></footer>
  </div>;
}
