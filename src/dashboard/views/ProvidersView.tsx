import { Pencil, RefreshCw, Save, X } from "lucide-react";
import { useState } from "react";
import { api } from "../../lib/api";
import { displaySource, formatCost, formatTokens, groupProviders } from "../../lib/analytics";
import { useMetera } from "../../state/MeteraContext";
import { RelationshipMatrix } from "../charts/RelationshipMatrix";

const providerName = (provider: string, aliases: Record<string, string>) => aliases[provider]
  ?? (provider === "unknown" ? "未知供应商" : provider.includes("-provider:") ? provider.split(":").slice(1).join(":") : provider);

export function ProvidersView() {
  const state = useMetera();
  const rows = groupProviders(state.buckets);
  const [mode, setMode] = useState<"tokens" | "cost">("tokens");
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const save = (provider: string) => {
    const aliases = { ...state.settings.providerAliases };
    if (draft.trim()) aliases[provider] = draft.trim(); else delete aliases[provider];
    void state.updateSettings({ providerAliases: aliases });
    setEditing(null);
  };
  const updatedAt = state.scan.lastScanAt
    ? new Date(state.scan.lastScanAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })
    : null;
  return <div className="v2-view">
    <header className="apple-view-header">
      <h1>供应商</h1>
      <div className="apple-view-header-aside">
        <div className="v2-segmented" aria-label="供应商指标"><button className={mode === "tokens" ? "active" : ""} onClick={() => setMode("tokens")}>Token</button><button className={mode === "cost" ? "active" : ""} onClick={() => setMode("cost")}>费用</button></div>
        {updatedAt && <span>更新于 {updatedAt}</span>}
        <button className="apple-header-refresh" onClick={() => void api.triggerScan()} disabled={state.scan.status === "scanning"}>
          <RefreshCw className={state.scan.status === "scanning" ? "spin" : ""}/>{state.scan.status === "scanning" ? "同步中" : "更新数据"}
        </button>
      </div>
    </header>
    <RelationshipMatrix metric={mode} buckets={state.buckets} rowField="provider" columnField="source" title="供应商 × 工具" subtitle="点击矩阵单元格，进入供应商与工具的联合筛选" aliases={state.settings.providerAliases} filters={state.filters} onFilters={state.setFilters}/>
    <section className="v2-panel v2-provider-ledger"><header className="v2-panel-header"><div><h2>供应商明细</h2><p>别名仅影响显示，不改变原始 provider 标识和计费口径。</p></div><span>{rows.length} 个来源</span></header>
      <div>{rows.map((row, index) => <article key={row.provider} className={state.filters.provider === row.provider ? "active" : ""}>
        <button className="provider-main" onClick={() => state.setFilters({ ...state.filters, provider: state.filters.provider === row.provider ? "all" : row.provider })}><b>{String(index + 1).padStart(2, "0")}</b><span><strong>{providerName(row.provider, state.settings.providerAliases)}</strong><small>{row.provider}</small></span></button>
        <div className="provider-contributors">{row.sources.slice(0, 4).map(source => <span key={source.name}>{displaySource(source.name)} <b>{formatTokens(source.tokens)}</b></span>)}</div>
        <div className="provider-numbers"><strong>{mode === "tokens" ? formatTokens(row.tokens) : formatCost(row.priced ? row.cost : Number.NaN)}</strong><small>{mode === "tokens" ? formatCost(row.priced ? row.cost : Number.NaN) : formatTokens(row.tokens)}</small></div>
        {editing === row.provider ? <form className="provider-edit" onSubmit={event => { event.preventDefault(); save(row.provider); }}><input autoFocus aria-label="供应商别名" value={draft} onChange={event => setDraft(event.target.value)} placeholder={providerName(row.provider, {})}/><button aria-label="保存别名"><Save/></button><button type="button" aria-label="取消编辑" onClick={() => setEditing(null)}><X/></button></form> : <button className="provider-pencil" aria-label={`编辑 ${providerName(row.provider, state.settings.providerAliases)} 的别名`} onClick={() => { setEditing(row.provider); setDraft(state.settings.providerAliases[row.provider] ?? ""); }}><Pencil/></button>}
      </article>)}{!rows.length && <div className="v2-empty">当前范围暂无供应商数据</div>}</div>
    </section>
  </div>;
}
