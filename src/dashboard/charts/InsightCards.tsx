import { Activity, Boxes, Database, Gauge, Ratio, WalletCards } from "lucide-react";
import { useMemo } from "react";
import { cacheHitRate, displaySource, formatCost, formatTokens, groupBuckets, totalsOf } from "../../lib/analytics";
import type { UsageBucket, UsageSession } from "../../lib/types";
import type { DailyInsight, SecondarySummary } from "../dashboardAnalytics";
import { sourceFill } from "../brands";

// §17.6 深度分析：六张指标卡，每卡一种微图（细环 / 三微柱 / 双向条 / 堆叠带 / 点列 / 迷你热力行），互不重复。
export function InsightCards({ buckets, sessions, days, summary }: {
  buckets: UsageBucket[];
  sessions: UsageSession[];
  days: DailyInsight[];
  summary: SecondarySummary;
}) {
  const totals = totalsOf(buckets);
  const cacheRate = cacheHitRate(totals.input, totals.cached);
  const input = totals.input + totals.cached;
  const output = totals.output + totals.reasoning;

  const costBars = useMemo(() => {
    const recent = days.slice(-3).map(day => day.costPerMillion);
    const peak = Math.max(1e-9, ...recent);
    return recent.map(value => value / peak);
  }, [days]);

  const toolStrip = useMemo(() => {
    const groups = groupBuckets(buckets, "source");
    const total = Math.max(1, groups.reduce((sum, group) => sum + group.tokens, 0));
    const top = groups.slice(0, 3).map(group => ({ name: group.name, share: group.tokens / total }));
    const rest = 1 - top.reduce((sum, group) => sum + group.share, 0);
    return rest > 0.001 ? [...top, { name: "__rest", share: rest }] : top;
  }, [buckets]);

  const sessionDots = useMemo(() => {
    const counts = sessions.map(session => session.messageCount).sort((a, b) => b - a).slice(0, 24);
    const peak = Math.max(1, ...counts);
    return counts.map(count => .25 + .75 * count / peak);
  }, [sessions]);

  const hourRow = useMemo(() => {
    const hours = Array(24).fill(0) as number[];
    for (const session of sessions) session.userPromptHours.forEach((value, hour) => { hours[hour] += value; });
    const ranked = hours.map((value, hour) => ({ value, hour })).sort((a, b) => b.value - a.value).slice(0, 3).map(item => item.hour);
    const peak = Math.max(1, ...hours);
    return hours.map((value, hour) => ({ scale: Math.max(.06, value / peak), hot: ranked.includes(hour) }));
  }, [sessions]);

  return <div className="apple-insight-cards">
    {/* 1 缓存命中率 · 细环 */}
    <section className="v2-panel apple-insight-card">
      <header><Database/><span>缓存命中率</span></header>
      <strong className="num">{(cacheRate * 100).toFixed(1)}%</strong>
      <div className="micro micro-ring" role="img" aria-label={`缓存命中率 ${(cacheRate * 100).toFixed(1)}%`}>
        <svg viewBox="0 0 44 44"><circle className="track" cx="22" cy="22" r="18"/><circle className="value" cx="22" cy="22" r="18" pathLength={1} strokeDasharray={`${cacheRate} 1`}/></svg>
        <small>{formatTokens(totals.cached)} 缓存 Token</small>
      </div>
    </section>

    {/* 2 每百万 Token 成本 · 三微柱（近三日） */}
    <section className="v2-panel apple-insight-card">
      <header><WalletCards/><span>每百万 Token 成本</span></header>
      <strong className="num">{summary.pricingCoverage ? formatCost(summary.costPerMillion) : "--"}</strong>
      <div className="micro micro-bars" role="img" aria-label="近三日成本">
        {costBars.length ? costBars.map((scale, index) => <i key={index} style={{ transform: `scaleY(${Math.max(.08, scale)})` }}/>) : <small>暂无历史</small>}
        <small>{(summary.pricingCoverage * 100).toFixed(0)}% 计价覆盖</small>
      </div>
    </section>

    {/* 3 输入输出比 · 双向条 */}
    <section className="v2-panel apple-insight-card">
      <header><Ratio/><span>输入输出比</span></header>
      <strong className="num">{output ? `${(input / output).toFixed(1)}:1` : "--"}</strong>
      <div className="micro micro-duo" role="img" aria-label={`输入 ${formatTokens(input)}，输出 ${formatTokens(output)}`}>
        <div className="duo-bar"><i className="duo-in" style={{ width: `${input + output ? input / (input + output) * 100 : 50}%` }}/><i className="duo-out"/></div>
        <small>{formatTokens(input)} / {formatTokens(output)}</small>
      </div>
    </section>

    {/* 4 工具集中度 · 堆叠带（品牌色） */}
    <section className="v2-panel apple-insight-card">
      <header><Boxes/><span>工具集中度</span></header>
      <strong className="num">{(summary.topToolShare * 100).toFixed(1)}%</strong>
      <div className="micro micro-strip" role="img" aria-label={`工具集中度 ${(summary.topToolShare * 100).toFixed(1)}%`}>
        <div className="strip">{toolStrip.map(part => <i key={part.name} title={part.name === "__rest" ? "其余工具" : displaySource(part.name)} style={{ width: `${part.share * 100}%`, background: part.name === "__rest" ? "var(--brand-unknown)" : sourceFill(part.name) }}/>)}</div>
        <small>供应商 {(summary.topProviderShare * 100).toFixed(1)}%</small>
      </div>
    </section>

    {/* 5 每会话 Token · 点列 */}
    <section className="v2-panel apple-insight-card">
      <header><Activity/><span>每会话 Token</span></header>
      <strong className="num">{formatTokens(summary.tokensPerSession)}</strong>
      <div className="micro micro-dots" role="img" aria-label={`${sessions.length} 个会话`}>
        {sessionDots.length ? sessionDots.map((opacity, index) => <i key={index} style={{ opacity }}/>) : <small>暂无会话</small>}
        <small>{sessions.length} 个会话</small>
      </div>
    </section>

    {/* 6 峰值时段集中度 · 迷你热力行 */}
    <section className="v2-panel apple-insight-card">
      <header><Gauge/><span>峰值时段集中度</span></header>
      <strong className="num">{(summary.peakHourShare * 100).toFixed(1)}%</strong>
      <div className="micro micro-hours" role="img" aria-label="24 小时提示分布">
        {hourRow.map((hour, index) => <i key={index} className={hour.hot ? "hot" : ""} style={{ transform: `scaleY(${hour.scale})` }}/>)}
        <small>最高三个小时</small>
      </div>
    </section>
  </div>;
}
