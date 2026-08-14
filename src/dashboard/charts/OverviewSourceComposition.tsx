import type { CSSProperties } from "react";
import { BrandIcon } from "../BrandIcon";
import { sourceFill } from "../brands";
import { displaySource, formatCost, formatTokens } from "../../lib/analytics";

export interface OverviewSourceRow {
  source: string;
  tokens: number;
  cost: number;
  priced: number;
  models: number;
}

export function OverviewSourceComposition({ rows, onInspect }: {
  rows: OverviewSourceRow[];
  onInspect?(source: string): void;
}) {
  const visibleRows = rows.slice(0, 5);
  const total = rows.reduce((sum, row) => sum + row.tokens, 0);
  const totalCost = rows.reduce((sum, row) => sum + row.cost, 0);

  return <div className="overview-source-composition">
    <div className="overview-source-stack" role="img" aria-label="今日各工具用量构成">
      {visibleRows.map((row, index) => <span
        key={row.source}
        className="overview-source-segment"
        style={{ width: `${total > 0 ? (row.tokens / total) * 100 : 0}%`, background: sourceFill(row.source), "--source-index": index } as CSSProperties}
        title={`${displaySource(row.source)} · ${formatTokens(row.tokens)}`}
      />)}
      {!visibleRows.length && <span className="overview-source-stack-empty"/>}
    </div>
    <div className="overview-source-table" role="list">
      {visibleRows.map((row, index) => {
        const share = total > 0 ? (row.tokens / total) * 100 : 0;
        const content = <>
          <span className="overview-source-index">{String(index + 1).padStart(2, "0")}</span>
          <span className="overview-source-icon" style={{ "--source-color": sourceFill(row.source) } as CSSProperties}>
            <i className="overview-source-swatch" aria-hidden="true"/>
            <BrandIcon brand={row.source} size={17}/>
          </span>
          <span className="overview-source-name">{displaySource(row.source)}</span>
          <span className="overview-source-models">{row.models} 个模型</span>
          <strong className="overview-source-tokens num">{formatTokens(row.tokens)}</strong>
          <span className="overview-source-cost num">{row.priced > 0 ? formatCost(row.cost) : "--"}</span>
          <span className="overview-source-share num">{share.toFixed(1)}%</span>
        </>;
        return onInspect
          ? <button type="button" className="overview-source-row" key={row.source} onClick={() => onInspect(row.source)}>{content}</button>
          : <div className="overview-source-row" key={row.source} role="listitem">{content}</div>;
      })}
      {!visibleRows.length && <div className="overview-inline-empty">今天暂无来源用量</div>}
    </div>
    {rows.length > visibleRows.length && <div className="overview-source-more">还有 {rows.length - visibleRows.length} 个来源未展开</div>}
    {rows.length > 0 && <div className="overview-source-total"><span>合计</span><strong className="num">{formatTokens(total)}</strong><span className="num">{totalCost > 0 ? formatCost(totalCost) : "费用待定"}</span></div>}
  </div>;
}
