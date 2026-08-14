import { describe, expect, it } from "vitest";
import type { UsageBucket, UsageSession } from "../src/lib/types";
import { activeByHour, activityPoints, utcHourToLocalHour } from "../src/dashboard/views/overviewActivity";

const session = (hour: number): UsageSession => ({
  sessionHash: "session",
  source: "reasonix",
  startedAt: 0,
  endedAt: 0,
  durationSeconds: 3600,
  activeSeconds: 1800,
  messageCount: 1,
  userPromptHours: Array.from({ length: 24 }, (_, index) => index === hour ? 1 : 0),
});

const bucket = (hour: number, tokens: number): UsageBucket => ({
  bucketStart: new Date(2026, 7, 8, hour).getTime(),
  source: "reasonix",
  model: "deepseek/deepseek-v4-flash",
  inputTokens: tokens,
  cachedInputTokens: 0,
  outputTokens: 0,
  reasoningOutputTokens: 0,
  requestCount: 1,
});

describe("overview activity semantics", () => {
  it("converts UTC prompt hours to local hours", () => {
    expect(utcHourToLocalHour(17, 480)).toBe(1);
    expect(activeByHour([session(17)], 480)[1]).toBe(30);
  });

  it("does not draw active minutes in hours without token usage", () => {
    const points = activityPoints([bucket(2, 1200)], [session(17)], "tokens", 1, item => item.inputTokens, 480);
    expect(points[1].activeMinutes).toBe(0);
    expect(points[2].usage).toBe(1200);
  });

  it("caps an hourly activity point at sixty minutes", () => {
    const long = { ...session(17), activeSeconds: 9_000 };
    const points = activityPoints([bucket(1, 1200)], [long], "tokens", 1, item => item.inputTokens, 480);
    expect(points[1].activeMinutes).toBe(60);
  });
});
