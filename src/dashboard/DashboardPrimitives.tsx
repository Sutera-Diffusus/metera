import type { LucideIcon } from "lucide-react";
import { RollingNumber } from "../components/RollingNumber";
import { displaySource, formatTokens } from "../lib/analytics";
import { motion } from "motion/react";
import { useMetera } from "../state/MeteraContext";

export function MetricBand({ metrics, loading }: { metrics: Array<{ label: string; value: string; detail?: string; icon: LucideIcon; tone?: string }>; loading: boolean }) {
  return <motion.section className="v2-metric-band" aria-busy={loading}>{metrics.map(metric => { const Icon = metric.icon; return <motion.article whileHover={{ y: -6, scale: 1.025 }} whileTap={{ y: -1, scale: .985 }} transition={{ type: "spring", stiffness: 360, damping: 28, mass: .65 }} className={metric.tone ?? ""} key={metric.label}><div><Icon/><span>{metric.label}</span></div><strong><RollingNumber value={metric.value}/></strong>{metric.detail && <small>{metric.detail}</small>}</motion.article>; })}</motion.section>;
}

export function RankedList({ title, subtitle, rows, source = false, selected, onSelect }: {
  title: string; subtitle: string; rows: Array<{ name: string; tokens: number; cost?: number }>;
  source?: boolean; selected?: string; onSelect?(name: string): void;
}) {
  const state = useMetera();
  const peak = Math.max(1, ...rows.map(row => row.tokens));
  return <motion.section layout className="v2-panel v2-ranked-panel" whileHover={{ y: -3 }} transition={{ type: "spring", stiffness: 320, damping: 30 }}><header className="v2-panel-header"><div><h2>{title}</h2><p>{subtitle}</p></div><span>Token</span></header><div className="v2-ranked-list">{rows.slice(0, 8).map((row, index) => <motion.button layout key={row.name} className={selected === row.name ? "active" : ""} onClick={() => onSelect?.(row.name)} disabled={!onSelect} whileHover={{ x: 5 }} whileTap={{ x: 1, scale: .985 }} transition={{ type: "spring", stiffness: 450, damping: 30, mass: .5 }}><b>{String(index + 1).padStart(2, "0")}</b><span title={row.name}>{source ? displaySource(row.name) : row.name}</span><i><motion.em key={`${state.chartReplay}-${row.name}`} initial={state.chartReplayActive ? { scaleX: 0 } : false} animate={{ scaleX: row.tokens / peak }} transition={{ duration: .6, delay: index * .07, ease: [0.16, 1, 0.3, 1] }}/></i><strong>{formatTokens(row.tokens)}</strong></motion.button>)}{!rows.length && <div className="v2-empty">当前范围暂无排行数据</div>}</div></motion.section>;
}

export function PageHeading({ eyebrow, title, description, aside }: { eyebrow: string; title: string; description?: string; aside?: React.ReactNode }) {
  return <motion.header layout className="v2-page-heading"><div><span>{eyebrow}</span><h1>{title}</h1>{description && <p>{description}</p>}</div>{aside}</motion.header>;
}
