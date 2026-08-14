import { useMemo } from "react";
import { activityHeatmap } from "../dashboardAnalytics";
import type { UsageSession } from "../../lib/types";
import { formatDuration } from "../../lib/analytics";
import { motion } from "motion/react";
import { usePhysicalTilt } from "../../hooks/usePhysicalTilt";
import { useMetera } from "../../state/MeteraContext";

const weekdays = ["一", "二", "三", "四", "五", "六", "日"];

export function ActivityHeatmap({ sessions, compact = false }: { sessions: UsageSession[]; compact?: boolean }) {
  const state = useMetera();
  const cells = useMemo(() => activityHeatmap(sessions), [sessions]);
  const map = useMemo(() => new Map(cells.map(cell => [`${cell.day}:${cell.hour}`, cell])), [cells]);
  const peak = Math.max(1, ...cells.map(cell => cell.prompts));
  const physical = usePhysicalTilt(.8);
  return <motion.section layout className={`v2-panel v2-heat-panel${compact ? " compact" : ""}`} style={physical.style} onPointerMove={physical.onPointerMove} onPointerLeave={physical.onPointerLeave} whileHover={{ y: -5, scale: 1.004 }} whileTap={{ scale: .994 }} transition={{ type: "spring", stiffness: 320, damping: 30, mass: .8 }}>
    <header className="v2-panel-header"><div><h2>活跃热力</h2><p>星期与小时的用户提示分布</p></div><span className="v2-heat-total">{cells.reduce((sum, cell) => sum + cell.prompts, 0).toLocaleString()} 次提示</span></header>
    <div className="v2-heatmap" role="grid" aria-label="星期与小时活跃热力图">
      <span className="corner"/>{Array.from({ length: 24 }, (_, hour) => <span className="hour" key={hour}>{hour % 3 === 0 ? String(hour).padStart(2, "0") : ""}</span>)}
      {weekdays.map((day, dayIndex) => <div className="heat-row" role="row" key={day}><span className="day">周{day}</span>{Array.from({ length: 24 }, (_, hour) => { const cell = map.get(`${dayIndex}:${hour}`); const level = cell ? Math.max(.12, cell.prompts / peak) : 0; const label = `周${day} ${String(hour).padStart(2, "0")}:00，${cell?.prompts ?? 0} 次提示，${formatDuration(cell?.activeSeconds ?? 0)} 活跃`; return <motion.button role="gridcell" aria-label={label} title={label} key={`${state.chartReplay}-${hour}`} style={{ "--heat": level, transformOrigin: "center bottom" } as React.CSSProperties} initial={state.chartReplayActive ? { scaleY: 0, opacity: .25 } : false} animate={{ scaleY: 1, opacity: 1 }} whileHover={{ scale: 1.28, zIndex: 3 }} whileTap={{ scale: .76 }} transition={{ type: "spring", stiffness: 620, damping: 25, mass: .35, delay: (dayIndex * 24 + hour) * .0025 }}/>; })}</div>)}
    </div>
  </motion.section>;
}
