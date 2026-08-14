import { useState } from "react";
import { UNIT_COLORS } from "../brands";

const CX = 100;
const CY = 100;
const R0 = 54;
const MAXLEN = 34;

// §17.6 日晷盘：今日 24 小时节奏的极坐标签名图形。正午在顶，当前时刻细指针。
export function SundialChart({ hourly, activeLabel, metric, now = new Date() }: {
  hourly: number[];
  activeLabel: string;
  metric: "tokens" | "cost";
  now?: Date;
}) {
  const [hovered, setHovered] = useState<number | null>(null);
  const max = Math.max(...hourly, 0);
  const color = UNIT_COLORS[metric];
  const currentAngle = (now.getHours() + now.getMinutes() / 60 - 12) * 15;
  const fmt = (value: number) => metric === "cost"
    ? `$${value.toFixed(2)}`
    : value >= 1e6 ? `${(value / 1e6).toFixed(2)}M`
    : value >= 1e3 ? `${(value / 1e3).toFixed(1)}K`
    : `${Math.round(value)}`;

  return <svg className="sundial" viewBox="0 0 200 200" role="img" aria-label="今日 24 小时节奏">
    <circle cx={CX} cy={CY} r={R0 - 9} className="sundial-track"/>
    {hourly.map((value, hour) => {
      const ratio = max > 0 ? value / max : 0;
      const len = value > 0 ? 4 + ratio * MAXLEN : 3;
      return <line
        key={hour}
        x1={CX} y1={CY - R0 - len} x2={CX} y2={CY - R0}
        transform={`rotate(${(hour - 12) * 15} ${CX} ${CY})`}
        className={`sundial-bar${value > 0 ? "" : " empty"}${hovered === hour ? " hover" : ""}`}
        tabIndex={0}
        role="img"
        aria-label={`${hour}:00 至 ${hour + 1}:00，${fmt(value)}`}
        stroke={color}
        strokeOpacity={value > 0 ? .3 + ratio * .7 : .14}
        onPointerEnter={() => setHovered(hour)}
        onPointerLeave={() => setHovered(null)}
        onFocus={() => setHovered(hour)}
        onBlur={() => setHovered(null)}
      >
        <title>{`${hour}:00–${hour + 1}:00 · ${fmt(value)}`}</title>
      </line>;
    })}
    <line x1={CX} y1={CY - R0 + 9} x2={CX} y2={CY - R0 - MAXLEN - 9}
      transform={`rotate(${currentAngle} ${CX} ${CY})`} className="sundial-pointer"/>
    <circle cx={CX} cy={CY - R0 - MAXLEN - 9} r={2.6}
      transform={`rotate(${currentAngle} ${CX} ${CY})`} className="sundial-pointer-tip"/>
    <text x={CX} y={14} className="sundial-marker">12</text>
    <text x={CX} y={192} className="sundial-marker">0</text>
    <text x={CX} y={CY - 3} className="sundial-center-value num">{hovered != null ? fmt(hourly[hovered]) : activeLabel}</text>
    <text x={CX} y={CY + 13} className="sundial-center-label">{hovered != null ? `${hovered}:00 时段` : "滚动 24h 活跃"}</text>
  </svg>;
}
