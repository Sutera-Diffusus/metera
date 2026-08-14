import { useMemo } from "react";
import { motion } from "motion/react";
import { displaySource, formatCost, formatTokens } from "../../lib/analytics";
import type { Filters, UsageBucket } from "../../lib/types";
import { relationshipMatrix, type MatrixMetric } from "../dashboardAnalytics";
import { useMetera } from "../../state/MeteraContext";

type Field = "source" | "provider" | "model" | "hostname" | "project";

export function RelationshipMatrix({ buckets, rowField, columnField, title, subtitle, aliases = {}, filters, onFilters, metric = "tokens" }: {
  buckets: UsageBucket[];
  rowField: Field;
  columnField: Field;
  title: string;
  subtitle: string;
  aliases?: Record<string, string>;
  filters: Filters;
  onFilters(filters: Filters): void;
  metric?: MatrixMetric;
}) {
  const state = useMetera();
  const matrix = useMemo(() => relationshipMatrix(buckets, rowField, columnField, 7, 8, metric), [buckets, rowField, columnField, metric]);
  const format = (field: Field, value: string) => field === "source" ? displaySource(value) : field === "provider" ? aliases[value] ?? value : value;
  const formatValue = (value: number) => metric === "cost" ? formatCost(value) : formatTokens(value);

  return <section className="v2-panel v2-matrix-panel">
    <header className="v2-panel-header"><div><h2>{title}</h2><p>{subtitle}</p></div><span>{matrix.rows.length} × {matrix.columns.length}</span></header>
    {matrix.rows.length && matrix.columns.length ? <div className="v2-matrix-scroll"><div className="v2-matrix" style={{ "--columns": matrix.columns.length } as React.CSSProperties}>
      <span className="matrix-corner"/>{matrix.columns.map(column => <span className="matrix-column" title={column} key={column}>{format(columnField, column)}</span>)}
      {matrix.rows.map((row, rowIndex) => <div className="matrix-row" key={row}>
        <span className="matrix-label" title={row}>{format(rowField, row)}<small>{formatValue(matrix.rowTotals.get(row) ?? 0)}</small></span>
        {matrix.columns.map((column, columnIndex) => {
          const value = matrix.values.get(`${row}\u0000${column}`) ?? 0;
          const intensity = value / matrix.max;
          const label = `${format(rowField, row)} × ${format(columnField, column)}：${formatValue(value)}`;
          return <button key={column} style={{ "--intensity": intensity } as React.CSSProperties} aria-label={label} title={`${label}；点击筛选`} disabled={!value} onClick={() => onFilters({ ...filters, [rowField]: row, [columnField]: column })}>
            <motion.i key={`${state.chartReplay}-${row}-${column}`} initial={state.chartReplayActive ? { scale: 0, opacity: 0 } : false} animate={{ scale: 1, opacity: intensity * .78 }} transition={{ duration: .42, delay: (rowIndex * matrix.columns.length + columnIndex) * .012, ease: [0.16, 1, 0.3, 1] }}/>
          </button>;
        })}
      </div>)}
    </div></div> : <div className="v2-empty">当前范围暂无关系数据</div>}
  </section>;
}
