// 定价表：单一 JSON 数据源（crates/core/src/pricing_data.json），与 Rust pricing.rs 共享。
// 匹配规则与 Rust 侧一致：source+model 规范化（小写+压缩空白）后精确/前缀匹配。
import type { UsageBucket } from "./types";
import pricingData from "../../crates/core/src/pricing_data.json";

type Price = { input: number; cached: number; output: number; currency: string };

export const CNY_PER_USD = 7.2;

function toUsd(price: Price): Price {
  if (price.currency === "CNY") {
    return { input: price.input / CNY_PER_USD, cached: price.cached / CNY_PER_USD, output: price.output / CNY_PER_USD, currency: "USD" };
  }
  return price;
}

export type PriceEntry = {
  source: string;
  model: string;
  input: number;
  cached: number;
  output: number;
  currency: string;
  note?: string;
};

export type PlanEntry = {
  provider: string;
  plan: string;
  priceMonthly: number;
  priceYearlyPerMonth: number;
  currency: string;
  tier: number;
  note?: string;
};

// pricing_data.json 的 plans 项是 snake_case（price_monthly），这里做一次显式映射，
// 避免 `(pricingData as any).plans` 的字段名错位（priceMonthly 恒为 undefined）。
const MODELS: PriceEntry[] = (pricingData as any).models;
const PLANS: PlanEntry[] = ((pricingData as any).plans ?? []).map((plan: Record<string, unknown>) => ({
  provider: String(plan.provider ?? ""),
  plan: String(plan.plan ?? ""),
  priceMonthly: Number(plan.price_monthly ?? 0),
  priceYearlyPerMonth: Number(plan.price_yearly_per_month ?? 0),
  currency: String(plan.currency ?? "USD"),
  tier: Number(plan.tier ?? 0),
  note: typeof plan.note === "string" ? plan.note : undefined,
}));

function normalize(value: string): string {
  return value.trim().toLowerCase().split(/\s+/).join(" ");
}

export function priceForModel(source: string, model: string): Price | null {
  const src = normalize(source);
  const mdl = normalize(model);
  if (!mdl) return null;
  // 剥离 provider 前缀（如 `deepseek/deepseek-v4-flash` → `deepseek-v4-flash`、`kimi-code/k3` → `k3`）。
  const bare = mdl.split("/").pop() || mdl;
  const candidates = [mdl, bare];
  // 第一轮：同 source 精确匹配。
  for (const entry of MODELS) {
    if (normalize(entry.source) !== src) continue;
    const entryModel = normalize(entry.model);
    if (!entryModel) continue;
    for (const cand of candidates) {
      if (cand === entryModel) return { input: entry.input, cached: entry.cached, output: entry.output, currency: entry.currency };
    }
  }
  // 第二轮：同 source 前缀匹配（最长优先）。
  let prefixMatch: Price | null = null;
  let prefixLen = 0;
  for (const entry of MODELS) {
    if (normalize(entry.source) !== src) continue;
    const entryModel = normalize(entry.model);
    if (!entryModel) continue;
    for (const cand of candidates) {
      if (cand.startsWith(entryModel) && entryModel.length > prefixLen) {
        prefixLen = entryModel.length;
        prefixMatch = { input: entry.input, cached: entry.cached, output: entry.output, currency: entry.currency };
      }
    }
  }
  if (prefixMatch) return prefixMatch;
  // 第三轮：跨 source 全局回退（claude-code 里跑 deepseek、zcode 里跑 gpt/k3 等），精确匹配。
  for (const entry of MODELS) {
    const entryModel = normalize(entry.model);
    if (!entryModel) continue;
    for (const cand of candidates) {
      if (cand === entryModel) return { input: entry.input, cached: entry.cached, output: entry.output, currency: entry.currency };
    }
  }
  // 第四轮：跨 source 前缀匹配（最长优先）。
  let globalPrefix: Price | null = null;
  let globalLen = 0;
  for (const entry of MODELS) {
    const entryModel = normalize(entry.model);
    if (!entryModel) continue;
    for (const cand of candidates) {
      if (cand.startsWith(entryModel) && entryModel.length > globalLen) {
        globalLen = entryModel.length;
        globalPrefix = { input: entry.input, cached: entry.cached, output: entry.output, currency: entry.currency };
      }
    }
  }
  return globalPrefix;
}

export function estimateCost(bucket: UsageBucket): number | null {
  if (Object.prototype.hasOwnProperty.call(bucket, "estimatedCost")) return (bucket as any).estimatedCost ?? null;
  const price = priceForModel(bucket.source, bucket.model); if (!price) return null;
  const usd = toUsd(price);
  return bucket.inputTokens / 1e6 * usd.input + bucket.cachedInputTokens / 1e6 * usd.cached + (bucket.outputTokens + bucket.reasoningOutputTokens) / 1e6 * usd.output;
}

export function plansForProvider(provider: string): PlanEntry[] {
  return PLANS.filter(p => p.provider.toLowerCase() === provider.toLowerCase()).sort((a, b) => a.tier - b.tier);
}
