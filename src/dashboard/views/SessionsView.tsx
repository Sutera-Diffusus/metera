import { RefreshCw, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { api } from "../../lib/api";
import { displaySource, formatDuration } from "../../lib/analytics";
import { useMetera } from "../../state/MeteraContext";
import { SessionBeeswarm } from "../charts/SessionBeeswarm";

// §17.6 会话记录：蜂群图为签名图形，下方保留明细表（与蜂群互相定位）。
export function SessionsView() {
  const state = useMetera();
  const [selected, setSelected] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const rows = useMemo(() => state.sessions.filter(session => `${session.source} ${session.project} ${session.hostname}`.toLowerCase().includes(query.toLowerCase())).sort((a, b) => new Date(b.lastMessageAt).getTime() - new Date(a.lastMessageAt).getTime()), [state.sessions, query]);
  const updatedAt = state.scan.lastScanAt
    ? new Date(state.scan.lastScanAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })
    : null;
  return <div className="v2-view">
    <header className="apple-view-header">
      <h1>会话记录</h1>
      <div className="apple-view-header-aside">
        <label className="v2-search"><Search/><input value={query} onChange={event => setQuery(event.target.value)} placeholder="搜索工具、项目或终端"/></label>
        {updatedAt && <span>更新于 {updatedAt}</span>}
        <button className="apple-header-refresh" onClick={() => void api.triggerScan()} disabled={state.scan.status === "scanning"}>
          <RefreshCw className={state.scan.status === "scanning" ? "spin" : ""}/>{state.scan.status === "scanning" ? "同步中" : "更新数据"}
        </button>
      </div>
    </header>
    <SessionBeeswarm sessions={rows} selected={selected} onSelect={setSelected}/>
    <section className="v2-panel v2-session-ledger"><header className="apple-panel-header"><h2>会话明细</h2><span className="apple-ledger-count">{rows.length} 个会话</span></header><div className="session-table" role="table" aria-label="会话明细">
      <div className="session-head" role="row"><span>工具 / 项目</span><span>活跃</span><span>跨度</span><span>消息</span><span>最近活动</span></div>
      {rows.slice(0, 120).map(session => <button key={session.sessionHash} className={selected === session.sessionHash ? "active" : ""} onClick={() => setSelected(selected === session.sessionHash ? null : session.sessionHash)} role="row"><span><strong>{displaySource(session.source)}</strong><small>{session.project || "未命名项目"} · {session.hostname}</small></span><span>{formatDuration(session.activeSeconds)}</span><span>{formatDuration(session.durationSeconds)}</span><span>{session.messageCount}<small>{session.userMessageCount} 条用户消息</small></span><time dateTime={session.lastMessageAt}>{new Date(session.lastMessageAt).toLocaleString([], { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" })}</time></button>)}
      {!rows.length && <div className="v2-empty">没有符合条件的会话</div>}
    </div></section>
  </div>;
}
