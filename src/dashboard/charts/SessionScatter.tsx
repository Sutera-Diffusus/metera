import { useMemo, useState } from "react";
import { motion } from "motion/react";
import { formatDuration } from "../../lib/analytics";
import type { UsageSession } from "../../lib/types";
import { useMetera } from "../../state/MeteraContext";

export function SessionScatter({ sessions, selected, onSelect }: { sessions: UsageSession[]; selected: string | null; onSelect(id: string | null): void }) {
  const state = useMetera();
  const [hovered, setHovered] = useState<string | null>(null);
  const points = useMemo(() => sessions.slice().sort((a, b) => b.activeSeconds - a.activeSeconds).slice(0, 80), [sessions]);
  const maxDuration = Math.max(1, ...points.map(point => point.durationSeconds));
  const maxActive = Math.max(1, ...points.map(point => point.activeSeconds));
  const width = 760, height = 280, left = 44, right = 20, top = 20, bottom = 36;
  const active = points.find(point => point.sessionHash === (selected ?? hovered));
  return <section className="v2-panel v2-scatter-panel">
    <header className="v2-panel-header"><div><h2>会话分布</h2><p>横轴总时长，纵轴活跃时长，圆点大小表示消息数</p></div>{active && <div className="scatter-readout"><strong>{formatDuration(active.activeSeconds)}</strong><span>{active.source} · {active.messageCount} 条消息</span></div>}</header>
    {points.length ? <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={`会话时长分布，共 ${points.length} 个会话`} className="v2-scatter-svg" onMouseLeave={() => setHovered(null)}>
      {[0, .25, .5, .75, 1].map(value => <line key={value} x1={left} x2={width - right} y1={top + value * (height - top - bottom)} y2={top + value * (height - top - bottom)} className="grid-line"/>)}
      {points.map((point, index) => { const x = left + point.durationSeconds / maxDuration * (width - left - right); const y = top + (1 - point.activeSeconds / maxActive) * (height - top - bottom); const isActive = point.sessionHash === (selected ?? hovered); return <motion.circle key={`${state.chartReplay}-${point.sessionHash}`} initial={state.chartReplayActive ? { scale: 0, opacity: 0 } : false} animate={{ scale: 1, opacity: 1 }} transition={{ duration: .36, delay: Math.min(index, 24) * .025, ease: [0.16, 1, 0.3, 1] }} cx={x} cy={y} r={Math.min(10, 3 + Math.sqrt(point.messageCount))} className={isActive ? "active" : ""} tabIndex={0} role="button" aria-label={`${point.source}，${formatDuration(point.activeSeconds)} 活跃，${point.messageCount} 条消息`} onMouseEnter={() => setHovered(point.sessionHash)} onFocus={() => setHovered(point.sessionHash)} onBlur={() => setHovered(null)} onClick={() => onSelect(selected === point.sessionHash ? null : point.sessionHash)}/>; })}
      <text x={width - right} y={height - 8} textAnchor="end">总时长 {formatDuration(maxDuration)}</text>
    </svg> : <div className="v2-empty">当前范围暂无会话数据</div>}
  </section>;
}
