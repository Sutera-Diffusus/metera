import { useMemo, useState } from "react";
import { motion } from "motion/react";
import { displaySource, formatDuration } from "../../lib/analytics";
import type { UsageSession } from "../../lib/types";
import { useMetera } from "../../state/MeteraContext";
import { sourceFill } from "../brands";

const OTHER = "__other";
const MAX_LANES = 6;

interface Dot { session: UsageSession; lane: string; x: number; y: number; r: number }

// §17.6 会话记录签名图形：蜂群图——每工具一条泳道，横轴最近活动时间，
// 圆点按消息数定大小、按品牌色填充，垂直避让不重叠。
export function SessionBeeswarm({ sessions, selected, onSelect }: { sessions: UsageSession[]; selected: string | null; onSelect(id: string | null): void }) {
  const state = useMetera();
  const [hovered, setHovered] = useState<string | null>(null);

  const { lanes, minTime, maxTime } = useMemo(() => {
    const laneTotals = new Map<string, number>();
    for (const session of sessions) {
      const source = session.source || "unknown";
      laneTotals.set(source, (laneTotals.get(source) ?? 0) + session.activeSeconds);
    }
    const ranked = [...laneTotals.entries()].sort((a, b) => b[1] - a[1]);
    const kept = ranked.slice(0, MAX_LANES).map(([name]) => name);
    const laneKeys = ranked.length > MAX_LANES ? [...kept, OTHER] : kept;
    const times = sessions.map(session => new Date(session.lastMessageAt).getTime());
    const min = Math.min(...times, Date.now());
    const max = Math.max(...times, Date.now());
    return { lanes: laneKeys, minTime: min, maxTime: max };
  }, [sessions]);

  const width = 920, laneH = 66, left = 96, right = 26, top = 10, bottom = 26;
  const height = top + lanes.length * laneH + bottom;
  const xOf = (time: number) => left + (time - minTime) / Math.max(1, maxTime - minTime) * (width - left - right);

  // 蜂群避让：同泳道内按时间排序后逐个找不碰撞的垂直偏移
  const swarm = useMemo(() => {
    const dots: Dot[] = [];
    const laneIndex = new Map(lanes.map((key, index) => [key, index]));
    const byLane = new Map<string, UsageSession[]>();
    for (const session of sessions) {
      const source = session.source || "unknown";
      const lane = laneIndex.has(source) ? source : OTHER;
      byLane.set(lane, [...(byLane.get(lane) ?? []), session]);
    }
    for (const lane of lanes) {
      const row = (byLane.get(lane) ?? []).sort((a, b) => new Date(a.lastMessageAt).getTime() - new Date(b.lastMessageAt).getTime());
      const centerY = top + (laneIndex.get(lane) ?? 0) * laneH + laneH / 2;
      const settled: Dot[] = [];
      for (const session of row.slice(0, 120)) {
        const r = Math.min(9, Math.max(3, 2.4 + Math.sqrt(session.messageCount) * .85));
        const x = xOf(new Date(session.lastMessageAt).getTime());
        let dy = 0;
        for (let step = 0; step <= 12; step++) {
          const candidate = step === 0 ? 0 : (step % 2 ? 1 : -1) * Math.ceil(step / 2) * (r + 3.2);
          const fits = Math.abs(candidate) + r <= laneH / 2 - 4
            && settled.every(other => Math.abs(other.x - x) >= r + other.r + 1 || (other.x - x) ** 2 + (centerY + candidate - other.y) ** 2 >= (r + other.r + 1) ** 2);
          if (fits) { dy = candidate; break; }
          dy = candidate;
        }
        const dot: Dot = { session, lane, x, y: centerY + dy, r };
        settled.push(dot);
        dots.push(dot);
      }
    }
    return dots;
  }, [sessions, lanes, minTime, maxTime]);

  const active = swarm.find(dot => dot.session.sessionHash === (selected ?? hovered));
  const timeLabel = (time: number) => new Date(time).toLocaleString([], { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });

  if (!sessions.length) return <section className="v2-panel apple-swarm-panel"><header className="apple-panel-header"><h2>会话蜂群</h2></header><div className="v2-empty">当前范围暂无会话数据</div></section>;

  return <section className="v2-panel apple-swarm-panel">
    <header className="apple-panel-header">
      <h2>会话蜂群</h2>
      {active && <div className="apple-swarm-readout"><strong className="num">{formatDuration(active.session.activeSeconds)}</strong><span>{displaySource(active.session.source)} · {active.session.project || "未命名项目"} · {active.session.messageCount} 条消息</span></div>}
    </header>
    <svg className="apple-swarm-svg" viewBox={`0 0 ${width} ${height}`} role="img" aria-label={`会话蜂群图，共 ${swarm.length} 个会话`} onMouseLeave={() => setHovered(null)}>
      {lanes.map((lane, index) => <g key={lane} className="swarm-lane">
        {index > 0 && <line className="lane-line" x1={8} x2={width - 8} y1={top + index * laneH} y2={top + index * laneH}/>}
        <text x={left - 12} y={top + index * laneH + laneH / 2} textAnchor="end" className="lane-label">{lane === OTHER ? "其他" : displaySource(lane)}</text>
      </g>)}
      {swarm.map((dot, index) => {
        const isActive = dot.session.sessionHash === (selected ?? hovered);
        const dimmed = (selected ?? hovered) != null && !isActive;
        return <motion.circle key={`${state.chartReplay}-${dot.session.sessionHash}`}
          cx={dot.x} cy={dot.y} r={dot.r}
          className={`swarm-dot${isActive ? " active" : ""}${dot.session.sessionHash === selected ? " selected" : ""}`}
          initial={state.chartReplayActive ? { scale: 0, opacity: 0 } : false}
          animate={{ scale: 1, opacity: dimmed ? .25 : 1 }}
          style={{ transformOrigin: `${dot.x}px ${dot.y}px`, fill: sourceFill(dot.lane === OTHER ? "unknown" : dot.lane) }}
          transition={{ duration: .32, delay: state.chartReplayActive ? Math.min(index, 40) * .02 : 0, ease: [0.16, 1, 0.3, 1] }}
          tabIndex={0} role="button"
          aria-label={`${displaySource(dot.session.source)}，${formatDuration(dot.session.activeSeconds)} 活跃，${dot.session.messageCount} 条消息，最近 ${timeLabel(new Date(dot.session.lastMessageAt).getTime())}`}
          onMouseEnter={() => setHovered(dot.session.sessionHash)} onFocus={() => setHovered(dot.session.sessionHash)} onBlur={() => setHovered(null)}
          onClick={() => onSelect(selected === dot.session.sessionHash ? null : dot.session.sessionHash)}
          onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); onSelect(selected === dot.session.sessionHash ? null : dot.session.sessionHash); } }}/>;
      })}
      <text x={left} y={height - 8} className="axis-label">{timeLabel(minTime)}</text>
      <text x={width - right} y={height - 8} textAnchor="end" className="axis-label">{timeLabel(maxTime)}</text>
    </svg>
  </section>;
}
