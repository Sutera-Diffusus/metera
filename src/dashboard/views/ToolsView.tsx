import { RefreshCw } from "lucide-react";
import { api } from "../../lib/api";
import { useMetera } from "../../state/MeteraContext";
import { SourceModelSankey } from "../charts/SourceModelSankey";

// §17.6 工具与模型：桑基流图为签名图形（矩阵移交供应商页）。
export function ToolsView() {
  const state = useMetera();
  const updatedAt = state.scan.lastScanAt
    ? new Date(state.scan.lastScanAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })
    : null;
  return <div className="v2-view">
    <header className="apple-view-header">
      <h1>工具与模型</h1>
      <div className="apple-view-header-aside">
        {updatedAt && <span>更新于 {updatedAt}</span>}
        <button className="apple-header-refresh" onClick={() => void api.triggerScan()} disabled={state.scan.status === "scanning"}>
          <RefreshCw className={state.scan.status === "scanning" ? "spin" : ""}/>{state.scan.status === "scanning" ? "同步中" : "更新数据"}
        </button>
      </div>
    </header>
    <SourceModelSankey buckets={state.buckets} filters={state.filters} onFilters={state.setFilters}/>
  </div>;
}
