import { useMemo, useState } from "react";
import { motion } from "motion/react";
import { displaySource, formatCost, formatDuration, formatTokens } from "../../lib/analytics";
import { estimateCost } from "../../lib/pricing";
import type { UsageBucket } from "../../lib/types";
import { useMetera } from "../../state/MeteraContext";
import { sourceFill } from "../brands";
import { BrandIcon } from "../BrandIcon";

export type UsageMode = "tokens" | "cost" | "active";

const OTHER = "__other";
const ACTIVE = "__active";
const MAX_SOURCES = 6;

interface Part { source: string; value: number }
interface Slot { key: string; label: string; timestamp: number; total: number; parts: Part[] }

const periodKey = (date: Date, hourly: boolean) =>
  hourly ? `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}-${date.getHours()}` : `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;

const bucketTokens = (bucket: UsageBucket) =>
  bucket.inputTokens + bucket.cachedInputTokens + bucket.outputTokens + bucket.reasoningOutputTokens;

// §17.6 用量分析签名图形：按工具品牌色的堆叠柱 + 总计虚线。
// 数值一律在图旁伴随（汇总行 / hover 徽章），不标柱顶。
export function SourceStackedBars({ buckets, activeMap, range, mode, onMode }: {
  buckets: UsageBucket[];
  activeMap: Map<string, number>;
  range: string;
  mode: UsageMode;
  onMode(mode: UsageMode): void;
}) {
  const state = useMetera();
  const hourly = range === "today" || range === "24h";
  const [hovered, setHovered] = useState<number | null>(null);
  const [pinned, setPinned] = useState<number | null>(null);
  const activeIndex = pinned ?? hovered;

  const { slots, sources } = useMemo(() => {
    const byKey = new Map<string, { timestamp: number; label: string; parts: Map<string, number> }>();
    const bySource = new Map<string, number>();
    if (mode !== "active") {
      for (const bucket of buckets) {
        const date = new Date(bucket.bucketStart);
        const key = periodKey(date, hourly);
        const value = mode === "cost" ? estimateCost(bucket) ?? 0 : bucketTokens(bucket);
        const source = bucket.source || "unknown";
        const slot = byKey.get(key) ?? {
          timestamp: date.getTime(),
          label: hourly ? `${String(date.getHours()).padStart(2, "0")}:00` : `${date.getMonth() + 1}/${date.getDate()}`,
          parts: new Map<string, number>(),
        };
        slot.parts.set(source, (slot.parts.get(source) ?? 0) + value);
        byKey.set(key, slot);
        bySource.set(source, (bySource.get(source) ?? 0) + value);
      }
    }
    const ranked = [...bySource.entries()].sort((a, b) => b[1] - a[1]);
    const kept = ranked.slice(0, MAX_SOURCES).map(([name]) => name);
    const hasOther = ranked.length > MAX_SOURCES;
    const sourceList = mode === "active" ? [ACTIVE] : hasOther ? [...kept, OTHER] : kept;

    // 活跃模式没有源维度：按 periodKey 聚成单系列。
    const activeSlots = mode === "active"
      ? [...activeMap.entries()].map(([key, seconds]) => {
          const [y, m, d, h] = key.split("-").map(Number);
          return {
            key,
            timestamp: new Date(y, m, d, h ?? 0).getTime(),
            label: hourly ? `${String(h ?? 0).padStart(2, "0")}:00` : `${m + 1}/${d}`,
            parts: new Map([[ACTIVE, seconds]]),
          };
        })
      : [];

    const merged = new Map<string, { timestamp: number; label: string; parts: Map<string, number> }>(mode === "active" ? activeSlots.map(slot => [slot.key, slot]) : byKey);
    const slotList: Slot[] = [...merged.entries()].map(([key, slot]) => {
      const parts: Part[] = sourceList.map(source => {
        if (source === OTHER) {
          const value = ranked.slice(MAX_SOURCES).reduce((sum, [name]) => sum + (slot.parts.get(name) ?? 0), 0);
          return { source, value };
        }
        return { source, value: slot.parts.get(source) ?? 0 };
      }).filter(part => part.value > 0);
      return { key, label: slot.label, timestamp: slot.timestamp, total: parts.reduce((sum, part) => sum + part.value, 0), parts };
    }).sort((a, b) => a.timestamp - b.timestamp);
    return { slots: slotList, sources: sourceList };
  }, [buckets, activeMap, hourly, mode]);

  const width = 880, height = 320, top = 24, bottom = 34, left = 8, right = 8;
  const baseline = height - bottom;
  const innerW = width - left - right;
  const peak = Math.max(1, ...slots.map(slot => slot.total));
  const step = slots.length ? innerW / slots.length : innerW;
  const barW = Math.min(30, Math.max(5, step * .58));
  const xOf = (index: number) => left + step * (index + .5);
  const nowKey = periodKey(new Date(), hourly);
  const todayIndex = slots.findIndex(slot => slot.key === nowKey);

  const format = (value: number) => mode === "cost" ? formatCost(value) : mode === "active" ? formatDuration(value) : formatTokens(value);
  const sourceName = (source: string) => source === OTHER ? "其他" : source === ACTIVE ? "活跃时长" : displaySource(source);
  const partColor = (source: string) => source === OTHER ? "var(--brand-unknown)" : source === ACTIVE ? "var(--accent)" : sourceFill(source);

  const inspected = activeIndex == null ? null : slots[activeIndex];
  const topYOf = (slot: Slot) => baseline - slot.total / peak * (baseline - top);

  return <section className="v2-panel apple-stack-panel">
    <header className="apple-panel-header">
      <h2>用量趋势</h2>
      <div className="apple-stack-legend">
        {sources.map(source => <span key={source} className="apple-stack-legend-item">
          {source !== ACTIVE && source !== OTHER ? <BrandIcon brand={source} size={12}/> : <i style={{ background: partColor(source) }}/>}
          {sourceName(source)}
        </span>)}
      </div>
      <div className="v2-segmented" aria-label="趋势指标">
        {(["tokens", "cost", "active"] as UsageMode[]).map(value =>
          <button className={mode === value ? "active" : ""} key={value} onClick={() => onMode(value)}>{value === "tokens" ? "Token" : value === "cost" ? "费用" : "活跃"}</button>)}
      </div>
    </header>

    <div className="apple-stack-summary" aria-live="polite">
      <strong className="num">{inspected ? format(inspected.total) : format(slots.reduce((sum, slot) => sum + slot.total, 0))}</strong>
      <span>{inspected ? inspected.label : "当前范围合计"}</span>
      {pinned != null && <button onClick={() => setPinned(null)}>取消固定</button>}
    </div>

    {slots.length ? <div className="apple-stack-stage" onMouseLeave={() => setHovered(null)}>
      <svg className="apple-stack-svg" viewBox={`0 0 ${width} ${height}`} role="img" aria-label={`用量趋势，${slots.length} 个时段`}>
        {[0.25, 0.5, 0.75, 1].map(ratio =>
          <line key={ratio} className="grid-line" x1={left} x2={width - right} y1={top + ratio * (baseline - top)} y2={top + ratio * (baseline - top)}/>)}
        {todayIndex >= 0 && <rect className="today-band" x={left + step * todayIndex + 1} y={top - 10} width={step - 2} height={baseline - top + 14} rx={6}/>}

        {slots.map((slot, index) => {
          let y = baseline;
          return <g key={`${state.chartReplay}-${slot.key}`}
            tabIndex={0} role="button" className={`bar-group${activeIndex === index ? " active" : ""}`}
            aria-label={`${slot.label}，${format(slot.total)}，Enter 固定`}
            onMouseEnter={() => setHovered(index)} onFocus={() => setHovered(index)} onBlur={() => setHovered(null)}
            onClick={() => setPinned(pinned === index ? null : index)}
            onKeyDown={event => {
              if (event.key === "Enter" || event.key === " ") { event.preventDefault(); setPinned(pinned === index ? null : index); }
              if (event.key === "Escape") setPinned(null);
            }}>
            <rect className="bar-hit" x={left + step * index} y={top - 10} width={step} height={baseline - top + 10}/>
            {slot.parts.map((part, partIndex) => {
              const h = part.value / peak * (baseline - top);
              y -= h;
              const isTop = partIndex === slot.parts.length - 1;
              return <motion.rect key={part.source}
                x={xOf(index) - barW / 2} y={y} width={barW} height={Math.max(h, part.value > 0 ? 1.5 : 0)}
                rx={isTop ? Math.min(4, barW / 3) : 0}
                initial={state.chartReplayActive ? { scaleY: 0 } : false} animate={{ scaleY: 1 }}
                style={{ transformOrigin: `${xOf(index)}px ${baseline}px`, fill: partColor(part.source) }}
                transition={{ type: "spring", stiffness: 380, damping: 32, delay: Math.min(index, 40) * .018 }}/>;
            })}
          </g>;
        })}

        {/* 总计虚线（不标数值，只描走势） */}
        {slots.length > 1 && <polyline className="total-dash"
          points={slots.map((slot, index) => `${xOf(index)},${topYOf(slot) - 6}`).join(" ")}/>}

        {activeIndex != null && <line className="cursor" x1={xOf(activeIndex)} x2={xOf(activeIndex)} y1={top - 8} y2={baseline}/>}

        {slots.map((slot, index) => (index === 0 || index === slots.length - 1 || index % Math.max(1, Math.ceil(slots.length / 8)) === 0) &&
          <text key={slot.key} x={xOf(index)} y={height - 10} textAnchor={index === 0 ? "start" : index === slots.length - 1 ? "end" : "middle"}>{slot.label}</text>)}
      </svg>

      {inspected && <div className="apple-stack-badge" style={{ left: `${xOf(activeIndex!) / width * 100}%` }}>
        <strong className="num">{inspected.label} · {format(inspected.total)}</strong>
        {mode !== "active" && <ul>
          {inspected.parts.slice().sort((a, b) => b.value - a.value).slice(0, 4).map(part =>
            <li key={part.source}><i style={{ background: partColor(part.source) }}/><span>{sourceName(part.source)}</span><b className="num">{format(part.value)}</b></li>)}
          {inspected.parts.length > 4 && <li className="more">其余 {inspected.parts.length - 4} 个源</li>}
        </ul>}
      </div>}
    </div> : <div className="v2-empty">当前范围暂无趋势数据</div>}
  </section>;
}
