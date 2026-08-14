import { RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { totalsOf } from "../../lib/analytics";
import { api } from "../../lib/api";
import { useMetera } from "../../state/MeteraContext";
import { activeByPeriod } from "../dashboardAnalytics";
import { ActivityHeatmap } from "../charts/ActivityHeatmap";
import { SourceStackedBars, type UsageMode } from "../charts/SourceStackedBars";
import { TokenWaffle } from "../charts/TokenWaffle";

// §17.6 用量分析：堆叠柱（签名图形）+ 像素池 + 热力；矩阵移交供应商页。
export function UsageView() {
  const state = useMetera();
  const [mode, setMode] = useState<UsageMode>("tokens");
  const totals = totalsOf(state.buckets);
  const activeMap = useMemo(() => activeByPeriod(state.sessions, state.range), [state.sessions, state.range]);
  const updatedAt = state.scan.lastScanAt
    ? new Date(state.scan.lastScanAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })
    : null;
  return <div className="v2-view">
    <header className="apple-view-header">
      <h1>用量分析</h1>
      <div className="apple-view-header-aside">
        {updatedAt && <span>更新于 {updatedAt}</span>}
        <button className="apple-header-refresh" onClick={() => void api.triggerScan()} disabled={state.scan.status === "scanning"}>
          <RefreshCw className={state.scan.status === "scanning" ? "spin" : ""}/>{state.scan.status === "scanning" ? "同步中" : "更新数据"}
        </button>
      </div>
    </header>
    <SourceStackedBars buckets={state.buckets} activeMap={activeMap} range={state.range} mode={mode} onMode={setMode}/>
    <div className="v2-analysis-grid">
      <ActivityHeatmap sessions={state.sessions}/>
      <TokenWaffle input={totals.input} cache={totals.cached} output={totals.output} reasoning={totals.reasoning}/>
    </div>
  </div>;
}
