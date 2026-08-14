import { Activity, ArrowUpRight, Clock3, MessageSquareText } from "lucide-react";
import type { CSSProperties } from "react";
import { BrandIcon } from "../BrandIcon";
import { sourceFill } from "../brands";
import { displaySource, formatDuration, formatTokens } from "../../lib/analytics";
import type { UsageSession } from "../../lib/types";

export interface OverviewModelRow {
  name: string;
  source: string;
  tokens: number;
  cost: number;
  priced: number;
  share: number;
}

export function OverviewModelPanel({ rows, onInspect }: {
  rows: OverviewModelRow[];
  onInspect?(): void;
}) {
  return <section className="overview-panel overview-model-panel">
    <header className="overview-panel-header">
      <div><p className="overview-kicker">模型排行</p><h2>今天使用最多的模型</h2></div>
      {onInspect && <button type="button" className="overview-panel-link" onClick={onInspect}>查看工具与模型 <ArrowUpRight/></button>}
    </header>
    <div className="overview-model-list" role="list">
      {rows.slice(0, 6).map((row, index) => <div className="overview-model-row" key={`${row.name}-${row.source}`} role="listitem" style={{ "--model-color": sourceFill(row.source) } as CSSProperties}>
        <span className="overview-model-rank">{String(index + 1).padStart(2, "0")}</span>
        <span className="overview-model-icon"><BrandIcon brand={row.source} size={16}/></span>
        <span className="overview-model-name"><strong>{row.name === "unknown" ? "未知模型" : row.name}</strong><small>{displaySource(row.source)}</small></span>
        <span className="overview-model-bar"><i style={{ width: `${Math.max(4, row.share)}%` }}/></span>
        <strong className="overview-model-tokens num">{formatTokens(row.tokens)}</strong>
        <span className="overview-model-share num">{row.share.toFixed(1)}%</span>
      </div>)}
      {!rows.length && <div className="overview-inline-empty">今天暂无模型用量</div>}
    </div>
  </section>;
}

const sessionTime = (value: string) => {
  const date = new Date(value);
  return Number.isFinite(date.getTime())
    ? date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })
    : "--:--";
};

export function OverviewSessionPanel({ sessions, onInspect }: {
  sessions: UsageSession[];
  onInspect?(): void;
}) {
  return <section className="overview-panel overview-session-panel">
    <header className="overview-panel-header">
      <div><p className="overview-kicker">最近活动</p><h2>最近会话</h2></div>
      {onInspect && <button type="button" className="overview-panel-link" onClick={onInspect}>打开会话记录 <ArrowUpRight/></button>}
    </header>
    <div className="overview-session-list" role="list">
      {sessions.slice(0, 6).map(session => {
        const active = session.activeSeconds > 0;
        return <button type="button" className="overview-session-row" key={session.sessionHash} onClick={onInspect} style={{ "--session-color": sourceFill(session.source) } as CSSProperties}>
          <span className={`overview-session-state${active ? " active" : ""}`}><i aria-hidden="true"/><BrandIcon brand={session.source} size={16}/></span>
          <span className="overview-session-main"><strong>{session.project || "未命名项目"}</strong><small>{displaySource(session.source)} · {sessionTime(session.lastMessageAt)}</small></span>
          <span className="overview-session-stat"><MessageSquareText/><b className="num">{session.messageCount}</b><small>消息</small></span>
          <span className="overview-session-stat"><Clock3/><b className="num">{formatDuration(session.activeSeconds)}</b><small>活跃</small></span>
          <span className="overview-session-arrow"><ArrowUpRight/></span>
        </button>;
      })}
      {!sessions.length && <div className="overview-inline-empty"><Activity/>今天暂无会话活动</div>}
    </div>
  </section>;
}
