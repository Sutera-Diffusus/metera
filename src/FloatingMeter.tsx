import { useEffect, useMemo, useRef, useState } from "react";
import { Bot, CalendarDays, CircleGauge, Clock3, LayoutDashboard, PanelRightClose, X } from "lucide-react";
import openAiIcon from "./assets/brand-openai.svg";
import kimiIcon from "./assets/brand-kimi.svg";
import dshIcon from "./assets/brands/dsh.svg";
import { RollingNumber } from "./components/RollingNumber";
import { api } from "./lib/api";
import { displaySource, formatCost, totalsOf } from "./lib/analytics";
import { balancePercent, balanceTone, formatBalance } from "./lib/quota";
import type { QuotaAccount } from "./lib/types";
import { activeSeconds24hOf, formatActiveTime, formatWidgetTokens, hourlyUsageOf, todayBucketsOf, widgetSummaryOf, type WidgetSummary, type WidgetValueMetric } from "./lib/widget";
import { useMetera } from "./state/MeteraContext";

type Phase = "idle" | "active" | "fading" | "waiting" | "error";
type PanelMode = "hours" | "quota";

export function FloatingMeter() {
  const { buckets, sessions, settings, activity, quotas, updateSettings } = useMetera();
  const [nowMs, setNowMs] = useState(() => Date.now());
  const now = useMemo(() => new Date(nowMs), [nowMs]);
  const todayBuckets = useMemo(() => todayBucketsOf(buckets, now), [buckets, now]);
  const totals = useMemo(() => totalsOf(todayBuckets), [todayBuckets]);
  const [phase, setPhase] = useState<Phase>(activity.active ? "active" : "idle");
  const [panelMode, setPanelMode] = useState<PanelMode>(settings.widgetMetric === "quota" ? "quota" : "hours");
  const fadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const previousState = useRef(activity.state);

  useEffect(() => {
    if (fadeTimer.current) clearTimeout(fadeTimer.current);
    if (activity.state === "active") setPhase("active");
    else if (activity.state === "waiting" || activity.state === "error") setPhase(activity.state);
    else if (previousState.current === "active") {
      setPhase("fading");
      fadeTimer.current = setTimeout(() => setPhase("idle"), 3000);
    } else setPhase("idle");
    previousState.current = activity.state;
    return () => { if (fadeTimer.current) clearTimeout(fadeTimer.current); };
  }, [activity.state]);

  useEffect(() => {
    if (settings.widgetMetric === "quota") setPanelMode("quota");
    else setPanelMode("hours");
  }, [settings.widgetMetric]);

  useEffect(() => {
    const timer = setInterval(() => setNowMs(Date.now()), 60_000);
    return () => clearInterval(timer);
  }, []);

  // ---- 折叠/展开:后端全程编排(滑动 → 切内容 → 唯一一次 resize),
  // 前端零状态机,布局切换时的进场动画由 CSS 在挂载时自动播放。
  const beginCollapse = () => { void api.collapseWidget(); };
  const beginExpand = () => { void api.expandWidget(); };

  const metric: WidgetValueMetric = settings.widgetMetric === "cost" ? "cost" : "tokens";
  const hourlyUsage = useMemo(() => hourlyUsageOf(todayBuckets, now, metric), [todayBuckets, now, metric]);
  const summary = useMemo(() => widgetSummaryOf(buckets, sessions, now, metric), [buckets, sessions, now, metric]);
  const activeTime24h = useMemo(() => formatActiveTime(activeSeconds24hOf(sessions, now)), [sessions, now]);
  const pinned = settings.pinnedQuotaProviders
    .map(provider => quotas.find(item => item.provider === provider))
    .filter(Boolean)
    .slice(0, 2) as QuotaAccount[];

  if (settings.widgetCollapsed) {
    return <button className={`edge-tab edge-${settings.widgetEdge} ${phase}`} aria-label="展开 Metera 浮窗" title="展开 Metera" onClick={beginExpand}><span className="status-orb" /></button>;
  }

  const runningSources = activity.sources.length ? activity.sources.join("、") : activity.source ?? "Agent";
  const statusLabel = phase === "active" ? `${runningSources} 正在运行` : phase === "waiting" ? (activity.detail ?? `${runningSources} 正在等待确认`) : phase === "error" ? (activity.detail ?? `${runningSources} 运行出错`) : phase === "fading" ? "Agent 活动刚刚结束" : "当前没有 Agent 活动";

  if (settings.widgetCompact) {
    const compactQuota = (() => {
      const fiveHour = pinned[0]?.windows.find(window => window.windowMinutes === 300) ?? pinned[0]?.windows.find(window => window.label.includes("5") || window.label.includes("五"));
      return fiveHour?.remainingPercent ?? pinned[0]?.windows[0]?.remainingPercent ?? null;
    })();
    const compactValue = settings.widgetMetric === "cost"
      ? formatCost(totals.cost)
      : settings.widgetMetric === "quota"
        ? compactQuota == null ? "--" : `${compactQuota.toFixed(2)}%`
        : formatWidgetTokens(totals.tokens);
    const compactLengthClass = compactValue.length >= 9 ? " long" : compactValue.length >= 7 ? " medium" : "";
    return <div className="floating-meter floating-meter-compact" onPointerDown={startWindowDrag}>
      <button className={`compact-reading${compactLengthClass}`} onClick={() => void updateSettings({ widgetMetric: metric === "tokens" ? "cost" : "tokens" })} aria-label={`当前 ${compactValue}，点击切换显示项目`}>
        <RollingNumber value={compactValue} />
      </button>
      <AgentControl phase={phase} statusLabel={statusLabel} onCollapse={beginCollapse} />
    </div>;
  }

  const reading = splitReading(metric === "cost" ? formatCost(totals.cost) : formatWidgetTokens(totals.tokens), metric);
  const numberClass = reading.number.length >= 7 ? " long" : reading.number.length >= 6 ? " medium" : "";
  return <div className={`floating-meter floating-meter-ios metric-${metric}`} onPointerDown={startWindowDrag}>
    <div className="ios-window-drag-region" onPointerDown={startHeaderDrag} aria-hidden="true" title="拖动浮窗" />
    <button className={`ios-summary-card metric-${metric}`} onClick={() => {
      if (recentlyDragged()) return;
      void updateSettings({ widgetMetric: metric === "tokens" ? "cost" : "tokens" });
    }} aria-label={summaryLabel(reading.number, reading.accent, metric, summary)}>
      <div className="ios-calendar-date">
        <CalendarDays />
        <time dateTime={localDateKey(now)}>{calendarHeading(now)}</time>
        <span className={`ios-summary-status ${phase}`} aria-hidden="true" />
      </div>
      <div className={`ios-summary-reading${numberClass}`}>
        <strong><RollingNumber value={reading.number} className="ios-reading-value" /></strong>
        <span className={`ios-summary-unit unit-${metric}`}><b className={metric}>{reading.accent}</b><span>{reading.suffix}</span></span>
      </div>
      <SummaryMatrix summary={summary} />
    </button>

    <button className={`ios-detail-panel mode-${panelMode}`} onClick={() => {
      if (!recentlyDragged()) setPanelMode(current => current === "hours" ? "quota" : "hours");
    }} aria-label={`${panelMode === "hours" ? "显示订阅额度" : "显示今日二十四小时用量"}，最近二十四小时活跃 ${activeTime24h}`}>
      {panelMode === "hours" ? <>
        <header onPointerDown={startHeaderDrag}><span><Clock3 /></span><small>{activeTime24h}</small></header>
        <HourlyCapsules values={hourlyUsage} metric={metric} />
      </> : <>
        <header onPointerDown={startHeaderDrag}><span><CircleGauge /></span><small>{activeTime24h}</small></header>
        <QuotaPanel accounts={pinned} />
      </>}
    </button>
    <AgentControl phase={phase} statusLabel={statusLabel} showIndicator={false} onCollapse={beginCollapse} />
  </div>;
}

