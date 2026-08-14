import { describe, expect, it } from "vitest";
import { estimateCost, plansForProvider, priceForModel } from "../src/lib/pricing";
import type { UsageBucket } from "../src/lib/types";

const bucket = (model: string): UsageBucket => ({ source: "Codex", provider:"api.openai.com", model, project: "unknown", hostname: "desktop", bucketStart: new Date().toISOString(), inputTokens: 1_000_000, outputTokens: 1_000_000, cachedInputTokens: 1_000_000, reasoningOutputTokens: 0, totalTokens: 3_000_000 });

describe("Metera pricing", () => {
  it("prices cached input separately", () => expect(estimateCost(bucket("gpt-5.6-sol"))).toBeCloseTo(35.5));
  it("uses current Terra and Luna tiers", () => {
    expect(estimateCost(bucket("gpt-5.6-terra"))).toBeCloseTo(14.2);
    expect(estimateCost(bucket("gpt-5.6-luna"))).toBeCloseTo(1.42);
  });
  it("honors an explicit zero provider cost", () => expect(estimateCost({ ...bucket("gpt-5.6-sol"), estimatedCost: 0 })).toBe(0));
  it("does not invent unknown model prices", () => expect(estimateCost(bucket("future-codex-model"))).toBeNull());
  it("lists subscription tiers for the plan selector (chatgpt/codex)", () => {
    const plans = plansForProvider("chatgpt");
    expect(plans.length).toBeGreaterThanOrEqual(4);
    const nonZero = plans.filter(plan => plan.priceMonthly > 0);
    expect(nonZero.some(plan => plan.plan === "Plus" && plan.priceMonthly === 20)).toBe(true);
    expect(nonZero.some(plan => plan.plan === "Go" && plan.priceMonthly === 8)).toBe(true);
  });
  it("lists kimi subscription tiers incl Allegretto 199", () => {
    const plans = plansForProvider("kimi");
    const nonZero = plans.filter(plan => plan.priceMonthly > 0);
    expect(nonZero.some(plan => plan.plan === "Allegretto" && plan.priceMonthly === 199)).toBe(true);
    expect(nonZero.some(plan => plan.plan === "日常使用" && plan.priceMonthly === 49)).toBe(true);
  });
  it("matches dsh source deepseek models (explicit entries)", () => {
    expect(priceForModel("dsh", "deepseek-v4-pro")).toEqual({ input: 3, cached: 0.025, output: 6, currency: "CNY" });
    expect(priceForModel("dsh", "deepseek-v4-flash")).toEqual({ input: 1, cached: 0.02, output: 2, currency: "CNY" });
    // 1M input + 1M cached + 1M output，CNY 折 USD（¥9.025 / 7.2）。
    expect(estimateCost({ ...bucket("deepseek-v4-pro"), source: "dsh" })).toBeCloseTo(9.025 / 7.2);
  });
});
