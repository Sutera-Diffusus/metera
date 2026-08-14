import { RefreshCw } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../../lib/api";
import type { UsageBucket } from "../../lib/types";
import { useMetera } from "../../state/MeteraContext";
import { dailyInsights, forecastUsage, secondarySummary } from "../dashboardAnalytics";
import { ForecastChart } from "../charts/InsightCharts";
import { InsightCards } from "../charts/InsightCards";

// §17.6 深度分析：六卡六微图 + 预测带（签名图形）。会话散点移交会话记录页。
export function InsightsView() {
  const state = useMetera();
  const [history, setHistory] = useState<UsageBucket[]>([]);
  useEffect(() => {
    let mounted = true;
    const end = new Date(); const start = new Date(end); start.setDate(start.getDate() - 89); start.setHours(0, 0, 0, 0);
    void api.usage(start.toISOString(), end.toISOString()).then(result => { if (mounted) setHistory(result.buckets); }).catch(() => { if (mounted) setHistory([]); });
    return () => { mounted = false; };
  }, [state.chartReplay]);
  const filteredHistory = useMemo(() => history.filter(bucket =>
    (state.filters.hostname === "all" || bucket.hostname === state.filters.hostname)
    && (state.filters.source === "all" || bucket.source === state.filters.source)
    && (state.filters.provider === "all" || bucket.provider === state.filters.provider)
    && (state.filters.model === "all" || bucket.model === state.filters.model)
    && (state.filters.project === "all" || bucket.project === state.filters.project)), [history, state.filters]);
  const summary = secondarySummary(state.buckets, state.sessions);
  const days = dailyInsights(filteredHistory);
  const forecast = forecastUsage(filteredHistory);
  const updatedAt = state.scan.lastScanAt
    ? new Date(state.scan.lastScanAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })
    : null;
  return <div className="v2-view insights-view">
    <header className="apple-view-header">
      <h1>深度分析</h1>
      <div className="apple-view-header-aside">
        {updatedAt && <span>更新于 {updatedAt}</span>}
        <button className="apple-header-refresh" onClick={() => void api.triggerScan()} disabled={state.scan.status === "scanning"}>
          <RefreshCw className={state.scan.status === "scanning" ? "spin" : ""}/>{state.scan.status === "scanning" ? "同步中" : "更新数据"}
        </button>
      </div>
    </header>
    <InsightCards buckets={state.buckets} sessions={state.sessions} days={days} summary={summary}/>
    <ForecastChart points={forecast} replayKey={state.chartReplay} replay={state.chartReplayActive}/>
  </div>;
}
