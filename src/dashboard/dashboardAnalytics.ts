import type { UsageBucket, UsageSession } from "../lib/types";
import { cacheHitRate, totalsOf } from "../lib/analytics";
import { estimateCost } from "../lib/pricing";

export const bucketTokens = (bucket: UsageBucket) =>
  bucket.inputTokens + bucket.cachedInputTokens + bucket.outputTokens + bucket.reasoningOutputTokens;

export interface HeatCell {
  day: number;
  hour: number;
  prompts: number;
  activeSeconds: number;
}

export function activityHeatmap(sessions: UsageSession[]): HeatCell[] {
  const cells = new Map<string, HeatCell>();
  for (const session of sessions) {
    const day = (new Date(session.firstMessageAt).getDay() + 6) % 7;
    const promptTotal = session.userPromptHours.reduce((sum, value) => sum + value, 0) || 1;
    session.userPromptHours.forEach((prompts, hour) => {
      if (!prompts) return;
      const key = `${day}:${hour}`;
      const current = cells.get(key) ?? { day, hour, prompts: 0, activeSeconds: 0 };
      current.prompts += prompts;
      current.activeSeconds += session.activeSeconds * prompts / promptTotal;
      cells.set(key, current);
    });
  }
  return [...cells.values()];
}

type MatrixField = "source" | "provider" | "model" | "hostname" | "project";
export interface MatrixData {
  rows: string[];
  columns: string[];
  values: Map<string, number>;
  rowTotals: Map<string, number>;
  columnTotals: Map<string, number>;
  max: number;
}

export type MatrixMetric = "tokens" | "cost";

export function relationshipMatrix(
  buckets: UsageBucket[],
  rowField: MatrixField,
  columnField: MatrixField,
  rowLimit = 7,
  columnLimit = 8,
  metric: MatrixMetric = "tokens",
): MatrixData {
  const values = new Map<string, number>();
  const rowTotals = new Map<string, number>();
  const columnTotals = new Map<string, number>();
  for (const bucket of buckets) {
    const row = bucket[rowField] || "unknown";
    const column = bucket[columnField] || "unknown";
    const value = metric === "cost" ? estimateCost(bucket) : bucketTokens(bucket);
    if (value == null) continue;
    values.set(`${row}\u0000${column}`, (values.get(`${row}\u0000${column}`) ?? 0) + value);
    rowTotals.set(row, (rowTotals.get(row) ?? 0) + value);
    columnTotals.set(column, (columnTotals.get(column) ?? 0) + value);
  }
  const ranked = (map: Map<string, number>, limit: number) =>
    [...map.entries()].sort((a, b) => b[1] - a[1]).slice(0, limit).map(([name]) => name);
  const rows = ranked(rowTotals, rowLimit);
  const columns = ranked(columnTotals, columnLimit);
  const max = Math.max(1, ...rows.flatMap(row => columns.map(column => values.get(`${row}\u0000${column}`) ?? 0)));
  return { rows, columns, values, rowTotals, columnTotals, max };
}

