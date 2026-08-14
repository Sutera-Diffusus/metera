import { describe, expect, it } from "vitest";
import { activeSeconds24hOf, dailyUsageOf, formatActiveTime, formatWidgetTokens, halfHourUsageOf, hourlyUsageOf, previousDayChange, todayBucketsOf, usagePulseOf, widgetSummaryOf } from "../src/lib/widget";
import type { UsageBucket, UsageSession } from "../src/lib/types";

const bucket = (bucketStart: string, totalTokens: number): UsageBucket => ({
  source: "Codex", provider: "openai", model: "gpt", project: "Metera", hostname: "PC", bucketStart,
  inputTokens: totalTokens, outputTokens: 0, cachedInputTokens: 0, reasoningOutputTokens: 0, totalTokens,
});

const session = (sessionHash: string, firstMessageAt: string): UsageSession => ({
  source: "Codex", project: "Metera", hostname: "PC", sessionHash, firstMessageAt, lastMessageAt: firstMessageAt,
  durationSeconds: 60, activeSeconds: 30, messageCount: 2, userMessageCount: 1, userPromptHours: [],
});

describe("floating widget pulse", () => {
  it("keeps widget values at two decimal places", () => {
    expect(formatWidgetTokens(100)).toBe("100.00");
    expect(formatWidgetTokens(12_100)).toBe("12.10K");
    expect(formatWidgetTokens(3_000_000)).toBe("3.00M");
  });

  it("formats the rolling 24-hour active time and prorates boundary sessions", () => {
    const now = new Date("2026-08-03T12:00:00Z");
    const recent = { ...session("recent", "2026-08-03T09:00:00Z"), lastMessageAt: "2026-08-03T11:00:00Z", durationSeconds: 7200, activeSeconds: 3600 };
    const boundary = { ...session("boundary", "2026-08-02T10:00:00Z"), lastMessageAt: "2026-08-02T14:00:00Z", durationSeconds: 14400, activeSeconds: 7200 };
    expect(activeSeconds24hOf([recent, boundary], now)).toBe(7200);
    expect(formatActiveTime(7200 + 55 * 60)).toBe("2h55m");
  });

  it("does not double count overlapping activity from parallel tools", () => {
    const now = new Date("2026-08-03T12:00:00Z");
    const first = { ...session("one", "2026-08-03T10:00:00Z"), lastMessageAt: "2026-08-03T11:00:00Z", durationSeconds: 3600, activeSeconds: 1800 };
    const parallel = { ...session("two", "2026-08-03T10:00:00Z"), lastMessageAt: "2026-08-03T11:00:00Z", durationSeconds: 3600, activeSeconds: 1800 };
    expect(activeSeconds24hOf([first, parallel], now)).toBe(1800);
  });
  it("groups equal timestamps and normalizes the latest points", () => {
    expect(usagePulseOf([
      bucket("2026-08-02T10:00:00Z", 20),
      bucket("2026-08-02T09:00:00Z", 10),
      bucket("2026-08-02T10:00:00Z", 20),
    ])).toEqual([0.25, 1]);
  });

  it("keeps only the requested trailing points", () => {
    expect(usagePulseOf([
      bucket("2026-08-02T09:00:00Z", 10),
      bucket("2026-08-02T10:00:00Z", 20),
      bucket("2026-08-02T11:00:00Z", 30),
    ], 2)).toEqual([2 / 3, 1]);
  });

  it("builds ten real half-hour slots ending at the current slot", () => {
    const now = new Date("2026-08-03T12:17:00Z");
    expect(halfHourUsageOf([
      bucket("2026-08-03T07:30:00Z", 5),
      bucket("2026-08-03T11:30:00Z", 20),
      bucket("2026-08-03T12:00:00Z", 40),
    ], 10, now)).toEqual([5, 0, 0, 0, 0, 0, 0, 0, 20, 40]);
  });

  it("builds a local-day series with one slot for every hour", () => {
    const now = new Date("2026-08-03T12:17:00");
    const values = hourlyUsageOf([
      bucket("2026-08-03T00:10:00", 5),
      bucket("2026-08-03T00:40:00", 7),
      bucket("2026-08-03T12:00:00", 40),
      bucket("2026-08-02T23:30:00", 99),
    ], now);
    expect(values).toHaveLength(24);
    expect(values[0]).toBe(12);
    expect(values[12]).toBe(40);
    expect(values.reduce((sum, value) => sum + value, 0)).toBe(52);
  });

  it("separates today and returns a seven-day series with empty days", () => {
    const now = new Date("2026-08-03T12:00:00");
    const rows = [
      bucket("2026-08-01T08:00:00", 10),
      bucket("2026-08-02T08:00:00", 20),
      bucket("2026-08-03T08:00:00", 30),
    ];
    expect(todayBucketsOf(rows, now).map(item => item.totalTokens)).toEqual([30]);
    expect(dailyUsageOf(rows, 7, now)).toEqual([0, 0, 0, 0, 10, 20, 30]);
    expect(previousDayChange(dailyUsageOf(rows, 7, now))).toBe(50);
  });

  it("does not invent a percentage when yesterday is empty", () => {
    expect(previousDayChange([0, 10])).toBeNull();
  });

  it("builds the approved A1 summary from local-day data", () => {
    const now = new Date("2026-08-03T12:30:00");
    const rows = [
      { ...bucket("2026-08-02T08:00:00", 80), source: "Codex" },
      { ...bucket("2026-08-02T13:00:00", 500), source: "Codex" },
      { ...bucket("2026-08-03T09:00:00", 90), source: "Codex" },
      { ...bucket("2026-08-03T09:30:00", 30), source: "ZCode" },
    ];
    const summary = widgetSummaryOf(rows, [
      session("one", "2026-08-03T08:00:00"),
      session("one", "2026-08-03T08:05:00"),
      session("two", "2026-08-03T11:00:00"),
      session("old", "2026-08-02T11:00:00"),
    ], now);
    expect(summary.previousChange).toBe(50);
    expect(summary.sessionCount).toBe(2);
    expect(summary.peakHour).toBe(9);
    expect(summary.leadingSource).toEqual({ source: "Codex", share: .75 });
  });

  it("uses cost values and leaves unavailable summary values explicit", () => {
    const now = new Date("2026-08-03T12:30:00");
    const summary = widgetSummaryOf([
      { ...bucket("2026-08-03T10:00:00", 100), estimatedCost: 2.5 },
    ], [], now, "cost");
    expect(summary.previousChange).toBeNull();
    expect(summary.sessionCount).toBe(0);
    expect(summary.peakHour).toBe(10);
    expect(summary.leadingSource).toEqual({ source: "Codex", share: 1 });
  });
});
