import { formatTokens } from "../../lib/analytics";

interface TokenMixRow {
  key: "input" | "cached" | "output" | "reasoning";
  label: string;
  value: number;
}

export function OverviewTokenMix({ input, cached, output, reasoning }: {
  input: number;
  cached: number;
  output: number;
  reasoning: number;
}) {
  const rows: TokenMixRow[] = [
    { key: "input", label: "输入", value: input },
    { key: "cached", label: "缓存命中", value: cached },
    { key: "output", label: "输出", value: output },
    { key: "reasoning", label: "推理输出", value: reasoning },
  ];
  const total = rows.reduce((sum, row) => sum + row.value, 0);

  return <div className="overview-token-mix">
    <div className="overview-token-stack" role="img" aria-label="今日 Token 构成">
      {rows.map(row => <span
        key={row.key}
        className={`overview-token-segment ${row.key}`}
        style={{ width: `${total > 0 ? (row.value / total) * 100 : 0}%` }}
        title={`${row.label} · ${formatTokens(row.value)}`}
      />)}
    </div>
    <div className="overview-token-list">
      {rows.map(row => {
        const share = total > 0 ? (row.value / total) * 100 : 0;
        return <div className="overview-token-row" key={row.key}>
          <span className={`overview-token-dot ${row.key}`}/>
          <span>{row.label}</span>
          <strong className="num">{formatTokens(row.value)}</strong>
          <em className="num">{share.toFixed(1)}%</em>
        </div>;
      })}
    </div>
  </div>;
}
