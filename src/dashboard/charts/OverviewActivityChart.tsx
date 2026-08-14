import { useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { formatTokens } from "../../lib/analytics";
import type { RangeKey } from "../../lib/types";
import { monotoneCurve } from "./overviewChartGeometry";

export type OverviewMetric = "tokens" | "cost";

export interface OverviewActivityPoint {
  hour: number;
  usage: number;
  activeMinutes: number;
}

const chartWidth = 1000;
const chartHeight = 350;
const plot = { left: 0, right: 1000, top: 38, bottom: 276 };
const plotWidth = plot.right - plot.left;
const plotHeight = plot.bottom - plot.top;

const money = (value: number, currency: "USD" | "CNY") => `${currency === "CNY" ? "¥" : "$"}${value.toFixed(2)}`;
const usageLabel = (value: number, metric: OverviewMetric, currency: "USD" | "CNY") => metric === "cost" ? money(value, currency) : formatTokens(value);
export function OverviewActivityChart({ points, metric, now, range, rangeLabel, currency }: {
  points: OverviewActivityPoint[];
  metric: OverviewMetric;
  now: Date;
  range: RangeKey;
  rangeLabel: string;
  currency: "USD" | "CNY";
}) {
  const [hovered, setHovered] = useState<number | null>(null);
  const [compact, setCompact] = useState(false);
  useEffect(() => {
    const media = window.matchMedia?.("(max-width: 720px)");
    if (!media) return;
    const sync = () => setCompact(media.matches);
    sync();
    media.addEventListener?.("change", sync);
    return () => media.removeEventListener?.("change", sync);
  }, []);
  const usageMax = Math.max(1, ...points.map(point => point.usage));
  const activeScaleMax = 60;
  const step = plotWidth / 24;
  const xForBar = (hour: number) => plot.left + (hour + .5) * step;
  const xForLine = (hour: number) => plot.left + (hour / 23) * plotWidth;
  const barWidth = Math.max(10, step * .56);
  const activePath = useMemo(() => monotoneCurve(points.map(point => ({
    x: xForLine(point.hour),
    y: plot.bottom - (point.activeMinutes / activeScaleMax) * plotHeight,
  }))), [points]);
  const activeAreaPath = activePath ? `${activePath} L ${plot.right} ${plot.bottom} L ${plot.left} ${plot.bottom} Z` : "";
  const currentHour = range === "today" || range === "24h" ? now.getHours() : null;
  const hoveredPoint = hovered == null ? null : points[hovered];
  const tooltipX = hoveredPoint ? Math.min(plot.right - 160, Math.max(plot.left + 8, xForBar(hoveredPoint.hour) - 76)) : 0;
  const pointLabel = (point: OverviewActivityPoint) => `${String(point.hour).padStart(2, "0")}:00，用量 ${usageLabel(point.usage, metric, currency)}，活跃 ${Math.round(point.activeMinutes)} 分钟`;

  return <div className={`overview-v4-activity-chart ${metric} ${currency.toLowerCase()}`}>
    <div className="overview-v4-axis-hints" aria-hidden="true">
      <span>{metric === "cost" ? "花费" : "Token"}</span><span>活跃分钟</span>
    </div>
    <svg className="overview-v4-activity-svg" viewBox={`0 0 ${chartWidth} ${chartHeight}`} preserveAspectRatio={compact ? "none" : "xMidYMid meet"} role="img" aria-label={`${rangeLabel}每小时用量与活跃时间`} onMouseLeave={() => setHovered(null)}>
      <defs>
        <linearGradient id="overview-v5-active-area" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="var(--overview-v4-active)" stopOpacity=".18"/><stop offset="1" stopColor="var(--overview-v4-active)" stopOpacity="0"/></linearGradient>
        <linearGradient id="overview-v5-bar-past" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="#a99cff"/><stop offset=".48" stopColor="#7467d9"/><stop offset="1" stopColor="#39345f"/></linearGradient>
        <linearGradient id="overview-v5-bar-current" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="#8dffe1"/><stop offset=".45" stopColor="#58dfb6"/><stop offset="1" stopColor="#247b69"/></linearGradient>
        <linearGradient id="overview-v5-bar-future" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="#655d9b"/><stop offset="1" stopColor="#2b2943"/></linearGradient>
      </defs>
      <g className="overview-v4-grid-lines" aria-hidden="true">
        {[0, .25, .5, .75, 1].map(ratio => <line key={ratio} x1={plot.left} x2={plot.right} y1={plot.bottom - ratio * plotHeight} y2={plot.bottom - ratio * plotHeight}/>) }
      </g>
      <g className="overview-v4-bars" onMouseMove={event => {
        const rect = event.currentTarget.getBoundingClientRect();
        const ratio = Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width));
        setHovered(Math.min(23, Math.max(0, Math.floor(ratio * 24))));
      }}>
        {points.map(point => {
          const barHeight = point.usage > 0 ? Math.max(2, (point.usage / usageMax) * plotHeight) : 0;
          const x = xForBar(point.hour) - barWidth / 2;
          const phase = currentHour == null || point.hour < currentHour ? "past" : point.hour === currentHour ? "current" : "future";
          return <motion.rect key={point.hour} className={`overview-v4-activity-bar ${phase}${hovered === point.hour ? " hovered" : ""}`} x={x} width={barWidth} rx={barWidth / 2} ry={barWidth / 2} initial={false} animate={{ y: plot.bottom - barHeight, height: barHeight }} transition={{ duration: .5, ease: [0.22, 1, .36, 1] }} onMouseEnter={() => setHovered(point.hour)} onFocus={() => setHovered(point.hour)} onBlur={() => setHovered(null)} onKeyDown={event => { if (event.key === "Escape") setHovered(null); }} tabIndex={0} role="button" aria-label={pointLabel(point)} />;
        })}
      </g>
      <motion.path key={`${metric}-${currency}-${range}-area`} className="overview-v5-active-area" d={activeAreaPath} initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: .34, ease: [0.16, 1, 0.3, 1] }} />
      <motion.path key={`${metric}-${currency}-${range}-line`} className="overview-v4-active-line" d={activePath} fill="none" initial={{ pathLength: 0, opacity: 0 }} animate={{ pathLength: 1, opacity: 1 }} transition={{ duration: .62, delay: .05, ease: [0.16, 1, 0.3, 1] }} />
      {points.map(point => <circle key={`point-${point.hour}`} className="overview-v4-active-hit" cx={xForLine(point.hour)} cy={plot.bottom - (point.activeMinutes / activeScaleMax) * plotHeight} r={Math.max(11, step / 2)} onMouseEnter={() => setHovered(point.hour)} aria-hidden="true" />)}
      <g className="overview-v4-x-axis" aria-hidden="true">
        {Array.from({ length: 7 }, (_, index) => index * 4).map(hour => <text key={hour} x={plot.left + (hour / 24) * plotWidth} y={plot.bottom + 45} textAnchor={hour === 0 ? "start" : hour === 24 ? "end" : "middle"}>{String(hour).padStart(2, "0")}:00</text>)}
      </g>
      <AnimatePresence initial={false}>
        {hoveredPoint && <motion.g className="overview-v4-chart-tooltip" pointerEvents="none" initial={{ opacity: 0, y: 5, filter: "blur(4px)" }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }} exit={{ opacity: 0, y: -3, filter: "blur(4px)" }} transition={{ duration: .16, ease: [0.16, 1, 0.3, 1] }}>
          <rect x={tooltipX} y={plot.top + 8} width="152" height="72" rx="10"/>
          <text x={tooltipX + 12} y={plot.top + 28} className="tooltip-title">{String(hoveredPoint.hour).padStart(2, "0")}:00</text>
          <text x={tooltipX + 12} y={plot.top + 48}>用量 <tspan>{usageLabel(hoveredPoint.usage, metric, currency)}</tspan></text>
          <text x={tooltipX + 12} y={plot.top + 65}>活跃 <tspan>{Math.round(hoveredPoint.activeMinutes)} min</tspan></text>
        </motion.g>}
      </AnimatePresence>
    </svg>
    <div className="overview-v5-mobile-axis" aria-hidden="true"><span>00:00</span><span>12:00</span><span>24:00</span></div>
  </div>;
}
