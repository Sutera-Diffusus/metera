import { useMemo, useState } from "react";
import { motion } from "motion/react";
import { formatCost, formatDuration, formatTokens } from "../../lib/analytics";
import { useMetera } from "../../state/MeteraContext";

export type TrendMode = "tokens" | "cost" | "active";
interface Point { key: string; label: string; tokens: number; cost: number; active: number; }

export function InteractiveTrend({ points, mode, onMode, title = "用量趋势" }: { points: Point[]; mode: TrendMode; onMode(mode: TrendMode): void; title?: string }) {
  const state = useMetera();
  const [hovered, setHovered] = useState<number | null>(null);
  const [pinned, setPinned] = useState<number | null>(null);
  const activeIndex = pinned ?? hovered;
  const width = 820, height = 260, left = 24, right = 18, top = 22, bottom = 34;
  const values = points.map(point => point[mode]);
  const peak = Math.max(1, ...values);
  const coords = useMemo(() => points.map((_, index) => ({
    x: points.length <= 1 ? width / 2 : left + index * (width - left - right) / (points.length - 1),
    y: top + (height - top - bottom) * (1 - values[index] / peak),
  })), [points, values.join("|"), peak]);
  const line = coords.map((point, index) => `${index ? "L" : "M"}${point.x},${point.y}`).join(" ");
  const area = coords.length ? `${line} L${coords.at(-1)!.x},${height - bottom} L${coords[0].x},${height - bottom} Z` : "";
  const format = (value: number) => mode === "cost" ? formatCost(value) : mode === "active" ? formatDuration(value) : formatTokens(value);
  const inspected = activeIndex == null ? null : points[activeIndex];
  return <section className="v2-panel v2-trend-panel">
    <header className="v2-panel-header"><div><h2>{title}</h2></div><div className="v2-segmented" aria-label="趋势指标">{(["tokens", "cost", "active"] as TrendMode[]).map(value => <button className={mode === value ? "active" : ""} key={value} onClick={() => onMode(value)}>{value === "tokens" ? "Token" : value === "cost" ? "费用" : "活跃"}</button>)}</div></header>
    <div className="v2-trend-summary" aria-live="polite"><strong>{inspected ? format(inspected[mode]) : format(values.reduce((sum, value) => sum + value, 0))}</strong><span>{inspected ? inspected.label : "当前范围合计"}</span>{pinned != null && <button onClick={() => setPinned(null)}>取消固定</button>}</div>
    {points.length ? <svg className="v2-trend-svg" viewBox={`0 0 ${width} ${height}`} role="img" aria-label={`${title}，${points.length} 个数据点`} onMouseLeave={() => setHovered(null)}>
      <defs><linearGradient id="v2-trend-fill" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="var(--v2-green)" stopOpacity=".26"/><stop offset="1" stopColor="var(--v2-green)" stopOpacity="0"/></linearGradient></defs>
      {[0, .25, .5, .75, 1].map(ratio => <line key={ratio} className="grid-line" x1={left} x2={width - right} y1={top + ratio * (height - top - bottom)} y2={top + ratio * (height - top - bottom)}/>) }
      <motion.path key={`area-${state.chartReplay}-${mode}`} className="trend-area" d={area} initial={state.chartReplayActive ? { opacity: 0 } : false} animate={{ opacity: 1 }} transition={{ duration: .5 }}/><motion.path key={`line-${state.chartReplay}-${mode}`} className="trend-line" d={line} initial={state.chartReplayActive ? { pathLength: 0 } : false} animate={{ pathLength: 1 }} transition={{ duration: .85, ease: [0.16, 1, 0.3, 1] }}/>
      {coords.map((coord, index) => <g key={points[index].key}><circle className={`trend-hit${activeIndex === index ? " active" : ""}`} cx={coord.x} cy={coord.y} r="13" tabIndex={0} role="button" aria-label={`${points[index].label}，${format(values[index])}`} onMouseEnter={() => setHovered(index)} onFocus={() => setHovered(index)} onBlur={() => setHovered(null)} onClick={() => setPinned(pinned === index ? null : index)} onKeyDown={event => { if (event.key === "Escape") setPinned(null); }}/><circle className="trend-dot" cx={coord.x} cy={coord.y} r={activeIndex === index ? 4.5 : 2.5}/></g>)}
      {activeIndex != null && <line className="trend-cursor" x1={coords[activeIndex].x} x2={coords[activeIndex].x} y1={top} y2={height - bottom}/>}
      {points.map((point, index) => (index === 0 || index === points.length - 1 || index % Math.max(1, Math.ceil(points.length / 6)) === 0) && <text key={point.key} x={coords[index].x} y={height - 10} textAnchor={index === 0 ? "start" : index === points.length - 1 ? "end" : "middle"}>{point.label}</text>)}
    </svg> : <div className="v2-empty">当前范围暂无趋势数据</div>}
  </section>;
}
