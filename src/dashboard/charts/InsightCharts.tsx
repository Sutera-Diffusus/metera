import { useMemo, useState } from "react";
import { motion } from "motion/react";
import { formatCost, formatTokens } from "../../lib/analytics";
import type { UsageSession } from "../../lib/types";
import type { DailyInsight, ForecastPoint } from "../dashboardAnalytics";

type ReplayProps = { replayKey: number; replay: boolean };

const lineGeometry = (values: number[], width = 900, height = 270, left = 38, right = 22, top = 24, bottom = 34) => {
  const peak = Math.max(1, ...values);
  const points = values.map((value, index) => ({
    x: values.length <= 1 ? width / 2 : left + index * (width - left - right) / (values.length - 1),
    y: top + (height - top - bottom) * (1 - value / peak),
  }));
  return { points, path: points.map((point, index) => `${index ? "L" : "M"}${point.x},${point.y}`).join(" "), peak, width, height, left, right, top, bottom };
};

export function ForecastChart({ points, replayKey, replay }: { points: ForecastPoint[] } & ReplayProps) {
  const [mode, setMode] = useState<"tokens" | "cost">("tokens");
  const [active, setActive] = useState<number | null>(null);
  const usable = mode === "tokens" ? points : points.filter(point => point.cost != null);
  const values = usable.map(point => mode === "tokens" ? point.tokens : point.cost ?? 0);
  const lower = usable.map(point => mode === "tokens" ? point.tokenLow : point.costLow ?? 0);
  const upper = usable.map(point => mode === "tokens" ? point.tokenHigh : point.costHigh ?? 0);
  const geometry = lineGeometry(upper);
  const y = (value: number) => geometry.top + (geometry.height - geometry.top - geometry.bottom) * (1 - value / geometry.peak);
  const centerPath = usable.map((_, index) => `${index ? "L" : "M"}${geometry.points[index].x},${y(values[index])}`).join(" ");
  const band = usable.length ? `${usable.map((_, index) => `${index ? "L" : "M"}${geometry.points[index].x},${y(upper[index])}`).join(" ")} ${usable.slice().reverse().map((_, reverseIndex) => { const index = usable.length - 1 - reverseIndex; return `L${geometry.points[index].x},${y(lower[index])}`; }).join(" ")} Z` : "";
  const format = (value: number) => mode === "tokens" ? formatTokens(value) : formatCost(value);
  return <section className="v2-panel insight-panel insight-forecast">
    <span className="insight-range-badge">历史基线 90 天</span>
    <header className="v2-panel-header"><div><h2>未来七天消耗区间</h2><p>基于最近 30–90 天趋势与星期规律；阴影表示不确定范围</p></div><div className="v2-segmented"><button className={mode === "tokens" ? "active" : ""} onClick={() => { setMode("tokens"); setActive(null); }}>Token</button><button className={mode === "cost" ? "active" : ""} onClick={() => { setMode("cost"); setActive(null); }}>费用</button></div></header>
    {usable.length ? <div className="insight-chart-wrap"><div className="insight-readout"><strong className="num">{active == null ? format(values.reduce((sum, value) => sum + value, 0)) : format(values[active])}</strong><span>{active == null ? "预测合计" : usable[active].date}</span></div><svg className="insight-line-svg" viewBox={`0 0 ${geometry.width} ${geometry.height}`} onMouseLeave={() => setActive(null)} role="img" aria-label="未来七天用量预测">
      <defs><linearGradient id="apple-forecast-band" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stopColor="#8b7cf6" stopOpacity=".30"/><stop offset="1" stopColor="#ff64d8" stopOpacity=".12"/></linearGradient></defs>
      {[0, .25, .5, .75, 1].map(ratio => <line className="grid-line" key={ratio} x1={geometry.left} x2={geometry.width - geometry.right} y1={geometry.top + ratio * (geometry.height - geometry.top - geometry.bottom)} y2={geometry.top + ratio * (geometry.height - geometry.top - geometry.bottom)}/>) }
      <motion.path key={`band-${replayKey}-${mode}`} className="forecast-band" fill="url(#apple-forecast-band)" d={band} initial={replay ? { opacity: 0 } : false} animate={{ opacity: 1 }} transition={{ duration: .55 }}/>
      <motion.path key={`line-${replayKey}-${mode}`} className="insight-main-line" d={centerPath} initial={replay ? { pathLength: 0 } : false} animate={{ pathLength: 1 }} transition={{ duration: .85, ease: [0.16, 1, 0.3, 1] }}/>
      <path className="forecast-ants" d={centerPath}/>
      {usable.map((point, index) => <g key={point.date}><circle className="insight-hit" cx={geometry.points[index].x} cy={y(values[index])} r="14" tabIndex={0} onMouseEnter={() => setActive(index)} onFocus={() => setActive(index)} onBlur={() => setActive(null)} onClick={() => setActive(active === index ? null : index)}/><circle className={`insight-dot${active === index ? " active" : ""}`} cx={geometry.points[index].x} cy={y(values[index])} r={active === index ? 5 : 3}/><text x={geometry.points[index].x} y={geometry.height - 10} textAnchor="middle">{point.date}</text></g>)}
    </svg></div> : <div className="v2-empty">{mode === "cost" && points.length ? "计价覆盖不足 80%，不生成费用预测" : "至少需要 14 天历史数据才能生成预测"}</div>}
  </section>;
}

