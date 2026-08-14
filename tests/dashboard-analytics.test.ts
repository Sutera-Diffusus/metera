import { describe, expect, it } from "vitest";
import { activityHeatmap, dailyInsights, forecastUsage, relationshipMatrix, secondarySummary } from "../src/dashboard/dashboardAnalytics";
import type { UsageBucket, UsageSession } from "../src/lib/types";

const bucket = (source: string, provider: string, model: string, tokens: number): UsageBucket => ({
  source, provider, model, project: "Metera", hostname: "desktop", bucketStart: "2026-08-02T08:00:00+08:00",
  inputTokens: tokens, cachedInputTokens: 0, outputTokens: 0, reasoningOutputTokens: 0, totalTokens: tokens,
});

describe("dashboard visualization transforms", () => {
  it("builds matrix cells without changing token totals", () => {
    const rows = [bucket("codex", "official", "gpt-5", 100), bucket("codex", "free", "gpt-5", 70), bucket("zcode", "free", "glm", 30)];
    const matrix = relationshipMatrix(rows, "provider", "source");
    expect(matrix.rowTotals.get("free")).toBe(100);
    expect(matrix.values.get("free\u0000codex")).toBe(70);
    expect([...matrix.rowTotals.values()].reduce((sum, value) => sum + value, 0)).toBe(200);
  });

  it("switches the matrix to priced costs without turning unknown costs into zero", () => {
    const priced = { ...bucket("codex", "official", "gpt-5", 100), estimatedCost: 1.25 };
    const unknown = { ...bucket("codex", "free", "unpriced", 70), estimatedCost: null };
    const matrix = relationshipMatrix([priced, unknown], "provider", "source", 7, 8, "cost");
    expect(matrix.rowTotals.get("official")).toBe(1.25);
    expect(matrix.rowTotals.has("free")).toBe(false);
    expect(matrix.values.get("official\u0000codex")).toBe(1.25);
  });

  it("assigns prompt hours to the session weekday and preserves active seconds", () => {
    const hours = Array(24).fill(0); hours[9] = 1; hours[10] = 3;
    const session: UsageSession = { source: "codex", project: "Metera", hostname: "desktop", sessionHash: "one", firstMessageAt: "2026-08-03T09:00:00+08:00", lastMessageAt: "2026-08-03T10:00:00+08:00", durationSeconds: 3600, activeSeconds: 1200, messageCount: 8, userMessageCount: 4, userPromptHours: hours };
    const cells = activityHeatmap([session]);
    expect(cells).toHaveLength(2);
    expect(cells.reduce((sum, cell) => sum + cell.prompts, 0)).toBe(4);
    expect(cells.reduce((sum, cell) => sum + cell.activeSeconds, 0)).toBeCloseTo(1200);
    expect(cells.every(cell => cell.day === 0)).toBe(true);
  });

  it("derives honest secondary metrics from usage and sessions", () => {
    const rows = [bucket("codex", "official", "gpt-5", 300), bucket("zcode", "free", "glm", 100)];
    rows[0].cachedInputTokens = 300; rows[0].inputTokens = 300; rows[0].totalTokens = 600;
    const hours = Array(24).fill(0); hours[9] = 3; hours[18] = 1;
    const session: UsageSession = { source: "codex", project: "Metera", hostname: "desktop", sessionHash: "summary", firstMessageAt: "2026-08-03T09:00:00+08:00", lastMessageAt: "2026-08-03T10:00:00+08:00", durationSeconds: 3600, activeSeconds: 1200, messageCount: 8, userMessageCount: 4, userPromptHours: hours };
    const summary = secondarySummary(rows, [session]);
    expect(summary.cacheRate).toBeCloseTo(3 / 7);
    expect(summary.topToolShare).toBeCloseTo(6 / 7);
    expect(summary.peakHourShare).toBe(1);
    expect(dailyInsights(rows)).toHaveLength(1);
  });

  it("only forecasts from at least fourteen daily observations", () => {
    const rows = Array.from({ length: 14 }, (_, index) => ({ ...bucket("codex", "official", "gpt-5", 100 + index * 10), bucketStart: `2026-07-${String(index + 1).padStart(2, "0")}T08:00:00+08:00` }));
    expect(forecastUsage(rows.slice(0, 13))).toEqual([]);
    const forecast = forecastUsage(rows);
    expect(forecast).toHaveLength(7);
    expect(forecast.every(point => point.tokens >= 0 && point.tokenHigh >= point.tokens && point.tokenLow <= point.tokens)).toBe(true);
    expect(forecast.every(point => point.cost === null)).toBe(true);
  });
});
