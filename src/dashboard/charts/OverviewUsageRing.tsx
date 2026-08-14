import { useState, type CSSProperties } from "react";
import { AnimatePresence, motion } from "motion/react";
import { formatTokens } from "../../lib/analytics";
import { BrandIcon } from "../BrandIcon";
import type { OverviewMetric } from "./OverviewActivityChart";

export interface OverviewDistributionRow {
  key: string;
  label: string;
  value: number;
  share: number;
  tokens: number;
  cost: number;
  color: string;
  iconBrand: string | null;
}

const money = (value: number, currency: "USD" | "CNY") => `${currency === "CNY" ? "¥" : "$"}${value.toFixed(2)}`;
const pointOnRing = (cx: number, cy: number, radius: number, angle: number) => {
  const radians = angle * Math.PI / 180;
  return { x: cx + radius * Math.cos(radians), y: cy + radius * Math.sin(radians) };
};
const ringArc = (cx: number, cy: number, radius: number, start: number, end: number) => {
  const from = pointOnRing(cx, cy, radius, start);
  const to = pointOnRing(cx, cy, radius, end);
  return `M ${from.x} ${from.y} A ${radius} ${radius} 0 ${end - start > 180 ? 1 : 0} 1 ${to.x} ${to.y}`;
};
export function OverviewUsageRing({ sourceRows, modelRows, metric, total, currency }: { sourceRows: OverviewDistributionRow[]; modelRows: OverviewDistributionRow[]; metric: OverviewMetric; total: number; currency: "USD" | "CNY" }) {
  const [mode, setMode] = useState<"source" | "model">("source");
  const [active, setActive] = useState<string | null>(null);
  const rows = mode === "source" ? sourceRows : modelRows;
  const totalLabel = metric === "cost" ? money(total, currency) : formatTokens(total);
  const radius = 63;
  const shareTotal = rows.reduce((sum, row) => sum + Math.max(0, row.share), 0) || 1;
  const gapDegrees = rows.length > 1 ? Math.min(1.6, 8 / rows.length) : 0;
  const rowLabel = (row: OverviewDistributionRow) => `${row.label}，${metric === "cost" ? money(row.cost, currency) : formatTokens(row.tokens)}，占比 ${Math.round(row.share * 100)}%`;
  let angle = -90;

  return <div className={`overview-v4-ring-layout overview-v5-block-ring ${mode}`}>
    <div className="overview-v4-ring-figure">
      <AnimatePresence mode="wait" initial={false}>
        <motion.div className="overview-v5-ring-view" key={mode} initial={{ opacity: 0, scale: .96, filter: "blur(5px)" }} animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }} exit={{ opacity: 0, scale: 1.03, filter: "blur(5px)" }} transition={{ duration: .26, ease: [0.16, 1, 0.3, 1] }}>
      <svg viewBox="0 0 190 190" role="img" aria-label={mode === "source" ? "来源用量分块构成" : "模型用量分块构成"}>
        <circle className="overview-v5-block-ring-track" cx="95" cy="95" r="63"/>
        {rows.map((row, index) => {
          const sweep = Math.max(0, row.share) / shareTotal * 360;
          const start = angle + gapDegrees / 2;
          const end = angle + sweep - gapDegrees / 2;
          angle += sweep;
          if (rows.length === 1) return <g key={`${mode}-${row.key}`}>
            <motion.circle
            key={`${mode}-${row.key}`}
            className={`overview-v5-ring-block${active === row.key ? " active" : ""}`}
            cx="95" cy="95" r={radius} stroke={row.color}
            initial={{ opacity: .2, scale: .96 }} animate={{ opacity: 1, scale: 1 }}
            transition={{ type: "spring", bounce: 0, duration: .48 }}
            onMouseEnter={() => setActive(row.key)} onMouseLeave={() => setActive(null)} onFocus={() => setActive(row.key)} onBlur={() => setActive(null)} onKeyDown={event => { if (event.key === "Escape") setActive(null); }} tabIndex={0} role="button" aria-pressed={active === row.key} aria-label={rowLabel(row)}
            />
          </g>;
          return <g key={`${mode}-${row.key}`}>
            <motion.path
              className={`overview-v5-ring-block${active === row.key ? " active" : ""}`}
              d={ringArc(95, 95, radius, start, Math.max(start + .5, end))}
              stroke={row.color}
              initial={{ pathLength: 0, opacity: .2 }} animate={{ pathLength: 1, opacity: 1 }}
              transition={{ type: "spring", bounce: 0, duration: .48, delay: index * .055 }}
              onMouseEnter={() => setActive(row.key)} onMouseLeave={() => setActive(null)} onFocus={() => setActive(row.key)} onBlur={() => setActive(null)} onKeyDown={event => { if (event.key === "Escape") setActive(null); }} tabIndex={0} role="button" aria-pressed={active === row.key} aria-label={rowLabel(row)}
            />
          </g>;
        })}
      </svg>
      <div className="overview-v4-ring-center"><i className="overview-v5-ring-spark">✦</i><strong>{totalLabel}</strong><span>{mode === "source" ? "来源构成" : "模型构成"}</span></div>
        </motion.div>
      </AnimatePresence>
    </div>
    <div className="overview-v4-distribution-list">
      <div className="overview-v4-ring-mode-switch" role="tablist" aria-label="分布维度"><button type="button" role="tab" aria-selected={mode === "source"} className={mode === "source" ? "active" : ""} onClick={() => { setMode("source"); setActive(null); }}>{mode === "source" && <motion.i className="overview-v5-tab-indicator" layoutId="overview-ring-indicator" transition={{ type: "spring", stiffness: 480, damping: 34, mass: .6 }}/>}<span>来源</span></button><button type="button" role="tab" aria-selected={mode === "model"} className={mode === "model" ? "active" : ""} onClick={() => { setMode("model"); setActive(null); }}>{mode === "model" && <motion.i className="overview-v5-tab-indicator" layoutId="overview-ring-indicator" transition={{ type: "spring", stiffness: 480, damping: 34, mass: .6 }}/>}<span>模型</span></button></div>
      <div className="overview-v4-list-heading"><span>{mode === "source" ? "来源" : "模型"}</span><span>{metric === "cost" ? "花费" : "Token"}</span><span>占比</span></div>
      <AnimatePresence initial={false} mode="popLayout">
      {rows.map((row, index) => <motion.div layout className={`overview-v4-distribution-row${active === row.key ? " active" : ""}`} key={`${mode}-${row.key}`} initial={{ opacity: 0, x: 9, filter: "blur(4px)" }} animate={{ opacity: 1, x: 0, filter: "blur(0px)" }} exit={{ opacity: 0, x: -6, filter: "blur(4px)" }} transition={{ duration: .2, delay: index * .035, ease: [0.16, 1, 0.3, 1] }} onMouseEnter={() => setActive(row.key)} onMouseLeave={() => setActive(null)} onFocus={() => setActive(row.key)} onBlur={() => setActive(null)} onKeyDown={event => { if (event.key === "Escape") setActive(null); }} tabIndex={0} role="button" aria-pressed={active === row.key} aria-label={rowLabel(row)}>
        <span className="overview-v4-source-mark" style={{ "--source-color": row.color } as CSSProperties}>{row.iconBrand ? <BrandIcon brand={row.iconBrand} size={16}/> : <i/>}</span>
        <span className="overview-v5-distribution-copy"><strong title={row.label}>{row.label}</strong><span className="overview-v5-distribution-track" style={{ "--distribution-width": `${Math.max(4, Math.round(row.share * 100))}%`, "--distribution-color": row.color } as CSSProperties}><i/></span></span>
        <AnimatePresence mode="wait" initial={false}><motion.span className="num" key={`${metric}-${currency}-${row.key}-value`} initial={{ opacity: 0, x: 4, filter: "blur(3px)" }} animate={{ opacity: 1, x: 0, filter: "blur(0px)" }} exit={{ opacity: 0, x: -3, filter: "blur(3px)" }} transition={{ duration: .16, ease: [0.16, 1, 0.3, 1] }}>{metric === "cost" ? money(row.cost, currency) : formatTokens(row.tokens)}</motion.span><motion.b className="num" key={`${metric}-${currency}-${row.key}-share`} initial={{ opacity: 0, x: 4, filter: "blur(3px)" }} animate={{ opacity: 1, x: 0, filter: "blur(0px)" }} exit={{ opacity: 0, x: -3, filter: "blur(3px)" }} transition={{ duration: .16, ease: [0.16, 1, 0.3, 1] }}>{Math.round(row.share * 100)}%</motion.b></AnimatePresence>
      </motion.div>)}
      </AnimatePresence>
      {!rows.length && <div className="overview-v4-empty-line">暂无用量数据</div>}
    </div>
  </div>;
}
