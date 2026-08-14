import { describe, expect, it } from "vitest";
import { cacheHitRate, groupBySource, groupProviders, totalsOf } from "../src/lib/analytics";
import type { UsageBucket } from "../src/lib/types";

const usage = (source: string, input: number, cached: number): UsageBucket => ({ source, provider:"api.openai.com", model: "gpt-5.6-luna", project: "unknown", hostname: "desktop", bucketStart: new Date().toISOString(), inputTokens: input, outputTokens: 100, cachedInputTokens: cached, reasoningOutputTokens: 20, totalTokens: input + cached + 120 });

describe("Metera analytics", () => {
  it("calculates cache hit rate from input and cached input tokens", () => {
    expect(cacheHitRate(20, 80)).toBe(.8);
    expect(cacheHitRate(0, 0)).toBe(0);
  });
  it("includes each non-overlapping token class exactly once", () => expect(totalsOf([usage("Codex", 1000, 500)]).tokens).toBe(1620));
  it("groups independent sources", () => {
    const groups = groupBySource([usage("Codex", 1000, 0), usage("WorkBuddy", 2000, 0)]);
    expect(groups.map(group => group.source)).toEqual(["WorkBuddy", "Codex"]);
  });
  it("groups the same provider across tools without changing totals", () => {
    const rows=[usage("Codex",1000,500),usage("ZCode",2000,700)];
    const providers=groupProviders(rows);
    expect(providers).toHaveLength(1);
    expect(providers[0].tokens).toBe(totalsOf(rows).tokens);
    expect(providers[0].sources).toHaveLength(2);
  });
});