function startWindowDrag(event: React.PointerEvent<HTMLElement>) {
  if (event.button === 0 && !(event.target as HTMLElement).closest("button")) beginPointerWindowDrag(event);
}

function startHeaderDrag(event: React.PointerEvent<HTMLElement>) {
  if (event.button !== 0) return;
  event.stopPropagation();
  beginPointerWindowDrag(event);
}

let draggedUntil = 0;

function recentlyDragged() {
  return performance.now() < draggedUntil;
}

function beginPointerWindowDrag(event: React.PointerEvent<HTMLElement>) {
  event.preventDefault();
  draggedUntil = performance.now() + 800;
  void api.startWidgetDrag();
}

function splitReading(value: string, metric: WidgetValueMetric) {
  if (metric === "cost") return { number: value.replace(/^\$/, ""), accent: "$", suffix: "USD" };
  const match = value.match(/^(.+?)([KMB])$/);
  return match ? { number: match[1], accent: match[2], suffix: "Token" } : { number: value, accent: "", suffix: "Token" };
}

function HourlyCapsules({ values, metric }: { values: number[]; metric: WidgetValueMetric }) {
  const maximum = Math.max(...values, 0);
  const total = values.reduce((sum, value) => sum + value, 0);
  const renderRow = (offset: number, alignment: "morning" | "evening") => <div className={`ios-hour-row ${alignment}`}>
    {values.slice(offset, offset + 12).map((value, index) => {
      const normalized = maximum > 0 ? value / maximum : 0;
      const height = value > 0 ? 8 + normalized * 24 : 7;
      const label = `${String(offset + index).padStart(2, "0")} 时，${metric === "cost" ? formatCost(value) : formatWidgetTokens(value)}`;
      return <span className={value > 0 ? "has-usage" : "empty"} key={offset + index} title={label}><i style={{ height: `${height}px`, opacity: value > 0 ? .34 + normalized * .66 : .16 }} /></span>;
    })}
  </div>;
  return <div className="ios-hours-chart" role="img" aria-label={`今日二十四小时${metric === "cost" ? "花销" : "用量"}，合计 ${metric === "cost" ? formatCost(total) : formatWidgetTokens(total)}`}>
    <span className="ios-daypart morning">上午</span>
    {renderRow(0, "morning")}
    <div className="ios-hour-labels"><span className="ios-dual-hour"><b>0/</b><b>12</b></span>{Array.from({ length: 11 }, (_, index) => <span key={index + 1}>{index + 1}</span>)}</div>
    {renderRow(12, "evening")}
    <span className="ios-daypart evening">下午</span>
  </div>;
}