export function EfficiencyTrend({ days, replayKey, replay }: { days: DailyInsight[] } & ReplayProps) {
  const [mode, setMode] = useState<"cache" | "cost">("cache");
  const [active, setActive] = useState<number | null>(null);
  const points = days.slice(-60);
  const values = points.map(point => mode === "cache" ? point.cacheRate * 100 : point.costPerMillion);
  const geometry = lineGeometry(values);
  const format = (value: number) => mode === "cache" ? `${value.toFixed(1)}%` : formatCost(value);
  return <section className="v2-panel insight-panel insight-efficiency">
    <header className="v2-panel-header"><div><h2>{mode === "cache" ? "缓存效率如何变化" : "每百万 Token 成本"}</h2><p>{mode === "cache" ? "缓存 Token ÷（输入 Token + 缓存 Token）" : "仅基于可计价数据桶"}</p></div><div className="v2-segmented"><button className={mode === "cache" ? "active" : ""} onClick={() => setMode("cache")}>缓存率</button><button className={mode === "cost" ? "active" : ""} onClick={() => setMode("cost")}>成本效率</button></div></header>
    {points.length ? <div className="insight-chart-wrap"><div className="insight-readout"><strong>{format(active == null ? values.at(-1) ?? 0 : values[active])}</strong><span>{active == null ? "最近一天" : points[active].date}</span></div><svg className="insight-line-svg" viewBox={`0 0 ${geometry.width} ${geometry.height}`} onMouseLeave={() => setActive(null)}>
      {[0, .25, .5, .75, 1].map(ratio => <line className="grid-line" key={ratio} x1={geometry.left} x2={geometry.width - geometry.right} y1={geometry.top + ratio * (geometry.height - geometry.top - geometry.bottom)} y2={geometry.top + ratio * (geometry.height - geometry.top - geometry.bottom)}/>) }
      <motion.path key={`${replayKey}-${mode}`} className="insight-main-line" d={geometry.path} initial={replay ? { pathLength: 0 } : false} animate={{ pathLength: 1 }} transition={{ duration: .9, ease: [0.16, 1, 0.3, 1] }}/>
      {points.map((point, index) => <g key={point.date}><circle className="insight-hit" cx={geometry.points[index].x} cy={geometry.points[index].y} r="12" tabIndex={0} onMouseEnter={() => setActive(index)} onFocus={() => setActive(index)} onBlur={() => setActive(null)} onClick={() => setActive(active === index ? null : index)}/><circle className={`insight-dot${active === index ? " active" : ""}`} cx={geometry.points[index].x} cy={geometry.points[index].y} r={active === index ? 4.5 : 2.5}/></g>)}
    </svg></div> : <div className="v2-empty">当前范围暂无效率趋势</div>}
  </section>;
}

