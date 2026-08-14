import { useState } from "react";
import { REASONIX_GRADIENT, sourceFill } from "../brands";
import { BrandIcon } from "../BrandIcon";
import { displaySource, formatCost, formatTokens } from "../../lib/analytics";

const CX = 90;
const CY = 90;
const R = 66;

const polar = (angle: number): [number, number] => [CX + R * Math.cos(angle), CY + R * Math.sin(angle)];
const arcPath = (start: number, end: number) => {
  const [sx, sy] = polar(start);
  const [ex, ey] = polar(end);
  return `M ${sx.toFixed(2)} ${sy.toFixed(2)} A ${R} ${R} 0 ${end - start > Math.PI ? 1 : 0} 1 ${ex.toFixed(2)} ${ey.toFixed(2)}`;
};

export interface DonutRow { source: string; tokens: number; cost: number | null }

// §17.6 今日构成：轨道环 donut（间隙 + 圆头端点 + hover 膨胀隔离），数值在右侧图例行。
export function CompositionDonut({ rows, onInspect }: {
  rows: DonutRow[];
  onInspect?(source: string): void;
}) {
  const [hovered, setHovered] = useState<string | null>(null);
  const total = rows.reduce((sum, row) => sum + row.tokens, 0);
  const totalCost = !rows.length || rows.some(row => row.cost == null) ? null : rows.reduce((sum, row) => sum + (row.cost ?? 0), 0);
  const gap = rows.filter(row => row.tokens > 0).length > 1 ? (4 * Math.PI) / 180 : 0;

  let cursor = -Math.PI / 2;
  const segments = total > 0 ? rows.filter(row => row.tokens > 0).map(row => {
    const span = (row.tokens / total) * Math.PI * 2;
    const start = cursor + gap / 2;
    const end = cursor + span - gap / 2;
    cursor += span;
    return { row, start, end: Math.max(start + .03, end) };
  }) : [];

  return <div className="donut-wrap">
    <div className="donut-figure">
      <svg viewBox="0 0 180 180" role="img" aria-label="今日用量构成">
        <defs>
          <linearGradient id="donut-reasonix" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor={REASONIX_GRADIENT[0]}/>
            <stop offset=".5" stopColor={REASONIX_GRADIENT[1]}/>
            <stop offset="1" stopColor={REASONIX_GRADIENT[2]}/>
          </linearGradient>
        </defs>
        {segments.length === 0 && <circle cx={CX} cy={CY} r={R} className="donut-empty-ring"/>}
        {segments.map(({ row, start, end }) => <path
          key={row.source}
          d={arcPath(start, end)}
          className={`donut-arc${hovered === row.source ? " hover" : ""}`}
          style={{ stroke: row.source === "reasonix" ? "url(#donut-reasonix)" : sourceFill(row.source) }}
          strokeOpacity={hovered && hovered !== row.source ? .3 : 1}
          onPointerEnter={() => setHovered(row.source)}
          onPointerLeave={() => setHovered(null)}
        >
          <title>{`${displaySource(row.source)} · ${formatTokens(row.tokens)}`}</title>
        </path>)}
      </svg>
      <div className="donut-center">
        <strong className="num">{totalCost != null ? formatCost(totalCost) : "--"}</strong>
        <small>今日预估费用</small>
      </div>
    </div>
    <div className="donut-legend">
      {rows.map(row => {
        const share = total > 0 ? (row.tokens / total) * 100 : 0;
        return <button type="button" key={row.source}
          className={`donut-legend-row${hovered === row.source ? " hover" : ""}`}
          style={{ opacity: hovered && hovered !== row.source ? .45 : 1 }}
          onPointerEnter={() => setHovered(row.source)}
          onPointerLeave={() => setHovered(null)}
          onClick={() => onInspect?.(row.source)}
          disabled={!onInspect}>
          <span className="donut-legend-icon" style={{ color: sourceFill(row.source) }}>
            <BrandIcon brand={row.source} size={15}/>
          </span>
          <span className="donut-legend-name">{displaySource(row.source)}</span>
          <span className="donut-legend-value num">{formatTokens(row.tokens)}</span>
          <span className="donut-legend-cost num">{row.cost != null && row.cost > 0 ? formatCost(row.cost) : "--"}</span>
          <span className="donut-legend-share num">{share.toFixed(1)}%</span>
        </button>;
      })}
      {!rows.length && <div className="v2-empty compact">今日暂无用量</div>}
    </div>
  </div>;
}