export function activeByPeriod(sessions: UsageSession[], range: string) {
  const hourly = range === "today" || range === "24h";
  const map = new Map<string, number>();
  for (const session of sessions) {
    const date = new Date(session.firstMessageAt);
    if (hourly) {
      const prompts = session.userPromptHours.reduce((a, b) => a + b, 0) || 1;
      session.userPromptHours.forEach((count, hour) => {
        if (!count) return;
        const key = `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}-${hour}`;
        map.set(key, (map.get(key) ?? 0) + session.activeSeconds * count / prompts);
      });
    } else {
      const key = `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
      map.set(key, (map.get(key) ?? 0) + session.activeSeconds);
    }
  }
  return map;
}

export interface DailyInsight {
  date: string;
  timestamp: number;
  tokens: number;
  cost: number;
  cacheRate: number;
  costPerMillion: number;
  input: number;
  output: number;
}

export function dailyInsights(buckets: UsageBucket[]): DailyInsight[] {
  const days = new Map<string, UsageBucket[]>();
  for (const bucket of buckets) {
    const date = new Date(bucket.bucketStart);
    const key = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
    days.set(key, [...(days.get(key) ?? []), bucket]);
  }
  return [...days.entries()].map(([date, rows]) => {
    const totals = totalsOf(rows);
    return {
      date,
      timestamp: new Date(`${date}T00:00:00`).getTime(),
      tokens: totals.tokens,
      cost: totals.cost,
      cacheRate: cacheHitRate(totals.input, totals.cached),
      costPerMillion: totals.tokens ? totals.cost / totals.tokens * 1_000_000 : 0,
      input: totals.input + totals.cached,
      output: totals.output + totals.reasoning,
    };
  }).sort((left, right) => left.timestamp - right.timestamp);
}

export interface SecondarySummary {
  cacheRate: number;
  costPerMillion: number;
  inputOutputRatio: number;
  topToolShare: number;
  topProviderShare: number;
  tokensPerSession: number;
  peakHourShare: number;
  pricingCoverage: number;
}

export function secondarySummary(buckets: UsageBucket[], sessions: UsageSession[]): SecondarySummary {
  const totals = totalsOf(buckets);
  const groupShare = (field: "source" | "provider") => {
    const groups = new Map<string, number>();
    for (const bucket of buckets) groups.set(bucket[field] || "unknown", (groups.get(bucket[field] || "unknown") ?? 0) + bucketTokens(bucket));
    return totals.tokens ? Math.max(0, ...groups.values()) / totals.tokens : 0;
  };
  const prompts = Array(24).fill(0) as number[];
  for (const session of sessions) session.userPromptHours.forEach((value, hour) => { prompts[hour] += value; });
  const promptTotal = prompts.reduce((sum, value) => sum + value, 0);
  const peakTotal = prompts.slice().sort((a, b) => b - a).slice(0, 3).reduce((sum, value) => sum + value, 0);
  return {
    cacheRate: cacheHitRate(totals.input, totals.cached),
    costPerMillion: totals.tokens ? totals.cost / totals.tokens * 1_000_000 : 0,
    inputOutputRatio: totals.output + totals.reasoning ? (totals.input + totals.cached) / (totals.output + totals.reasoning) : 0,
    topToolShare: groupShare("source"),
    topProviderShare: groupShare("provider"),
    tokensPerSession: sessions.length ? totals.tokens / sessions.length : 0,
    peakHourShare: promptTotal ? peakTotal / promptTotal : 0,
    pricingCoverage: buckets.length ? totals.priced / buckets.length : 0,
  };
}

export interface ForecastPoint {
  date: string;
  tokens: number;
  tokenLow: number;
  tokenHigh: number;
  cost: number | null;
  costLow: number | null;
  costHigh: number | null;
}

function forecastSeries(values: number[], weekdays: number[], horizon: number) {
  const count = values.length;
  const weights = values.map((_, index) => .45 + .55 * (index + 1) / count);
  const weightTotal = weights.reduce((sum, value) => sum + value, 0);
  const meanX = weights.reduce((sum, weight, index) => sum + weight * index, 0) / weightTotal;
  const meanY = weights.reduce((sum, weight, index) => sum + weight * values[index], 0) / weightTotal;
  const denominator = weights.reduce((sum, weight, index) => sum + weight * (index - meanX) ** 2, 0);
  const slope = denominator ? weights.reduce((sum, weight, index) => sum + weight * (index - meanX) * (values[index] - meanY), 0) / denominator : 0;
  const intercept = meanY - slope * meanX;
  const globalAverage = values.reduce((sum, value) => sum + value, 0) / count || 1;
  const weekdayFactors = Array.from({ length: 7 }, (_, weekday) => {
    const matches = values.filter((_, index) => weekdays[index] === weekday);
    return matches.length ? matches.reduce((sum, value) => sum + value, 0) / matches.length / globalAverage : 1;
  });
  const fitted = values.map((_, index) => Math.max(0, (intercept + slope * index) * weekdayFactors[weekdays[index]]));
  const deviation = Math.sqrt(values.reduce((sum, value, index) => sum + (value - fitted[index]) ** 2, 0) / Math.max(1, count - 2));
  return Array.from({ length: horizon }, (_, step) => {
    const index = count + step;
    const weekday = (weekdays.at(-1)! + step + 1) % 7;
    const value = Math.max(0, (intercept + slope * index) * weekdayFactors[weekday]);
    const uncertainty = deviation * (1.15 + step * .09);
    return { value, low: Math.max(0, value - uncertainty), high: value + uncertainty };
  });
}

export function forecastUsage(buckets: UsageBucket[], horizon = 7): ForecastPoint[] {
  const days = dailyInsights(buckets).slice(-90);
  if (days.length < 14) return [];
  const weekdays = days.map(day => new Date(`${day.date}T00:00:00`).getDay());
  const tokenForecast = forecastSeries(days.map(day => day.tokens), weekdays, horizon);
  const pricingCoverage = buckets.length ? totalsOf(buckets).priced / buckets.length : 0;
  const costForecast = pricingCoverage >= .8 ? forecastSeries(days.map(day => day.cost), weekdays, horizon) : null;
  const date = new Date(`${days.at(-1)!.date}T00:00:00`);
  return tokenForecast.map((tokens, index) => {
    const next = new Date(date); next.setDate(next.getDate() + index + 1);
    const cost = costForecast?.[index] ?? null;
    return {
      date: `${next.getMonth() + 1}/${next.getDate()}`,
      tokens: tokens.value,
      tokenLow: tokens.low,
      tokenHigh: tokens.high,
      cost: cost?.value ?? null,
      costLow: cost?.low ?? null,
      costHigh: cost?.high ?? null,
    };
  });
}