function SummaryMatrix({ summary }: { summary: WidgetSummary }) {
  const change = formatChange(summary.previousChange);
  const peak = summary.peakHour == null
    ? "--"
    : `${String(summary.peakHour).padStart(2, "0")}–${String((summary.peakHour + 1) % 24).padStart(2, "0")}`;
  const source = summary.leadingSource ? displaySource(summary.leadingSource.source) : "--";
  const share = summary.leadingSource ? `${(summary.leadingSource.share * 100).toFixed(2)}%` : undefined;
  return <div className="ios-summary-matrix" aria-hidden="true">
    <SummaryCell label="昨日同刻" value={change.text} tone={change.tone} />
    <SummaryCell label="会话" value={`${summary.sessionCount} 次`} color="blue" />
    <SummaryCell label="峰值时段" value={peak} color="orange" />
    <SummaryCell label="工具" value={source} sub={share} />
  </div>;
}

function SummaryCell({ label, value, sub, tone = "neutral", color }: { label: string; value: string; sub?: string; tone?: "positive" | "negative" | "neutral"; color?: "blue" | "orange" }) {
  return <span className={`ios-summary-cell ${tone}${color ? ` v-${color}` : ""}`}><strong>{value}</strong><small>{label}{sub && <em className="ios-cell-sub">{sub}</em>}</small></span>;
}

function formatChange(value: number | null) {
  if (value == null || !Number.isFinite(value)) return { text: "--", tone: "neutral" as const };
  if (Math.abs(value) < .005) return { text: "0.00%", tone: "neutral" as const };
  return { text: `${value > 0 ? "↑" : "↓"} ${Math.abs(value).toFixed(2)}%`, tone: value > 0 ? "positive" as const : "negative" as const };
}

function summaryLabel(number: string, accent: string, metric: WidgetValueMetric, summary: WidgetSummary) {
  const change = formatChange(summary.previousChange).text;
  const peak = summary.peakHour == null ? "不可用" : `${summary.peakHour} 至 ${(summary.peakHour + 1) % 24} 时`;
  const source = summary.leadingSource ? `${displaySource(summary.leadingSource.source)} ${(summary.leadingSource.share * 100).toFixed(2)}%` : "不可用";
  return `今日${metric === "tokens" ? "用量" : "花销"} ${number}${accent}；较昨日同刻 ${change}；今日 ${summary.sessionCount} 次会话；峰值 ${peak}；主要工具 ${source}。点击切换`;
}

