import { motion } from "motion/react";
import { monotoneCurve } from "./overviewChartGeometry";

export function OverviewSparkline({ values, tone, label }: { values: number[]; tone: "active" | "cache"; label: string }) {
  const width = 180;
  const height = 44;
  const padding = 3;
  const safe = values.length > 1 ? values : [values[0] ?? 0, values[0] ?? 0];
  const minimum = Math.min(...safe);
  const maximum = Math.max(...safe);
  const spread = maximum - minimum || Math.max(1, Math.abs(maximum) * .12);
  const points = safe.map((value, index) => ({
    x: padding + (index / (safe.length - 1)) * (width - padding * 2),
    y: height - padding - ((value - minimum) / spread) * (height - padding * 2),
  }));
  const path = monotoneCurve(points);
  const end = points.at(-1)!;

  return <svg className={`overview-v4-sparkline ${tone}`} viewBox={`0 0 ${width} ${height}`} role="img" aria-label={label} preserveAspectRatio="none">
    <motion.path className="overview-v4-sparkline-path" d={path} initial={false} animate={{ d: path }} transition={{ duration: .46, ease: [0.22, 1, .36, 1] }}/>
    <motion.circle className="overview-v4-sparkline-dot" cx={end.x} cy={end.y} r="2.8" initial={false} animate={{ cx: end.x, cy: end.y }} transition={{ duration: .46, ease: [0.22, 1, .36, 1] }}/>
  </svg>;
}