export function ContributionBars({ title, rows, selected, onSelect, replayKey, replay }: { title: string; rows: Array<{ name: string; label?: string; value: number }>; selected: string; onSelect(name: string): void } & ReplayProps) {
  const peak = Math.max(1, ...rows.map(row => row.value));
  return <section className="v2-panel insight-panel contribution-panel"><header className="v2-panel-header"><div><h2>{title}</h2><p>点击条目联动全局筛选</p></div><span>{rows.length} 项</span></header><div className="contribution-bars">{rows.slice(0, 8).map((row, index) => <button className={selected === row.name ? "active" : ""} key={row.name} onClick={() => onSelect(row.name)}><span><b>{String(index + 1).padStart(2, "0")}</b>{row.label ?? row.name}</span><strong>{formatTokens(row.value)}</strong><i><motion.em key={`${replayKey}-${row.name}`} initial={replay ? { scaleX: 0 } : false} animate={{ scaleX: row.value / peak }} transition={{ duration: .65, delay: index * .06, ease: [0.16, 1, 0.3, 1] }}/></i></button>)}</div></section>;
}

export function RatioRing({ input, output, replayKey, replay }: { input: number; output: number } & ReplayProps) {
  const total = input + output;
  const inputShare = total ? input / total : 0;
  return <section className="v2-panel insight-panel ratio-panel"><header className="v2-panel-header"><div><h2>输入与输出结构</h2><p>缓存计入输入，推理计入输出</p></div></header><div className="ratio-content"><div className="ratio-ring"><svg viewBox="0 0 140 140"><circle className="ratio-track" cx="70" cy="70" r="56"/><motion.circle key={replayKey} className="ratio-value" cx="70" cy="70" r="56" pathLength="1" initial={replay ? { pathLength: 0 } : false} animate={{ pathLength: inputShare }} transition={{ duration: .8, ease: [0.16, 1, 0.3, 1] }}/></svg><strong>{output ? `${(input / output).toFixed(1)}:1` : "--"}</strong><span>输入 : 输出</span></div><dl><div><dt>输入</dt><dd>{formatTokens(input)}</dd></div><div><dt>输出</dt><dd>{formatTokens(output)}</dd></div><div><dt>输入占比</dt><dd>{(inputShare * 100).toFixed(1)}%</dd></div></dl></div></section>;
}

export function PeakHours({ sessions, replayKey, replay }: { sessions: UsageSession[] } & ReplayProps) {
  const [active, setActive] = useState<number | null>(null);
  const hours = useMemo(() => { const result = Array(24).fill(0) as number[]; for (const session of sessions) session.userPromptHours.forEach((value, hour) => { result[hour] += value; }); return result; }, [sessions]);
  const peak = Math.max(1, ...hours);
  return <section className="v2-panel insight-panel peak-panel"><header className="v2-panel-header"><div><h2>提示集中在哪些时段</h2><p>每根刻度对应一小时的真实用户提示</p></div><span>{active == null ? `${hours.reduce((sum, value) => sum + value, 0)} 次` : `${String(active).padStart(2, "0")}:00 · ${hours[active]} 次`}</span></header><div className="peak-hours" onMouseLeave={() => setActive(null)}>{hours.map((value, hour) => <button key={hour} aria-label={`${hour}:00，${value} 次提示`} onMouseEnter={() => setActive(hour)} onFocus={() => setActive(hour)} onBlur={() => setActive(null)} onClick={() => setActive(active === hour ? null : hour)}><motion.i key={`${replayKey}-${hour}`} initial={replay ? { scaleY: 0 } : false} animate={{ scaleY: Math.max(.04, value / peak) }} transition={{ duration: .5, delay: hour * .018, ease: [0.16, 1, 0.3, 1] }}/><span>{hour % 3 === 0 ? String(hour).padStart(2, "0") : ""}</span></button>)}</div></section>;
}
