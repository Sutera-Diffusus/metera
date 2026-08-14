import { BrandIcon } from "./BrandIcon";
import type { QuotaAccount } from "../lib/types";

interface WindowRow {
  key: string;
  provider: string;
  name: string;
  label: string;
  remaining: number;
  resetsAt: number | null;
}

const providerBrand = (provider: string) => {
  const key = provider.toLowerCase();
  if (key.includes("kimi")) return "kimi-code";
  if (key.includes("codex") || key.includes("openai")) return "codex";
  if (key.includes("claude")) return "claude-code";
  if (key.includes("glm") || key.includes("zhipu") || key.includes("z.ai")) return "zai";
  if (key.includes("workbuddy")) return "workbuddy";
  if (key.includes("zcode")) return "zcode";
  if (key.includes("deepseek")) return "dsh";
  if (key.includes("reasonix")) return "reasonix";
  return "unknown";
};

const toneOf = (remaining: number) => remaining < 20 ? "danger" : remaining < 50 ? "warn" : "ok";
const SEGMENTS = 20;

const formatReset = (ms: number) => {
  const date = new Date(ms);
  const weekday = date.toLocaleDateString("zh-CN", { weekday: "short" });
  const time = date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false });
  return `${weekday} ${time}`;
};

const formatRunway = (ms: number) => {
  const delta = Math.max(0, ms - Date.now());
  const days = Math.floor(delta / 86400000);
  const hours = Math.floor((delta % 86400000) / 3600000);
  return days > 0 ? `${days}d ${hours}hr` : `${hours}hr`;
};

// §17.6 额度风险：最紧窗口分段进度条 + 预计可跑 + 风险徽标。
export function QuotaRiskPanel({ quotas, onInspect }: { quotas: QuotaAccount[]; onInspect?(): void }) {
  const windows: WindowRow[] = quotas.flatMap(account => account.windows
    .filter(window => window.remainingPercent != null)
    .map((window, index) => ({
      key: `${account.provider}-${index}`,
      provider: account.provider,
      name: account.name.split("/")[0].trim() || account.name,
      label: window.label,
      remaining: window.remainingPercent ?? 0,
      resetsAt: window.resetsAt ?? null,
    })))
    .sort((a, b) => a.remaining - b.remaining)
    .slice(0, 3);

  const runways = quotas
    .map(account => account.insight?.projectedExhaustionAt)
    .filter((value): value is number => value != null && value > Date.now())
    .sort((a, b) => a - b);
  const tightest = windows[0]?.remaining;
  const tone = tightest == null ? "ok" : toneOf(tightest);

  if (!windows.length) {
    return <section className="v2-panel apple-quota-panel">
      <header className="apple-panel-header"><h2>额度风险</h2>{onInspect && <button className="apple-detail-link" onClick={onInspect}>详情 →</button>}</header>
      <div className="v2-empty">未连接订阅，暂无额度数据</div>
    </section>;
  }

  return <section className="v2-panel apple-quota-panel">
    <header className="apple-panel-header">
      <h2>额度风险</h2>
      <span className={`apple-risk-badge ${tone}`}>{tone === "danger" ? "高风险" : tone === "warn" ? "中风险" : "低风险"}</span>
      {onInspect && <button className="apple-detail-link" onClick={onInspect}>详情 →</button>}
    </header>
    <div className="apple-quota-rows">
      {windows.map(row => {
        const filled = Math.round(((100 - row.remaining) / 100) * SEGMENTS);
        const rowTone = toneOf(row.remaining);
        return <div className="apple-quota-row" key={row.key}>
          <span className="apple-quota-icon"><BrandIcon brand={providerBrand(row.provider)} size={17}/></span>
          <div className="apple-quota-body">
            <div className="apple-quota-title">
              <strong>{row.name}</strong>
              <span>{row.label}</span>
              <b className={`num ${rowTone}`}>{row.remaining.toFixed(0)}%</b>
            </div>
            <div className={`apple-seg-bar ${rowTone}`} aria-label={`剩余 ${row.remaining.toFixed(0)}%`}>
              {Array.from({ length: SEGMENTS }, (_, index) => <i key={index} className={index < filled ? "filled" : ""}/>)}
            </div>
            {row.resetsAt && <small>Resets {formatReset(row.resetsAt)}</small>}
          </div>
        </div>;
      })}
    </div>
    {runways.length > 0 && <footer className="apple-quota-runway">
      <span>预计可跑</span>
      <strong className="num">{formatRunway(runways[0])}</strong>
      <small>按当前使用习惯推算</small>
    </footer>}
  </section>;
}
