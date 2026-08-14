import type { UsageBucket, UsageSession } from "../../lib/types";
import type { OverviewActivityPoint, OverviewMetric } from "../charts/OverviewActivityChart";

const bucketTokens = (bucket: UsageBucket) => bucket.inputTokens + bucket.cachedInputTokens + bucket.outputTokens + bucket.reasoningOutputTokens;

export function utcHourToLocalHour(hour: number, offsetMinutes = -new Date().getTimezoneOffset()) {
  const offsetHours = Math.round(offsetMinutes / 60);
  return ((hour + offsetHours) % 24 + 24) % 24;
}

export function activeByHour(sessions: UsageSession[], offsetMinutes = -new Date().getTimezoneOffset()) {
  const values = Array.from({ length: 24 }, () => 0);
  for (const session of sessions) {
    const promptTotal = session.userPromptHours.reduce((sum, value) => sum + value, 0) || 1;
    session.userPromptHours.forEach((prompts, utcHour) => {
      if (!prompts) return;
      const localHour = utcHourToLocalHour(utcHour, offsetMinutes);
      values[localHour] += session.activeSeconds * prompts / promptTotal / 60;
    });
  }
  return values;
}

export function activityPoints(
  buckets: UsageBucket[],
  sessions: UsageSession[],
  metric: OverviewMetric,
  rate: number,
  valueOf: (bucket: UsageBucket, metric: OverviewMetric, rate: number) => number | null,
  offsetMinutes = -new Date().getTimezoneOffset(),
): OverviewActivityPoint[] {
  const usage = Array.from({ length: 24 }, () => 0);
  const tokenPresence = Array.from({ length: 24 }, () => 0);
  for (const bucket of buckets) {
    const value = valueOf(bucket, metric, rate);
    const hour = new Date(bucket.bucketStart).getHours();
    if (value != null) usage[hour] += value;
    tokenPresence[hour] += bucketTokens(bucket);
  }
  const active = activeByHour(sessions, offsetMinutes);
  return usage.map((value, hour) => ({
    hour,
    usage: value,
    activeMinutes: tokenPresence[hour] > 0 ? Math.min(60, active[hour]) : 0,
  }));
}
