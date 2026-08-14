import { useState, type PointerEvent } from "react";
import { formatCost, formatTokens } from "../../lib/analytics";

export type OverviewMetric = "tokens" | "cost";

interface OverviewTrendChartProps {
  today: number[];
  previous: number[];
  metric: OverviewMetric;
  now: Date;
}

const WIDTH = 820;
const HEIGHT = 300;
const PLOT_LEFT = 64;
const PLOT_RIGHT = 18;
const PLOT_TOP = 22;
const PLOT_BOTTOM = 48;
const PLOT_WIDTH = WIDTH - PLOT_LEFT - PLOT_RIGHT;
const PLOT_HEIGHT = HEIGHT - PLOT_TOP - PLOT_BOTTOM;

const formatMetric = (value: number, metric: OverviewMetric) => metric === "cost"
  ? formatCost(value)
  : formatTokens(value);

const pointX = (index: number) => PLOT_LEFT + (index / 23) * PLOT_WIDTH;

const pointY = (value: number, max: number) => PLOT_TOP + PLOT_HEIGHT - (value / max) * PLOT_HEIGHT;

const linePath = (values: number[], max: number) => values
  .map((value, index) => `${index === 0 ? "M" : "L"} ${pointX(index).toFixed(2)} ${pointY(value, max).toFixed(2)}`)
  .join(" ");

const areaPath = (values: number[], max: number) => {
  const first = pointX(0);
  const last = pointX(values.length - 1);
  const baseline = PLOT_TOP + PLOT_HEIGHT;
  return `M ${first.toFixed(2)} ${baseline} ${values.map((value, index) => `L ${pointX(index).toFixed(2)} ${pointY(value, max).toFixed(2)}`).join(" ")} L ${last.toFixed(2)} ${baseline} Z`;
};

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

export function OverviewTrendChart({ today, previous, metric, now }: OverviewTrendChartProps) {
  const [hovered, setHovered] = useState<number | null>(null);
  const maximum = Math.max(...today, ...previous, 0);
  const scaleMax = maximum > 0 ? maximum * 1.15 : 1;
  const currentPosition = clamp(now.getHours() + now.getMinutes() / 60, 0, 23);
  const currentX = PLOT_LEFT + (currentPosition / 23) * PLOT_WIDTH;
  const tooltipX = hovered == null ? 0 : clamp(pointX(hovered) - 78, PLOT_LEFT, WIDTH - PLOT_RIGHT - 156);

  const yTicks = [0, 0.25, 0.5, 0.75, 1];
  const xTicks = [0, 3, 6, 9, 12, 15, 18, 21];
  const hasData = maximum > 0;

  const handlePointerMove = (event: PointerEvent<SVGSVGElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const ratio = clamp((event.clientX - rect.left) / rect.width, 0, 1);
    setHovered(clamp(Math.round(ratio * 23), 0, 23));
  };

  return <div className={`overview-trend-chart metric-${metric}`}>
    <svg
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      role="img"
      aria-label="今日与昨日同刻的 24 小时用量趋势"
      onPointerMove={handlePointerMove}
      onPointerLeave={() => setHovered(null)}
    >
      <defs>
        <linearGradient id="overview-trend-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="var(--overview-chart-accent)" stopOpacity=".22"/>
          <stop offset="1" stopColor="var(--overview-chart-accent)" stopOpacity="0"/>
        </linearGradient>
      </defs>
      {yTicks.map((fraction) => {
        const y = PLOT_TOP + PLOT_HEIGHT - fraction * PLOT_HEIGHT;
        return <g key={fraction}>
          <line x1={PLOT_LEFT} x2={WIDTH - PLOT_RIGHT} y1={y} y2={y} className="overview-chart-grid"/>
        </g>;
      })}
      {xTicks.map((hour) => <text key={hour} x={pointX(hour)} y={HEIGHT - 15} textAnchor="middle" className="overview-chart-axis">
        {String(hour).padStart(2, "0")}:00
      </text>)}
      {hasData && <>
        <path d={areaPath(today, scaleMax)} className="overview-trend-area"/>
        <path d={linePath(previous, scaleMax)} className="overview-trend-previous"/>
        <path d={linePath(today, scaleMax)} className="overview-trend-today"/>
      </>}
      <line x1={currentX} x2={currentX} y1={PLOT_TOP} y2={PLOT_TOP + PLOT_HEIGHT} className="overview-chart-now"/>
      <rect
        x={PLOT_LEFT}
        y={PLOT_TOP}
        width={PLOT_WIDTH}
        height={PLOT_HEIGHT}
        className="overview-chart-hit-area"
        aria-label="查看对应小时用量"
      />
      {hovered != null && <g className="overview-chart-tooltip" pointerEvents="none">
        <rect x={tooltipX} y={8} width="156" height="56" rx="10"/>
        <text x={tooltipX + 12} y={29} className="overview-chart-tooltip-time">
          {String(hovered).padStart(2, "0")}:00 时段
        </text>
        <text x={tooltipX + 12} y={49} className="overview-chart-tooltip-value">
          今日 {formatMetric(today[hovered] ?? 0, metric)} · 昨日 {formatMetric(previous[hovered] ?? 0, metric)}
        </text>
        <circle cx={pointX(hovered)} cy={pointY(today[hovered] ?? 0, scaleMax)} r="4" className="overview-chart-tooltip-dot"/>
      </g>}
      {!hasData && <text x={WIDTH / 2} y={PLOT_TOP + PLOT_HEIGHT / 2} textAnchor="middle" className="overview-chart-empty">
        今天还没有可绘制的用量
      </text>}
    </svg>
    <div className="overview-trend-legend" aria-hidden="true">
      <span><i className="today"/>今天</span>
      <span><i className="previous"/>昨日同刻</span>
      <span className="now">当前 {String(now.getHours()).padStart(2, "0")}:{String(now.getMinutes()).padStart(2, "0")}</span>
    </div>
  </div>;
}

export function OverviewDailyStrip({ values, metric, now }: { values: number[]; metric: OverviewMetric; now: Date }) {
  const maximum = Math.max(...values, 0);
  const total = values.reduce((sum, value) => sum + value, 0);
  return <div className={`overview-daily-strip metric-${metric}`}>
    <div className="overview-daily-strip-header"><span>近 7 日节奏</span><strong className="num">{formatMetric(total, metric)} 总量</strong></div>
    <div className="overview-daily-bars" role="img" aria-label="近 7 日用量节奏">
      {values.map((value, index) => {
        const date = new Date(now);
        date.setHours(0, 0, 0, 0);
        date.setDate(date.getDate() - (values.length - 1 - index));
        const label = index === values.length - 1 ? "今天" : `${date.getMonth() + 1}/${date.getDate()}`;
        return <div className={`overview-daily-bar${index === values.length - 1 ? " today" : ""}`} key={label} title={`${label} · ${formatMetric(value, metric)}`}>
          <div className="overview-daily-bar-track"><i style={{ height: `${maximum > 0 ? Math.max(4, (value / maximum) * 100) : 4}%` }}/></div>
          <span>{label}</span>
          <strong className="num">{formatMetric(value, metric)}</strong>
        </div>;
      })}
    </div>
  </div>;
}