function calendarHeading(now: Date) {
  const weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  return `${now.getMonth() + 1}月${now.getDate()}日 ${weekdays[now.getDay()]}`;
}

function localDateKey(now: Date) {
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
}

function QuotaPanel({ accounts }: { accounts: QuotaAccount[] }) {
  if (!accounts.length) return <p className="ios-quota-empty">请在仪表盘绑定额度账号</p>;
  return <div className="ios-quota-list">{accounts.map(account => <QuotaRow key={account.provider} account={account} />)}</div>;
}

function QuotaRow({ account }: { account: QuotaAccount }) {
  const fiveHour = account.windows.find(window => window.windowMinutes === 300) ?? account.windows.find(window => window.label.includes("5") || window.label.includes("五"));
  const week = account.windows.find(window => window.windowMinutes === 10080) ?? account.windows.find(window => window.label.includes("周") || window.label.includes("7 天"));
  const balance = account.windows.length === 0 ? account.credits?.balance : null;
  const icon = account.provider.toLowerCase().includes("kimi") ? kimiIcon : account.provider.toLowerCase().includes("codex") || account.provider.toLowerCase().includes("openai") ? openAiIcon : account.provider.toLowerCase().includes("deepseek") ? dshIcon : null;
  const providerClass = account.provider.toLowerCase().includes("kimi") ? "kimi" : account.provider.toLowerCase().includes("codex") || account.provider.toLowerCase().includes("openai") ? "openai" : "other";
  const displayName = account.name.split("/")[0].trim() || account.name;
  return <div className="ios-quota-row">
    <span className={`ios-provider-icon provider-${providerClass}`}>{icon ? <img src={icon} alt="" /> : <Bot />}</span>
    <div className="ios-quota-content">
      <strong>{displayName}</strong>
      <div className="ios-quota-lines">{balance ? <QuotaLine label="余额" balance={balance} /> : <><QuotaLine label="5 小时" value={fiveHour?.remainingPercent} /><QuotaLine label="一周" value={week?.remainingPercent} /></>}</div>
    </div>
  </div>;
}

function QuotaLine({ label, value, balance }: { label: string; value?: number | null; balance?: string | null }) {
  if (balance != null) {
    // 余额型账户：进度按余额百分比（100 元 = 100%），30 元黄 / 10 元红。
    const percent = balancePercent(balance) ?? 100;
    const tone = balanceTone(balancePercent(balance));
    const shown = formatBalance(balance);
    return <div className={`ios-quota-line ${tone}`} role="meter" aria-label={`${label}，${shown}`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(percent)}>
      <span>{label}</span><i><em style={{ width: `${percent}%` }} /></i><b>{shown}</b>
    </div>;
  }
  const normalized = value == null ? 0 : Math.max(0, Math.min(100, value));
  const tone = value == null ? "unknown" : normalized > 50 ? "good" : normalized >= 20 ? "warn" : "danger";
  const readableValue = value == null ? "额度暂不可用" : `剩余 ${normalized.toFixed(2)}%`;
  return <div className={`ios-quota-line ${tone}`} role="meter" aria-label={`${label}，${readableValue}`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={value == null ? undefined : Math.round(normalized)}>
    <span>{label}</span><i><em style={{ width: `${normalized}%` }} /></i><b>{value == null ? "--" : `${normalized.toFixed(2)}%`}</b>
  </div>;
}

function AgentControl({ phase, statusLabel, showIndicator = true, onCollapse }: { phase: Phase; statusLabel: string; showIndicator?: boolean; onCollapse: () => void }) {
  return <div className="agent-control">
    {showIndicator && <div className={`activity-indicator ${phase}`} role="status" aria-label={statusLabel} title={statusLabel}><span className="status-orb" /></div>}
    <div className="hover-actions">
      <button title="打开仪表盘" aria-label="打开仪表盘" onClick={() => void api.showDashboard()}><LayoutDashboard /></button>
      <button title="收起到屏幕边缘" aria-label="收起到屏幕边缘" onClick={onCollapse}><PanelRightClose /></button>
      <button title="关闭浮窗" aria-label="关闭浮窗" onClick={() => void api.closeWidget()}><X /></button>
    </div>
  </div>;
}
