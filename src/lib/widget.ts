import type { UsageBucket, UsageSession } from "./types";
import { estimateCost } from "./pricing";

export type WidgetValueMetric = "tokens" | "cost";

const DAY_MS = 24 * 60 * 60_000;

export interface WidgetSummary {
  previousChange: number | null;
  sessionCount: number;
  peakHour: number | null;
  leadingSource: { source: string; share: number } | null;
}

const bucketTokens = (bucket: UsageBucket) => bucket.inputTokens
  + bucket.cachedInputTokens
  + bucket.outputTokens
  + bucket.reasoningOutputTokens;

const bucketValue = (bucket: UsageBucket, metric: WidgetValueMetric) => metric === "cost"
  ? estimateCost(bucket) ?? 0
  : bucketTokens(bucket);

const localDayKey = (date: Date) => `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;

export function formatWidgetTokens(value: number): string {
  if (value >= 1e9) return `${(value / 1e9).toFixed(2)}B`;
  if (value >= 1e6) return `${(value / 1e6).toFixed(2)}M`;
  if (value >= 1e3) return `${(value / 1e3).toFixed(2)}K`;
  return Math.max(0, value).toFixed(2);
}

export function activeSeconds24hOf(sessions: UsageSession[], now = new Date()): number {
  const end = now.getTime();
  const start = end - DAY_MS;
  const slotMs = 60_000;
  const slots = Array.from({ length: 24 * 60 }, () => 0);
  for (const session of sessions) {
    const first = new Date(session.firstMessageAt).getTime();
    const last = new Date(session.lastMessageAt).getTime();
    if (!Number.isFinite(first) || !Number.isFinite(last) || last < start || first > end) continue;
    const durationMs = Math.max(0, last - first);
    if (durationMs === 0) {
      const index = Math.min(slots.length - 1, Math.max(0, Math.floor((first - start) / slotMs)));
      slots[index] = Math.max(slots[index], Math.min(60, Math.max(0, session.activeSeconds)));
      continue;
    }
    const overlapStart = Math.max(first, start);
    const overlapEnd = Math.min(last, end);
    const activeRatio = Math.min(1, Math.max(0, session.activeSeconds) / (durationMs / 1000));
    const firstSlot = Math.max(0, Math.floor((overlapStart - start) / slotMs));
    const lastSlot = Math.min(slots.length - 1, Math.floor((Math.max(overlapStart, overlapEnd - 1) - start) / slotMs));
    for (let index = firstSlot; index <= lastSlot; index++) {
      const slotStart = start + index * slotMs;
      const slotEnd = slotStart + slotMs;
      const overlapSeconds = Math.max(0, Math.min(overlapEnd, slotEnd) - Math.max(overlapStart, slotStart)) / 1000;
      slots[index] = Math.max(slots[index], overlapSeconds * activeRatio);
    }
  }
  return Math.round(slots.reduce((total, seconds) => total + seconds, 0));
}

export function formatActiveTime(seconds: number): string {
  const safe = Math.max(0, Math.min(24 * 3600, Math.floor(seconds)));
  const hours = Math.floor(safe / 3600);
  const minutes = Math.floor((safe % 3600) / 60);
  return `${hours}h${String(minutes).padStart(2, "0")}m`;
}

export function todayBucketsOf(buckets: UsageBucket[], now = new Date()): UsageBucket[] {
  const today = localDayKey(now);
  return buckets.filter(bucket => localDayKey(new Date(bucket.bucketStart)) === today);
}

export function halfHourUsageOf(
  buckets: UsageBucket[],
  points = 10,
  now = new Date(),
  metric: WidgetValueMetric = "tokens",
): number[] {
  const slotMs = 30 * 60_000;
  const currentSlot = Math.floor(now.getTime() / slotMs) * slotMs;
  const firstSlot = currentSlot - (points - 1) * slotMs;
  const values = Array.from({ length: points }, () => 0);
  for (const bucket of buckets) {
    const slot = Math.floor(new Date(bucket.bucketStart).getTime() / slotMs) * slotMs;
    const index = Math.round((slot - firstSlot) / slotMs);
    if (index >= 0 && index < points) values[index] += bucketValue(bucket, metric);
  }
  return values;
}

export function hourlyUsageOf(
  buckets: UsageBucket[],
  now = new Date(),
  metric: WidgetValueMetric = "tokens",
): number[] {
  const today = localDayKey(now);
  const values = Array.from({ length: 24 }, () => 0);
  for (const bucket of buckets) {
    const startedAt = new Date(bucket.bucketStart);
    if (localDayKey(startedAt) === today) values[startedAt.getHours()] += bucketValue(bucket, metric);
  }
  return values;
}

export function widgetSummaryOf(
  buckets: UsageBucket[],
  sessions: UsageSession[],
  now = new Date(),
  metric: WidgetValueMetric = "tokens",
): WidgetSummary {
  const today = localDayKey(now);
  const yesterdayDate = new Date(now);
  yesterdayDate.setDate(yesterdayDate.getDate() - 1);
  const yesterday = localDayKey(yesterdayDate);
  const secondsNow = now.getHours() * 3600 + now.getMinutes() * 60 + now.getSeconds();
  let todayValue = 0;
  let yesterdayValue = 0;
  const hourly = Array.from({ length: 24 }, () => 0);
  const sources = new Map<string, number>();

  for (const bucket of buckets) {
    const startedAt = new Date(bucket.bucketStart);
    const key = localDayKey(startedAt);
    const value = bucketValue(bucket, metric);
    if (key === today) {
      todayValue += value;
      hourly[startedAt.getHours()] += value;
      const source = bucket.source || "unknown";
      sources.set(source, (sources.get(source) ?? 0) + value);
    } else if (key === yesterday) {
      const seconds = startedAt.getHours() * 3600 + startedAt.getMinutes() * 60 + startedAt.getSeconds();
      if (seconds <= secondsNow) yesterdayValue += value;
    }
  }

  const peakValue = Math.max(...hourly, 0);
  const peakHour = peakValue > 0 ? hourly.indexOf(peakValue) : null;
  const sourceTotal = [...sources.values()].reduce((sum, value) => sum + value, 0);
  const leadingEntry = [...sources.entries()].sort((left, right) => right[1] - left[1])[0];
  const leadingSource = leadingEntry && sourceTotal > 0
    ? { source: leadingEntry[0], share: leadingEntry[1] / sourceTotal }
    : null;
  const sessionCount = new Set(sessions
    .filter(session => localDayKey(new Date(session.firstMessageAt)) === today)
    .map(session => session.sessionHash)).size;

  return {
    previousChange: yesterdayValue > 0 ? ((todayValue - yesterdayValue) / yesterdayValue) * 100 : null,
    sessionCount,
    peakHour,
    leadingSource,
  };
}

export function dailyUsageOf(
  buckets: UsageBucket[],
  days = 7,
  now = new Date(),
  metric: WidgetValueMetric = "tokens",
): number[] {
  const keys: string[] = [];
  for (let offset = days - 1; offset >= 0; offset--) {
    const date = new Date(now);
    date.setHours(0, 0, 0, 0);
    date.setDate(date.getDate() - offset);
    keys.push(localDayKey(date));
  }
  const positions = new Map(keys.map((key, index) => [key, index]));
  const values = Array.from({ length: days }, () => 0);
  for (const bucket of buckets) {
    const index = positions.get(localDayKey(new Date(bucket.bucketStart)));
    if (index !== undefined) values[index] += bucketValue(bucket, metric);
  }
  return values;
}

export function previousDayChange(values: number[]): number | null {
  if (values.length < 2) return null;
  const previous = values.at(-2) ?? 0;
  const current = values.at(-1) ?? 0;
  return previous > 0 ? ((current - previous) / previous) * 100 : null;
}

export function usagePulseOf(buckets: UsageBucket[], points = 10): number[] {
  const byTime = new Map<string, number>();
  for (const bucket of buckets) {
    byTime.set(bucket.bucketStart, (byTime.get(bucket.bucketStart) ?? 0) + bucket.totalTokens);
  }
  const values = [...byTime.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .slice(-points)
    .map(([, value]) => value);
  const maximum = Math.max(...values, 0);
  return values.map(value => maximum > 0 ? value / maximum : 0);
}
