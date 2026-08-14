//! 定价表：单一 JSON 数据源（`pricing_data.json`），与前端 `src/lib/pricing.ts` 共享同一份数据。
//! Rust 侧编译期 `include_str!`；前端从 `../../crates/core/src/pricing_data.json` 引用。
//! 匹配规则：source+model 规范化（小写+压缩空白）后精确/前缀匹配，先精确后前缀，
//! 避免无界 contains 造成的误匹配（如 deepseek 吞掉 deepseek-v4-pro）。
//! 单位为 元或美元 / 百万 token，具体币种见数据项 currency 字段。

use crate::usage::UsageBucket;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub cached: f64,
    pub output: f64,
    pub currency: &'static str,
}

/// CNY→USD 汇率（近似值，用于统一币种核算；仪表盘费用统一以 USD 显示）。
pub const CNY_PER_USD: f64 = 7.2;

/// 把价格统一折算成 USD（CNY 除以汇率，USD 原样）。
fn to_usd(price: &Price) -> Price {
    match price.currency {
        "CNY" => Price {
            input: price.input / CNY_PER_USD,
            cached: price.cached / CNY_PER_USD,
            output: price.output / CNY_PER_USD,
            currency: "USD",
        },
        _ => *price,
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PriceEntry {
    pub source: String,
    pub model: String,
    pub input: f64,
    pub cached: f64,
    pub output: f64,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub note: Option<String>,
}

fn default_currency() -> String { "USD".into() }

#[derive(Debug, Clone, Deserialize)]
pub struct PlanEntry {
    pub provider: String,
    pub plan: String,
    pub price_monthly: f64,
    pub price_yearly_per_month: f64,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub tier: u32,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PricingFile {
    #[allow(dead_code)]
    _meta: serde_json::Value,
    models: Vec<PriceEntry>,
    plans: Vec<PlanEntry>,
}

/// 编译期加载单一数据源；测试可覆盖解析与匹配。
pub fn price_entries() -> &'static [PriceEntry] {
    static ENTRIES: std::sync::OnceLock<Vec<PriceEntry>> = std::sync::OnceLock::new();
    ENTRIES.get_or_init(|| {
        let raw = include_str!("pricing_data.json");
        let file: PricingFile = serde_json::from_str(raw).expect("pricing_data.json 解析失败");
        file.models
    })
}

pub fn plan_entries() -> &'static [PlanEntry] {
    static PLANS: std::sync::OnceLock<Vec<PlanEntry>> = std::sync::OnceLock::new();
    PLANS.get_or_init(|| {
        let raw = include_str!("pricing_data.json");
        let file: PricingFile = serde_json::from_str(raw).expect("pricing_data.json 解析失败");
        file.plans
    })
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn price_for_model(source: &str, model: &str) -> Option<Price> {
    let src = normalize(source);
    let mdl = normalize(model);
    if mdl.is_empty() { return None; }
    // 剥离 provider 前缀（如 `deepseek/deepseek-v4-flash` → `deepseek-v4-flash`、
    // `kimi-code/k3` → `k3`），真实数据中模型名常带来源前缀。
    let bare = mdl.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(&mdl);
    let mut prefix_match: Option<(Price, usize)> = None;
    // 候选名：优先原 model（可含前缀），其次裸名。
    let candidates = [mdl.as_str(), bare];
    // 第一轮：同 source 精确匹配（先原 model 名，后裸名）。
    for entry in price_entries() {
        if normalize(&entry.source) != src { continue; }
        let entry_model = normalize(&entry.model);
        if entry_model.is_empty() { continue; }
        for cand in candidates {
            if cand == entry_model {
                return Some(Price { input: entry.input, cached: entry.cached, output: entry.output, currency: &entry.currency });
            }
        }
    }
    // 第二轮：同 source 前缀匹配（最长优先）。
    for entry in price_entries() {
        if normalize(&entry.source) != src { continue; }
        let entry_model = normalize(&entry.model);
        if entry_model.is_empty() { continue; }
        for cand in candidates {
            if cand.starts_with(&entry_model) && entry_model.len() > prefix_match.as_ref().map_or(0, |(_, l)| *l) {
                prefix_match = Some((Price { input: entry.input, cached: entry.cached, output: entry.output, currency: &entry.currency }, entry_model.len()));
            }
        }
    }
    if let Some((p, _)) = prefix_match { return Some(p); }
    // 第三轮：跨 source 全局回退（claude-code 里跑 deepseek、zcode 里跑 gpt/k3 等），
    // 按模型名精确匹配，先裸名后原 model 名。
    for entry in price_entries() {
        let entry_model = normalize(&entry.model);
        if entry_model.is_empty() { continue; }
        for cand in candidates {
            if cand == entry_model {
                return Some(Price { input: entry.input, cached: entry.cached, output: entry.output, currency: &entry.currency });
            }
        }
    }
    // 第四轮：跨 source 前缀匹配（最长优先）。
    let mut global_prefix: Option<(Price, usize)> = None;
    for entry in price_entries() {
        let entry_model = normalize(&entry.model);
        if entry_model.is_empty() { continue; }
        for cand in candidates {
            if cand.starts_with(&entry_model) && entry_model.len() > global_prefix.as_ref().map_or(0, |(_, l)| *l) {
                global_prefix = Some((Price { input: entry.input, cached: entry.cached, output: entry.output, currency: &entry.currency }, entry_model.len()));
            }
        }
    }
    global_prefix.map(|(p, _)| p)
}

/// 单个 bucket 的估算费用（统一折算成 USD）；未知模型返回 `None`（调用方跳过并计入定价覆盖率）。
pub fn estimate_cost(bucket: &UsageBucket) -> Option<f64> {
    let price = to_usd(&price_for_model(&bucket.source, &bucket.model)?);
    Some(
        bucket.input_tokens as f64 / 1e6 * price.input
            + bucket.cached_input_tokens as f64 / 1e6 * price.cached
            + (bucket.output_tokens + bucket.reasoning_output_tokens) as f64 / 1e6 * price.output,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(source: &str, model: &str) -> UsageBucket {
        UsageBucket {
            source: source.into(),
            provider: "provider.test".into(),
            model: model.into(),
            project: "proj".into(),
            hostname: "host".into(),
            bucket_start: "2026-08-03T10:00:00.000Z".into(),
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            cached_input_tokens: 2_000_000,
            reasoning_output_tokens: 500_000,
            total_tokens: 4_000_000,
        }
    }

    #[test]
    fn known_models_match_frontend_prices() {
        assert_eq!(
            price_for_model("codex", "gpt-5.5"),
            Some(Price { input: 5.0, cached: 0.5, output: 30.0, currency: "USD" })
        );
        assert_eq!(
            price_for_model("kimi-code", "kimi-k3"),
            Some(Price { input: 20.0, cached: 2.0, output: 100.0, currency: "CNY" })
        );
        assert_eq!(
            price_for_model("reasonix", "deepseek-v4-pro"),
            Some(Price { input: 3.0, cached: 0.025, output: 6.0, currency: "CNY" })
        );
    }

    #[test]
    fn more_specific_branches_win() {
        // deepseek-v4-pro 必须精确命中,不被 flash 或前缀吞掉。
        assert_eq!(
            price_for_model("reasonix", "deepseek-v4-pro"),
            Some(Price { input: 3.0, cached: 0.025, output: 6.0, currency: "CNY" })
        );
        assert_eq!(
            price_for_model("reasonix", "deepseek-v4-flash"),
            Some(Price { input: 1.0, cached: 0.02, output: 2.0, currency: "CNY" })
        );
        assert_eq!(
            price_for_model("codex", "gpt-5.3-codex"),
            Some(Price { input: 1.75, cached: 0.175, output: 14.0, currency: "USD" })
        );
        // glm-5.2 不被 glm-5 前缀吞掉。
        assert_eq!(
            price_for_model("zcode", "glm-5.2"),
            Some(Price { input: 8.0, cached: 2.0, output: 28.0, currency: "CNY" })
        );
    }

    #[test]
    fn prefixed_and_cross_source_models_match() {
        // 带 provider 前缀的模型名（剥离前缀后命中）。
        assert_eq!(
            price_for_model("reasonix", "deepseek/deepseek-v4-flash"),
            Some(Price { input: 1.0, cached: 0.02, output: 2.0, currency: "CNY" })
        );
        assert_eq!(
            price_for_model("kimi-code", "kimi-code/k3"),
            Some(Price { input: 20.0, cached: 2.0, output: 100.0, currency: "CNY" })
        );
        assert_eq!(
            price_for_model("kimi-code", "kimi-code/k3-256k"),
            Some(Price { input: 20.0, cached: 2.0, output: 100.0, currency: "CNY" })
        );
        // 跨 source 使用（claude-code 里跑 deepseek、zcode 里跑 gpt/k3、codex 里跑 k3）。
        assert_eq!(
            price_for_model("claude-code", "deepseek-v4-pro"),
            Some(Price { input: 3.0, cached: 0.025, output: 6.0, currency: "CNY" })
        );
        assert_eq!(
            price_for_model("zcode", "gpt-5.5"),
            Some(Price { input: 5.0, cached: 0.5, output: 30.0, currency: "USD" })
        );
        assert_eq!(
            price_for_model("zcode", "k3"),
            Some(Price { input: 20.0, cached: 2.0, output: 100.0, currency: "CNY" })
        );
        assert_eq!(
            price_for_model("codex", "k3"),
            Some(Price { input: 20.0, cached: 2.0, output: 100.0, currency: "CNY" })
        );
        // dsh 源显式条目（§18.5）。
        assert_eq!(
            price_for_model("dsh", "deepseek-v4-pro"),
            Some(Price { input: 3.0, cached: 0.025, output: 6.0, currency: "CNY" })
        );
        assert_eq!(
            price_for_model("dsh", "deepseek-v4-flash"),
            Some(Price { input: 1.0, cached: 0.02, output: 2.0, currency: "CNY" })
        );
        // 未知模型仍然返回 None，不被跨源前缀误吞。
        assert_eq!(price_for_model("codex", "mystery-9000"), None);
    }

    #[test]
    fn unknown_model_returns_none() {
        assert_eq!(price_for_model("codex", "mystery-9000"), None);
        assert_eq!(estimate_cost(&bucket("codex", "mystery-9000")), None);
    }

    #[test]
    fn estimate_cost_matches_frontend_formula() {
        // 1M input @5 + 2M cached @0.5 + (0.5M output + 0.5M reasoning) @30
        let cost = estimate_cost(&bucket("codex", "gpt-5.5")).unwrap();
        let expected = 5.0 + 2.0 * 0.5 + 1.0 * 30.0;
        assert!((cost - expected).abs() < 1e-9, "cost={cost} expected={expected}");
    }

    #[test]
    fn cny_prices_are_converted_to_usd() {
        // deepseek-v4-pro 是 CNY 定价（3/0.025/6 元），estimate_cost 应折算成 USD。
        let cost = estimate_cost(&bucket("reasonix", "deepseek-v4-pro")).unwrap();
        let expected = (3.0 + 2.0 * 0.025 + 1.0 * 6.0) / CNY_PER_USD;
        assert!((cost - expected).abs() < 1e-9, "cost={cost} expected={expected}");
        // 定价条目的 currency 随 Price 返回，用于展示侧区分。
        assert_eq!(price_for_model("reasonix", "deepseek-v4-pro").unwrap().currency, "CNY");
        assert_eq!(price_for_model("codex", "gpt-5.5").unwrap().currency, "USD");
    }

    #[test]
    fn json_data_is_valid_and_has_all_sections() {
        assert!(!price_entries().is_empty(), "models 表不能为空");
        assert!(!plan_entries().is_empty(), "plans 表不能为空");
        // 抽查关键模型都在。
        for needle in ["gpt-5.6-sol", "claude-opus-5", "kimi-k3", "deepseek-v4-flash", "glm-5.2"] {
            assert!(
                price_entries().iter().any(|e| e.model.to_lowercase().contains(needle)),
                "缺少模型 {needle}"
            );
        }
    }
}
