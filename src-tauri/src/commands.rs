use crate::{services, state::{AppCtx, AppSettings, SyncState}, windows};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashSet, fs, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf}, sync::{Mutex, OnceLock}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus { version: String, data_dir: String, sources: Vec<&'static str> }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageResponse {
    buckets: Vec<metera_core::usage::UsageBucket>,
    sessions: Vec<metera_core::usage::UsageSession>,
    has_any_data: bool,
}

#[tauri::command]
pub fn get_app_status(app: AppHandle) -> AppStatus {
    let ctx = app.state::<AppCtx>();
    AppStatus { version: app.package_info().version.to_string(), data_dir: ctx.data_dir.to_string_lossy().to_string(), sources: vec!["Codex", "Claude Code", "Kimi Code", "WorkBuddy", "ZCode", "Reasonix", "DeepSeek Harness"] }
}

#[tauri::command]
pub fn fetch_usage(app: AppHandle, start: String, end: String) -> Result<UsageResponse, String> {
    let ctx = app.state::<AppCtx>();
    let repository = ctx.usage.lock().unwrap();
    let buckets = repository.buckets_between(&start, &end).map_err(|e| e.to_string())?;
    // 用 overlapping 语义（与每日报告一致）：跨日界会话的活跃时长计入窗口，
    // 否则今天启动的会话若 first_message_at 在昨天，活跃时长会被漏掉。
    let sessions = repository.sessions_overlapping(&start, &end).map_err(|e| e.to_string())?;
    Ok(UsageResponse { has_any_data: !buckets.is_empty() || !sessions.is_empty(), buckets, sessions })
}

#[tauri::command]
pub async fn get_exchange_rate() -> Result<f64, String> {
    let payload: Value = reqwest::Client::new()
        .get("https://api.frankfurter.app/latest?from=USD&to=CNY")
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .map_err(|error| format!("汇率请求失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("汇率服务返回错误：{error}"))?
        .json()
        .await
        .map_err(|error| format!("汇率响应无法解析：{error}"))?;
    payload
        .get("rates")
        .and_then(|rates| rates.get("CNY"))
        .and_then(Value::as_f64)
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .ok_or_else(|| "汇率响应缺少 CNY 数值".to_string())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivity {
    active: bool,
    state: &'static str,
    source: Option<String>,
    sources: Vec<String>,
    detail: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    kind: Option<String>,
    label: String,
    used_percent: Option<f64>,
    remaining_percent: Option<f64>,
    window_minutes: Option<i64>,
    resets_at: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCredits {
    has_credits: Option<bool>,
    unlimited: Option<bool>,
    balance: Option<String>,
}

#[derive(Clone)]
struct CodexQuotaSnapshot {
    plan: Option<String>,
    windows: Vec<QuotaWindow>,
    credits: Option<QuotaCredits>,
    observed_at: Option<String>,
    observed_epoch: f64,
}

#[derive(Clone, Default)]
struct CodexQuotaScan {
    snapshot: Option<CodexQuotaSnapshot>,
    saw_rate_limits: bool,
    parse_errors: usize,
    /// 最近一次 token_count 事件时间（秒）。codex 新版可能长期不推送 rate_limits,
    /// 用这个区分“真的没在用”和“在用但未推送额度”。
    recent_activity_epoch: f64,
}

static CODEX_QUOTA_CACHE: OnceLock<Mutex<Option<(PathBuf, Instant, CodexQuotaScan)>>> = OnceLock::new();

fn codex_quota(codex_home: &Path) -> CodexQuotaScan {
    let cache = CODEX_QUOTA_CACHE.get_or_init(|| Mutex::new(None));
    if let Some((path, updated, snapshot)) = cache.lock().unwrap().as_ref() {
        if path == codex_home && updated.elapsed() < Duration::from_secs(60) {
            return snapshot.clone();
        }
    }
    let snapshot = read_codex_quota_from(&codex_home.join("sessions"), epoch_seconds());
    *cache.lock().unwrap() = Some((codex_home.to_path_buf(), Instant::now(), snapshot.clone()));
    snapshot
}

fn epoch_seconds() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_secs_f64()).unwrap_or(0.0)
}

fn read_codex_quota_from(sessions_dir: &Path, now: f64) -> CodexQuotaScan {
    let mut scan = CodexQuotaScan::default();
    // 按文件 mtime 倒序扫描:rate_limits 快照只出现在含 token_count 事件的
    // 文件里,且观察时间与文件新旧强相关。倒序扫到第一个携带快照的文件即可
    // 提前返回(它就是最近一次有效额度),避免全量遍历 179 个文件。
    let mut files = codex_session_files(sessions_dir);
    files.sort_by_key(|path| {
        std::cmp::Reverse(
            fs::metadata(path)
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
        )
    });
    for file in files {
        let file_scan = read_codex_quota_file(&file, now);
        scan.saw_rate_limits |= file_scan.saw_rate_limits;
        scan.parse_errors += file_scan.parse_errors;
        // 记录最近一次 token_count 活动（无论是否携带额度）。
        scan.recent_activity_epoch = scan.recent_activity_epoch.max(file_scan.recent_activity_epoch);
        if let Some(candidate) = file_scan.snapshot {
            scan.snapshot = Some(candidate);
            break;
        }
    }
    scan
}

fn codex_session_files(directory: &Path) -> Vec<PathBuf> {
    let mut pending = vec![directory.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() { pending.push(path); }
            else if path.extension().is_some_and(|value| value == "jsonl") { files.push(path); }
        }
    }
    files
}

fn read_codex_quota_file(path: &Path, now: f64) -> CodexQuotaScan {
    let Ok(raw) = read_file_tail(path, 2 * 1024 * 1024) else { return CodexQuotaScan::default() };
    let mut scan = CodexQuotaScan::default();
    for line in raw.lines() {
        let event = match serde_json::from_str::<Value>(line) {
            Ok(event) => event,
            Err(_) => { if line.contains("rate_limits") { scan.parse_errors += 1; } continue; }
        };
        if event.get("type").and_then(Value::as_str).is_some_and(|value| value != "event_msg") { continue; }
        let Some(payload) = event.get("payload") else { continue };
        if payload.get("type").and_then(Value::as_str) != Some("token_count") { continue; }
        let Some(rate_limits) = payload.get("rate_limits") else { continue };
        scan.saw_rate_limits = true;
        // 记录本次 token_count 活动时间（无论是否携带额度窗口）。
        if let Some(ts) = event.get("timestamp").and_then(Value::as_str)
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.timestamp_millis() as f64 / 1000.0) {
            scan.recent_activity_epoch = scan.recent_activity_epoch.max(ts);
        }
        let Some(limits) = rate_limits.as_object() else { continue };
        let has_window_payload = ["primary", "secondary"].into_iter().any(|slot| limits.get(slot).is_some_and(Value::is_object));
        if !has_window_payload { continue; }
        let mut windows = Vec::new();
        for slot in ["primary", "secondary"] {
            let Some(window) = parse_codex_window(slot, limits.get(slot), now) else { continue };
            windows.push(window);
        }
        if windows.is_empty() { continue; }
        let observed_at = event.get("timestamp").and_then(Value::as_str).map(str::to_owned);
        let observed_epoch = observed_at.as_deref().and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.timestamp_millis() as f64 / 1000.0).unwrap_or(0.0);
        let credits = limits.get("credits").and_then(Value::as_object).map(|credits| QuotaCredits {
            has_credits: credits.get("has_credits").and_then(Value::as_bool),
            unlimited: credits.get("unlimited").and_then(Value::as_bool),
            balance: credits.get("balance").and_then(|value| value.as_str().map(str::to_owned).or_else(|| value.as_f64().map(|number| number.to_string()))),
        });
        let candidate = CodexQuotaSnapshot {
            plan: codex_plan(limits.get("plan_type").and_then(Value::as_str)),
            windows,
            credits,
            observed_at,
            observed_epoch,
        };
        if scan.snapshot.as_ref().is_none_or(|current| candidate.observed_epoch >= current.observed_epoch) {
            scan.snapshot = Some(candidate);
        }
    }
    scan
}

fn parse_codex_window(kind: &str, value: Option<&Value>, now: f64) -> Option<QuotaWindow> {
    let value = value?.as_object()?;
    let used = value.get("used_percent")?.as_f64()?.clamp(0.0, 100.0);
    let minutes = value.get("window_minutes")?.as_i64()?;
    if minutes <= 0 { return None; }
    let label = codex_window_label(minutes);
    let reset_seconds = value.get("resets_at").and_then(Value::as_f64).filter(|value| *value > 0.0)
        .or_else(|| value.get("resets_in_seconds").and_then(Value::as_f64).filter(|value| *value >= 0.0).map(|value| now + value));
    Some(QuotaWindow {
        kind: Some(kind.into()),
        label,
        used_percent: Some(used),
        remaining_percent: Some(100.0 - used),
        window_minutes: Some(minutes),
        resets_at: reset_seconds.map(|value| (value * 1000.0).max(0.0) as u64),
    })
}

fn codex_window_label(minutes: i64) -> String {
    match minutes {
        300 => "5 小时".into(),
        10080 => "7 天".into(),
        value if value % 1440 == 0 => format!("{} 天", value / 1440),
        value if value % 60 == 0 => format!("{} 小时", value / 60),
        value => format!("{value} 分钟"),
    }
}

fn codex_plan(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let mut chars = value.chars();
    Some(chars.next()?.to_uppercase().collect::<String>() + chars.as_str())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaAccount {
    provider: &'static str,
    name: &'static str,
    plan: String,
    status: &'static str,
    consuming: bool,
    windows: Vec<QuotaWindow>,
    credits: Option<QuotaCredits>,
    observed_at: Option<String>,
    source: Option<&'static str>,
    detail: Option<String>,
    /// §16 订阅洞察：回本率、API 折算价值、预测耗尽、估算模式。
    insight: Option<SubscriptionInsight>,
}

/// §16 订阅洞察（个性化回本/耗尽可视化数据）。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInsight {
    /// 订阅月付价格（按 §15 套餐表）。
    subscription_price: Option<f64>,
    /// 本地 token 按 API 标价折算的等值金额（本订阅周期内）。
    api_value: Option<f64>,
    /// 回本率 = api_value / subscription_price × 100；>100 表示已回本。
    roi_percent: Option<f64>,
    /// 预测耗尽时间戳（ms）；无真实额度时按近 7 日消耗斜率外推。
    projected_exhaustion_at: Option<u64>,
    /// 估算模式：true=额度为估算(真实来源不可得)，false=真实额度。
    estimated: bool,
    /// 折算币种（USD/CNY）。
    currency: &'static str,
}

#[tauri::command]
pub async fn get_quota_status(app: AppHandle) -> Vec<QuotaAccount> {
    let (kimi_dir, codex_home, dsh_home) = {
        let ctx = app.state::<AppCtx>();
        (ctx.kimi_code_dir.clone(), ctx.codex_home.clone(), ctx.dsh_home.clone())
    };
    let codex_scan = tauri::async_runtime::spawn_blocking(move || codex_quota(&codex_home)).await.unwrap_or_default();
    let ctx = app.state::<AppCtx>();
    let codex_auth = ctx.codex_home.join("auth.json");
    let codex_subscription = codex_chatgpt_login(&codex_auth);
    let kimi_connected = ctx.kimi_code_dir.join("config.toml").exists()
        || ctx.kimi_code_dir.join("credentials.json").exists();
    let claude_connected = ctx.home_dir.join(".claude").join(".credentials.json").exists()
        || ctx.home_dir.join(".claude.json").exists();
    let workbuddy_connected = ctx.home_dir.join(".workbuddy").join("workbuddy.db").exists();
    let dsh_connected = dsh_home.join(".credentials.yaml").exists()
        || std::env::var_os("DEEPSEEK_API_KEY").is_some();
    // 双保险双验证：① wham/usage（官方实时额度,经系统代理）优先；② codex 会话 jsonl 离线兜底。
    // 两路都成功时交叉校验：不一致以 wham 为准（jsonl 可能长期不推送 rate_limits）。
    let wham_quota = fetch_codex_wham_usage(&codex_auth).await;
    let codex_account = match wham_quota {
        Ok(wham) => {
            // wham 有实时额度 → 用 wham；jsonl 快照作为补充交叉验证（不一致仅标记,不覆盖实时值）。
            let jsonl = codex_scan.snapshot.as_ref();
            let cross_checked = jsonl.map(|s| {
                s.windows.iter().find(|w| w.remaining_percent.is_some())
                    .map(|w| (100.0 - wham.used_percent.unwrap_or(0.0)) - w.remaining_percent.unwrap_or(0.0))
                    .filter(|diff| diff.abs() > 15.0)
            }).flatten();
            QuotaAccount {
                provider: "codex",
                name: "ChatGPT / Codex",
                plan: "Codex 订阅".into(),
                status: if wham.limit_reached.unwrap_or(false) { "stale" } else { "available" },
                consuming: false,
                windows: wham.to_windows(),
                credits: None,
                observed_at: Some(now_rfc3339()),
                source: Some("wham-usage"),
                detail: Some(if wham.limit_reached.unwrap_or(false) {
                    format!("官方额度已用尽（{}）；下次重置约 {} 后", wham.plan_type.as_deref().unwrap_or("ChatGPT 订阅"), wham.reset_in_human())
                } else if cross_checked.is_some() {
                    "已从官方额度接口更新（本地会话快照有偏差,以官方为准）".into()
                } else {
                    "已从官方额度接口实时更新".into()
                }),
                insight: None,
            }
        }
        Err(wham_error) => {
            // 官方额度不可达（代理未运行/网络失败/未用订阅登录）。明确提示原因,
            // 不再静默回退到过期 jsonl（避免继续显示误导性的旧 6%）。
            let mut account = codex_account(codex_scan, codex_subscription, epoch_seconds());
            if let Some(message) = &mut account.detail {
                *message = format!("{message}；官方实时额度不可达：{wham_error}");
            } else {
                account.detail = Some(format!("官方实时额度不可达：{wham_error}"));
            }
            account
        }
    };
    let mut accounts = vec![
        codex_account,
        local_account("kimi", "Kimi Code", kimi_connected, "Kimi Code", "未检测到 Kimi Code 本地登录"),
        local_account("claude", "Claude Code", claude_connected, "Claude 订阅", "未检测到 Claude Code 本地登录"),
        local_account("workbuddy", "WorkBuddy", workbuddy_connected, "WorkBuddy", "未检测到 WorkBuddy 本地数据"),
        local_account("deepseek", "DeepSeek Harness", dsh_connected, "DeepSeek API", "未检测到 DSH 本地凭据"),
    ];
    if kimi_connected { accounts[1] = fetch_kimi_quota(&kimi_dir).await; }
    if dsh_connected { accounts[4] = fetch_deepseek_balance(&dsh_home).await; }
    let activity = get_agent_activity(app.clone());
    let overrides = app.state::<AppCtx>().settings.lock().unwrap_or_else(|e| e.into_inner()).plan_overrides.clone();
    for account in &mut accounts {
        // 套餐名手动覆盖：用户指定的档位优先于自动检测（codex plan_type 常为 null、Kimi 依赖登录态）。
        if let Some(plan) = overrides.get(account.provider).filter(|v| !v.is_empty()) {
            account.plan = format!("{} · {}", account.name, plan);
        }
        account.consuming = account_is_consuming(account.provider, &activity.sources);
        // §16 订阅洞察：回本率 + 预测耗尽（本地计算）。
        account.insight = subscription_insight(&app, account);
    }
    accounts
}

/// §16：按 provider 计算订阅洞察。
/// - 回本率 = 本周期(近 30 日)该 provider 相关 source 的 API 折算价值 ÷ 订阅月付 × 100。
/// - 预测耗尽：有真实额度窗口(remaining>0)时按近 7 日消耗斜率外推；无真实额度时标估算模式。
fn subscription_insight(app: &AppHandle, account: &QuotaAccount) -> Option<SubscriptionInsight> {
    // provider → (来源前缀, 订阅计划查找键)。
    let source_prefix: &str = match account.provider {
        "codex" => "codex",
        "kimi" => "kimi",
        "claude" => "claude",
        "workbuddy" => "workbuddy",
        "zcode" => "zcode",
        _ => return None,
    };
    let ctx = app.state::<AppCtx>();
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|v| v.as_millis() as u64).unwrap_or(0);
    // 订阅档位手动覆盖：在借用 repository 之前读取（ctx.settings 与 ctx.usage 不可同时可变借用）。
    let overridden_plan = ctx.settings.lock().unwrap_or_else(|e| e.into_inner())
        .plan_overrides.get(account.provider).cloned().unwrap_or_default();
    // 30 天前(UTC)起的所有 bucket。
    let start = chrono::Utc::now() - chrono::Duration::days(30);
    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let end_str = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let repository = ctx.usage.lock().unwrap_or_else(|e| e.into_inner());
    let buckets = repository.buckets_between(&start_str, &end_str).ok()?;
    let relevant: Vec<_> = buckets.iter().filter(|b| b.source.to_lowercase().starts_with(source_prefix)).collect();
    if relevant.is_empty() { return None; }
    let mut api_value = 0.0_f64;
    let mut currency = "USD";
    let mut priced = 0usize;
    for bucket in &relevant {
        if let Some(cost) = metera_core::pricing::estimate_cost(bucket) {
            api_value += cost;
            priced += 1;
            // 币种：以多数 bucket 的币种为准。
            let entry_currency = bucket_currency(bucket);
            if entry_currency == "CNY" { currency = "CNY"; }
        }
    }
    // 定价覆盖率过低(<50%)时回本率不可靠 → 标 None。
    let coverage = if relevant.is_empty() { 0.0 } else { priced as f64 / relevant.len() as f64 };
    if coverage < 0.5 { api_value = 0.0; }

    // 订阅月付：① 优先用户手动指定的档位（plan_overrides，自动检测不可靠时最准确）；
    // ② 其次按账号实际套餐名精确匹配 plans 表（如 "ChatGPT Plus"、"Allegretto"）；
    // ③ 最后回退到该 provider 第一个非零档位（避免 0 元 Free 档）。
    let provider_key = match account.provider {
        "codex" => "chatgpt",
        "claude" => "claude",
        "kimi" => "kimi",
        _ => account.provider,
    };
    let account_plan = if !overridden_plan.is_empty() { overridden_plan.to_lowercase() } else { account.plan.to_lowercase() };
    let subscription_price = metera_core::pricing::plan_entries().iter()
        .filter(|p| p.provider == provider_key && p.price_monthly > 0.0)
        .find(|p| {
            // 精确/包含匹配：账号 plan 含套餐名（如 "ChatGPT Plus" 含 "plus"）。
            let plan = p.plan.to_lowercase();
            account_plan.contains(&plan) || plan.contains(&account_plan)
        })
        .map(|p| p.price_monthly)
        .or_else(|| metera_core::pricing::plan_entries().iter()
            .filter(|p| p.provider == provider_key && p.price_monthly > 0.0)
            .min_by_key(|p| p.tier)
            .map(|p| p.price_monthly));
    let roi_percent = match (subscription_price, api_value) {
        (Some(price), value) if price > 0.0 => Some(value / price * 100.0),
        _ => None,
    };

    // 预测耗尽：近 7 日消耗斜率 → 每日消耗；有真实额度窗口则外推剩余量耗尽。
    let week_buckets: Vec<_> = buckets.iter().filter(|b| b.source.to_lowercase().starts_with(source_prefix)).collect();
    let week_tokens: i64 = week_buckets.iter().map(|b| b.total_tokens).sum();
    let projected_exhaustion_at = if week_tokens > 0 {
        let per_day_tokens = week_tokens as f64 / 7.0;
        let remaining_percent = account.windows.iter().find(|w| w.remaining_percent.is_some()).and_then(|w| w.remaining_percent);
        if let Some(remaining) = remaining_percent {
            // 用第一个含剩余百分比的窗口做估算：剩余 token ≈ 周用量×(remaining/100)。
            let remaining_tokens = week_tokens as f64 * remaining / 100.0;
            if remaining_tokens > 0.0 && per_day_tokens > 0.0 {
                let days_left = remaining_tokens / per_day_tokens;
                let ts = now_ms as f64 + days_left * 86_400_000.0;
                Some(ts.max(now_ms as f64) as u64)
            } else { None }
        } else { None }
    } else { None };
    let estimated = account.windows.is_empty() || account.windows.iter().all(|w| w.remaining_percent.is_none());
    Some(SubscriptionInsight {
        subscription_price,
        api_value: if api_value > 0.0 { Some(api_value) } else { None },
        roi_percent,
        projected_exhaustion_at,
        estimated,
        currency,
    })
}

/// bucket 的币种判定：DeepSeek/Kimi/GLM 为 CNY，其余 USD。
fn bucket_currency(bucket: &metera_core::usage::UsageBucket) -> &'static str {
    match bucket.source.to_ascii_lowercase().as_str() {
        "reasonix" | "kimi-code" | "zcode" | "workbuddy" => "CNY",
        _ => "USD",
    }
}

fn codex_chatgpt_login(auth_path: &Path) -> bool {
    fs::read_to_string(auth_path).ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .is_some_and(|value| {
            value.get("auth_mode").and_then(Value::as_str).is_some_and(|mode| mode.eq_ignore_ascii_case("chatgpt"))
                || value.get("tokens").is_some_and(Value::is_object)
        })
}

fn codex_account(scan: CodexQuotaScan, logged_in: bool, now: f64) -> QuotaAccount {
    if let Some(snapshot) = scan.snapshot {
        let stale = snapshot.observed_epoch <= 0.0 || now - snapshot.observed_epoch > 24.0 * 60.0 * 60.0;
        // 区分两种 stale：codex 最近仍在用（token_count 活跃）但未推送额度（新版/代理），
        // 与真的长时间没用。前者提示更准确的原因,避免误报“数据过期”。
        let active_without_quota = stale && now - scan.recent_activity_epoch <= 24.0 * 60.0 * 60.0;
        return QuotaAccount {
            provider: "codex",
            name: "ChatGPT / Codex",
            plan: snapshot.plan.map(|plan| format!("ChatGPT {plan}")).unwrap_or_else(|| "Codex 订阅".into()),
            status: if stale { "stale" } else { "available" },
            consuming: false,
            windows: snapshot.windows,
            credits: snapshot.credits,
            observed_at: snapshot.observed_at,
            source: Some("codex-session"),
            detail: Some(if stale {
                if active_without_quota {
                    "Codex 最近在运行但未推送额度窗口（可能经 API 代理）；档位由订阅设置指定".into()
                } else {
                    "额度数据已有一段时间未更新".into()
                }
            } else {
                "已读取 Codex 最近一次有效额度".into()
            }),
            insight: None,
        };
    }
    let (status, detail) = if !logged_in {
        ("disconnected", "未检测到 ChatGPT 订阅登录；API Key 不等同于订阅额度")
    } else if scan.saw_rate_limits && scan.parse_errors > 0 {
        ("parse_error", "检测到额度事件，但数据无法解析")
    } else {
        ("unavailable", "已登录，但 Codex 暂未返回额度窗口")
    };
    QuotaAccount {
        provider: "codex", name: "ChatGPT / Codex", plan: if logged_in { "ChatGPT 订阅".into() } else { "尚未绑定".into() },
        status, consuming: false, windows: Vec::new(), credits: None, observed_at: None, source: Some("codex-session"), detail: Some(detail.into()), insight: None,
    }
}

/// wham/usage 官方额度（双保险第一路,权威实时值）。
/// 参考成熟开源实现 xiaotianxt/cx（MIT）：`GET https://chatgpt.com/backend-api/wham/usage`，
/// 请求头 `Authorization: Bearer <access_token>` + `ChatGPT-Account-ID`（均来自 ~/.codex/auth.json），
/// 走系统代理（chatgpt.com 需代理可达）。
struct CodexWhamUsage {
    used_percent: Option<f64>,
    reset_at: Option<u64>,
    window_seconds: Option<u64>,
    plan_type: Option<String>,
    limit_reached: Option<bool>,
}

impl CodexWhamUsage {
    fn to_windows(&self) -> Vec<QuotaWindow> {
        let mut windows = Vec::new();
        if let Some(used) = self.used_percent {
            windows.push(QuotaWindow {
                kind: Some("primary".into()),
                label: "7 天额度".into(),
                used_percent: Some(used),
                remaining_percent: Some((100.0 - used).clamp(0.0, 100.0)),
                window_minutes: self.window_seconds.map(|s| (s / 60) as i64),
                // wham 接口 reset_at 是 Unix 秒；QuotaWindow.resets_at 约定毫秒（与 jsonl 路径一致）。
                resets_at: self.reset_at.map(|v| v * 1000),
            });
        }
        windows
    }
    fn reset_in_human(&self) -> String {
        let Some(reset) = self.reset_at else { return "未知".into() };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|v| v.as_secs() as u64).unwrap_or(0);
        let secs = reset.saturating_sub(now);
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        if hours > 0 { format!("{hours}h {mins}m") } else { format!("{mins}m") }
    }
}

async fn fetch_codex_wham_usage(auth_path: &Path) -> Result<CodexWhamUsage, String> {
    let raw = fs::read_to_string(auth_path).map_err(|_| "未找到 Codex auth.json".to_string())?;
    let auth: Value = serde_json::from_str(&raw).map_err(|_| "auth.json 无法解析".to_string())?;
    if auth.get("auth_mode").and_then(Value::as_str) != Some("chatgpt") {
        return Err("Codex 未使用 ChatGPT 订阅登录（auth_mode 非 chatgpt）".into());
    }
    let tokens = auth.get("tokens").ok_or("auth.json 缺少 tokens")?;
    let access_token = tokens.get("access_token").and_then(Value::as_str).ok_or("auth.json 缺少 access_token")?;
    let account_id = tokens.get("account_id").and_then(Value::as_str).unwrap_or("");
    // 系统代理（chatgpt.com 需代理；reqwest system-proxy 特性已启用）。
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("codex-cli")
        .build()
        .map_err(|error| format!("HTTP 客户端构建失败：{error}"))?;
    let mut request = client.get("https://chatgpt.com/backend-api/wham/usage")
        .bearer_auth(access_token);
    if !account_id.is_empty() {
        request = request.header("ChatGPT-Account-ID", account_id);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            // 网络层失败（多为 chatgpt.com 需代理而代理未运行）。返回带原因的失败,
            // 调用方据此提示“官方额度不可达”,避免静默回退到过期 jsonl 快照。
            let is_proxy = error.is_connect()
                || error.to_string().contains("proxy")
                || error.to_string().contains("timed out");
            return Err(if is_proxy {
                "无法连接官方额度接口（代理不可用？请确认 Clash 等代理已运行）"
            } else {
                "无法连接官方额度接口（网络错误）"
            }.into());
        }
    };
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("官方额度接口返回 {status}: {}", body.chars().take(120).collect::<String>()));
    }
    let payload: Value = response.json().await.map_err(|error| format!("官方额度响应无法解析：{error}"))?;
    let rl = payload.get("rate_limit").ok_or("官方额度响应缺少 rate_limit")?;
    let primary = rl.get("primary_window").ok_or("官方额度响应缺少 primary_window")?;
    let used_percent = primary.get("used_percent").and_then(Value::as_f64)
        .or_else(|| primary.get("used_percent").and_then(Value::as_i64).map(|v| v as f64));
    let reset_at = primary.get("reset_at").and_then(Value::as_u64)
        .or_else(|| primary.get("reset_at").and_then(Value::as_i64).map(|v| v as u64));
    let window_seconds = primary.get("limit_window_seconds").and_then(Value::as_u64)
        .or_else(|| primary.get("window_minutes").and_then(Value::as_u64).map(|v| v * 60));
    let plan_type = payload.get("plan_type").and_then(Value::as_str).map(str::to_owned);
    let limit_reached = rl.get("limit_reached").and_then(Value::as_bool)
        .or_else(|| payload.get("rate_limit_reached_type").map(|v| !v.is_null()));
    Ok(CodexWhamUsage { used_percent, reset_at, window_seconds, plan_type, limit_reached })
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn account_is_consuming(provider: &str, sources: &[String]) -> bool {
    sources.iter().any(|source| source.to_ascii_lowercase().contains(provider))
}

async fn fetch_kimi_quota(kimi_dir: &Path) -> QuotaAccount {
    let credentials = kimi_dir.join("credentials").join("kimi-code.json");
    let raw = match fs::read_to_string(&credentials) {
        Ok(raw) => raw,
        Err(_) => return quota_error("未找到 Kimi Code OAuth 凭据"),
    };
    let mut credential: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => return quota_error("Kimi Code OAuth 凭据无法解析"),
    };
    let now_sec = SystemTime::now().duration_since(UNIX_EPOCH).map(|v| v.as_secs() as i64).unwrap_or(0);
    let expires_at = credential.get("expires_at").and_then(Value::as_i64).unwrap_or(0);
    // access_token 即将过期(<60s)或已过期:先用 refresh_token 刷新（Kimi OAuth 与 Claude
    // 同为 Anthropic 系协议:POST /v1/oauth/token, grant_type=refresh_token）。
    // 否则每次 401,套餐/额度显示会"消失"。
    let mut token = match credential.get("access_token").and_then(Value::as_str).map(str::to_owned) {
        Some(token) if now_sec < expires_at - 60 => token,
        Some(_) => {
            // Kimi OAuth 刷新：auth.kimi.com/api/oauth/token + form + client_id。
            // 成功则用新 token 继续;失败时保留"过期"提示,套餐名由 planOverrides 兜底。
            match refresh_kimi_token(&mut credential, &credentials).await {
                Ok(token) => token,
                Err(_) => return quota_error("登录令牌已过期；Kimi Code 下一次请求后会自动刷新"),
            }
        }
        None => return quota_error("未找到 Kimi Code OAuth access_token"),
    };
    if token.is_empty() {
        match credential.get("access_token").and_then(Value::as_str).map(str::to_owned) {
            Some(value) => token = value,
            None => return quota_error("未找到 Kimi Code OAuth access_token"),
        }
    }
    let response = reqwest::Client::new()
        .get("https://api.kimi.com/coding/v1/usages")
        .bearer_auth(&token)
        .timeout(Duration::from_secs(8))
        .send().await;
    let response = match response {
        Ok(response) if response.status().is_success() => response,
        // 刷新后仍 401:令牌彻底失效,交由 Kimi CLI 下次请求自动重新登录。
        Ok(response) if response.status().as_u16() == 401 => return quota_error("登录令牌已过期且刷新失败；Kimi Code 下一次请求后会自动重新登录"),
        Ok(response) => return quota_error(&format!("Kimi 额度服务返回 {}", response.status())),
        Err(error) => return quota_error(&format!("Kimi 额度更新失败：{error}")),
    };
    let payload = match response.json::<Value>().await {
        Ok(payload) => payload,
        Err(error) => return quota_error(&format!("Kimi 额度数据无法解析：{error}")),
    };
    let mut windows = Vec::new();
    if let Some(usage) = payload.get("usage") { if let Some(window) = parse_quota_window(usage, "每周") { windows.push(window); } }
    if let Some(limits) = payload.get("limits").and_then(Value::as_array) {
        for (index, item) in limits.iter().enumerate() {
            let detail = item.get("detail").unwrap_or(item);
            let label = quota_label(item, detail, index);
            if let Some(window) = parse_quota_window(detail, &label) { windows.push(window); }
        }
    }
    // 套餐名优先从 API 返回（limits[*].detail.name/title/scope 或顶层 plan/plan_name/subscription）。
    let plan = kimi_plan_from_payload(&payload)
        .unwrap_or_else(|| "Kimi Code 订阅".into());
    QuotaAccount {
        provider: "kimi", name: "Kimi Code", plan, status: if windows.is_empty() { "unavailable" } else { "available" },
        consuming: false, windows, credits: None, observed_at: None, source: Some("kimi-api"), detail: Some("刚刚从 Kimi 官方额度服务更新".into()), insight: None,
    }
}

/// 读取 DSH 凭据（进程环境 > $DSH_HOME/.credentials.yaml）。文件是严格 `KEY: value`
/// 映射，用极简逐行解析（不引入 YAML 库）。key 只在本函数内使用：
/// 绝不写入日志、错误信息或回传前端（§14.4 纪律同 SMTP 授权码）。
fn dsh_api_key(dsh_home: &Path) -> Option<String> {
    if let Some(value) = std::env::var("DEEPSEEK_API_KEY").ok().filter(|value| !value.trim().is_empty()) {
        return Some(value.trim().to_string());
    }
    let raw = fs::read_to_string(dsh_home.join(".credentials.yaml")).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.trim() == "DEEPSEEK_API_KEY" {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() { return Some(value.to_string()); }
        }
    }
    None
}

/// DeepSeek 错误账户（错误信息只描述网络/解析层，绝不包含 key）。
fn deepseek_error(message: &str) -> QuotaAccount {
    QuotaAccount {
        provider: "deepseek", name: "DeepSeek Harness", plan: "DeepSeek API".into(),
        status: "error", consuming: false, windows: Vec::new(), credits: None,
        observed_at: Some(now_rfc3339()), source: Some("deepseek-balance"),
        detail: Some(message.into()), insight: None,
    }
}

/// DeepSeek 余额账户：GET api.deepseek.com/user/balance（直连同 kimi 流程、15s 超时）。
/// 余额无周期窗口 → 走 credits.balance；无订阅套餐 → 无回本洞察（subscription_insight 对
/// 未知 provider 返回 None，无需改动）。
async fn fetch_deepseek_balance(dsh_home: &Path) -> QuotaAccount {
    let Some(key) = dsh_api_key(dsh_home) else {
        return deepseek_error("未找到 DSH 凭据（DEEPSEEK_API_KEY）");
    };
    let response = reqwest::Client::new()
        .get("https://api.deepseek.com/user/balance")
        .bearer_auth(&key)
        .timeout(Duration::from_secs(15))
        .send().await;
    let response = match response {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => return deepseek_error(&format!("DeepSeek 余额服务返回 {}", response.status())),
        Err(error) => return deepseek_error(&format!("DeepSeek 余额不可达：{error}")),
    };
    let payload = match response.json::<Value>().await {
        Ok(payload) => payload,
        Err(error) => return deepseek_error(&format!("DeepSeek 余额数据无法解析：{error}")),
    };
    if payload.get("is_available").and_then(Value::as_bool) == Some(false) {
        return deepseek_error("DeepSeek 账户余额不可用");
    }
    let primary = payload.get("balance_infos").and_then(Value::as_array).and_then(|items| items.first().cloned());
    let balance = primary.as_ref()
        .and_then(|item| item.get("total_balance").and_then(Value::as_str).map(str::to_owned))
        .or_else(|| primary.as_ref().and_then(|item| item.get("total_balance").and_then(Value::as_f64).map(|value| format!("{value:.2}"))));
    let currency = primary.as_ref()
        .and_then(|item| item.get("currency").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| "CNY".into());
    let detail = match (balance.as_deref(), currency.as_str()) {
        (Some(amount), "CNY") => format!("余额 ¥{amount}（DeepSeek API）"),
        (Some(amount), _) => format!("余额 {amount} {currency}（DeepSeek API）"),
        (None, _) => "DeepSeek 余额接口已连通".into(),
    };
    QuotaAccount {
        provider: "deepseek",
        name: "DeepSeek Harness",
        plan: "DeepSeek API".into(),
        status: "connected",
        consuming: false,
        windows: Vec::new(),
        credits: Some(QuotaCredits { has_credits: Some(true), unlimited: None, balance: balance.map(|value| format!("{value} {currency}")) }),
        observed_at: Some(now_rfc3339()),
        source: Some("deepseek-balance"),
        detail: Some(detail),
        insight: None,
    }
}

/// 用 refresh_token 调用 Kimi OAuth 刷新端点,成功后回写凭据文件并返回新 access_token。
/// 协议（2026-08 实测有效）：
///   POST https://auth.kimi.com/api/oauth/token
///   Content-Type: application/x-www-form-urlencoded
///   grant_type=refresh_token&refresh_token=...&client_id=17e5f671-d194-4dfb-9706-5516cb48c098
async fn refresh_kimi_token(credential: &mut Value, credentials_path: &Path) -> Result<String, String> {
    const KIMI_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
    const KIMI_TOKEN_ENDPOINT: &str = "https://auth.kimi.com/api/oauth/token";
    let refresh_token = credential.get("refresh_token").and_then(Value::as_str).ok_or("Kimi 凭据缺少 refresh_token，无法自动刷新")?;
    let client = reqwest::Client::new();
    let response = client
        .post(KIMI_TOKEN_ENDPOINT)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", KIMI_CLIENT_ID),
        ])
        .timeout(Duration::from_secs(8))
        .send().await
        .map_err(|error| format!("Kimi 令牌刷新请求失败：{error}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Kimi 令牌刷新失败：HTTP {status} {body}"));
    }
    let body: Value = response.json().await.map_err(|error| format!("Kimi 令牌刷新响应无法解析：{error}"))?;
    let new_token = body.get("access_token").and_then(Value::as_str).ok_or("Kimi 令牌刷新响应缺少 access_token")?;
    let expires_in = body.get("expires_in").and_then(Value::as_i64).unwrap_or(900);
    let now_sec = SystemTime::now().duration_since(UNIX_EPOCH).map(|v| v.as_secs() as i64).unwrap_or(0);
    // 回写新 token 与过期时间;保留 refresh_token(可能被轮换)。
    credential["access_token"] = Value::String(new_token.to_string());
    credential["expires_at"] = Value::Number((now_sec + expires_in).into());
    if let Some(new_refresh) = body.get("refresh_token").and_then(Value::as_str) {
        credential["refresh_token"] = Value::String(new_refresh.to_string());
    }
    // 原子写回凭据文件,避免中断损坏。
    let _ = metera_core::config::atomic_write(credentials_path, &serde_json::to_vec(credential).unwrap_or_default());
    Ok(new_token.to_string())
}

/// 从 Kimi 额度 API 响应提取套餐名（脱敏：只返回名称）。
fn kimi_plan_from_payload(payload: &Value) -> Option<String> {
    for key in ["plan", "plan_name", "planName", "subscription", "subscription_plan"] {
        if let Some(name) = payload.get(key).and_then(Value::as_str).filter(|v| !v.trim().is_empty()) {
            return Some(name.trim().to_string());
        }
    }
    payload.get("limits").and_then(Value::as_array)?.iter()
        .filter_map(|item| {
            let detail = item.get("detail").unwrap_or(item);
            ["name", "title", "scope"].into_iter()
                .find_map(|key| detail.get(key).and_then(Value::as_str).filter(|v| !v.trim().is_empty()))
                .map(|v| v.trim().to_string())
        })
        .find(|name| !name.is_empty())
}

fn quota_error(message: &str) -> QuotaAccount {
    QuotaAccount { provider: "kimi", name: "Kimi Code", plan: "Kimi Code · 本地登录已检测".into(), status: "parse_error", consuming: false, windows: Vec::new(), credits: None, observed_at: None, source: Some("kimi-api"), detail: Some(message.into()), insight: None }
}

fn parse_quota_window(value: &Value, fallback_label: &str) -> Option<QuotaWindow> {
    let limit = value.get("limit").and_then(value_as_f64)?;
    if limit <= 0.0 { return None; }
    let used = value.get("used").and_then(value_as_f64)
        .or_else(|| value.get("remaining").and_then(value_as_f64).map(|remaining| limit - remaining))
        .unwrap_or(0.0).clamp(0.0, limit);
    let remaining = ((limit - used) / limit * 100.0).clamp(0.0, 100.0);
    let label = value.get("name").or_else(|| value.get("title")).and_then(Value::as_str).unwrap_or(fallback_label).to_string();
    let resets_at = ["reset_at", "resetAt", "reset_time", "resetTime"].into_iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
        .map(|date| date.timestamp_millis().max(0) as u64);
    Some(QuotaWindow { kind: None, label, used_percent: Some(100.0 - remaining), remaining_percent: Some(remaining), window_minutes: None, resets_at })
}

fn quota_label(item: &Value, detail: &Value, index: usize) -> String {
    for key in ["name", "title", "scope"] {
        if let Some(label) = item.get(key).or_else(|| detail.get(key)).and_then(Value::as_str) { return label.to_string(); }
    }
    let window = item.get("window").unwrap_or(item);
    let duration = window.get("duration").or_else(|| item.get("duration")).or_else(|| detail.get("duration")).and_then(value_as_f64).unwrap_or(0.0) as u64;
    let unit = window.get("timeUnit").or_else(|| item.get("timeUnit")).or_else(|| detail.get("timeUnit")).and_then(Value::as_str).unwrap_or("");
    if unit.contains("MINUTE") && duration % 60 == 0 { return format!("{} 小时", duration / 60); }
    if unit.contains("HOUR") { return format!("{} 小时", duration); }
    format!("额度 {}", index + 1)
}

fn value_as_f64(value: &Value) -> Option<f64> { value.as_f64().or_else(|| value.as_str()?.parse().ok()) }

fn local_account(provider: &'static str, name: &'static str, connected: bool, plan: &'static str, missing: &'static str) -> QuotaAccount {
    QuotaAccount {
        provider,
        name,
        plan: if connected { format!("{} · 本地登录已检测", plan) } else { "尚未绑定".into() },
        status: if connected { "unavailable" } else { "disconnected" },
        consuming: false,
        windows: Vec::new(),
        credits: None,
        observed_at: None,
        source: None,
        detail: Some(if connected { "等待服务返回 5 小时与每周额度".into() } else { missing.into() }),
        insight: None,
    }
}

#[tauri::command]
pub fn bind_account(app: AppHandle, provider: String) -> Result<(), String> {
    let ctx = app.state::<AppCtx>();
    let (program, args, kimi_home): (std::path::PathBuf, Vec<&str>, Option<std::path::PathBuf>) = match provider.as_str() {
        "codex" => ("codex.cmd".into(), vec!["login", "--device-auth"], None),
        "kimi" => (ctx.home_dir.join(".kimi-code/bin/kimi.exe"), vec!["login"], Some(ctx.kimi_code_dir.clone())),
        "claude" => (ctx.home_dir.join("AppData/Roaming/npm/claude.cmd"), vec!["auth", "login"], None),
        "workbuddy" => return Err("WorkBuddy 使用应用自身登录；登录后返回 Metera 重新检测即可".into()),
        _ => return Err("不支持的账号类型".into()),
    };
    let mut command = std::process::Command::new(program);
    command.args(args);
    if let Some(kimi_home) = kimi_home {
        command.env("KIMI_CODE_HOME", kimi_home);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000010).spawn().map_err(|error| format!("无法启动官方登录：{error}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    command.spawn().map_err(|error| format!("无法启动官方登录：{error}"))?;
    Ok(())
}

#[tauri::command]
pub fn get_agent_activity(app: AppHandle) -> AgentActivity {
    // 前端每 3 秒轮询一次;目录遍历 + SQLite 查询较重,缓存 2 秒避免主线程反复全量扫描。
    static LAST_ACTIVITY_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static CACHED_ACTIVITY: std::sync::Mutex<Option<AgentActivity>> = std::sync::Mutex::new(None);
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64).unwrap_or(0);
    if let Ok(cached) = CACHED_ACTIVITY.lock() {
        if now_ms.saturating_sub(LAST_ACTIVITY_AT.load(std::sync::atomic::Ordering::Relaxed)) < 2000 {
            if let Some(activity) = cached.as_ref() { return activity.clone(); }
        }
    }

    let ctx = app.state::<AppCtx>();
    let now = SystemTime::now();
    let mut active_sources = Vec::new();
    if codex_desktop_active(&ctx.codex_home.join("logs_2.sqlite"), now) {
        active_sources.push("Codex".to_string());
    }
    let file_sources = [
        ("Codex", vec![ctx.codex_home.join("sessions")]),
        ("Claude Code", vec![ctx.home_dir.join(".claude/projects")]),
        ("WorkBuddy", vec![ctx.workbuddy_projects_dir.clone()]),
        ("ZCode", vec![ctx.home_dir.join(".zcode/cli/db/db.sqlite"), ctx.home_dir.join(".zcode/cli/db/db.sqlite-wal")]),
        ("Reasonix", vec![ctx.reasonix_projects_dir.clone()]),
        ("DeepSeek Harness", vec![ctx.dsh_sessions_dir.clone()]),
    ];
    for (source, paths) in &file_sources {
        if recent_file_activity(&[(*source, paths.clone())], now, Duration::from_millis(2800)).is_some()
            && !active_sources.iter().any(|value| value == source) {
            active_sources.push((*source).to_string());
        }
    }
    let kimi = kimi_activity_state(&ctx.kimi_code_dir, now);
    if kimi.state == "error" || kimi.state == "waiting" {
        return AgentActivity { active: false, state: kimi.state, source: Some("Kimi Code".into()), sources: vec!["Kimi Code".into()], detail: kimi.detail };
    }
    if kimi.state == "active" { active_sources.push("Kimi Code".into()); }
    let source = active_sources.first().cloned();
    let activity = AgentActivity {
        active: !active_sources.is_empty(),
        state: if active_sources.is_empty() { "idle" } else { "active" },
        source,
        sources: active_sources,
        detail: None,
    };
    LAST_ACTIVITY_AT.store(now_ms, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut cached) = CACHED_ACTIVITY.lock() { *cached = Some(activity.clone()); }
    activity
}

struct KimiActivity { state: &'static str, detail: Option<String> }

fn kimi_activity_state(kimi_dir: &Path, now: SystemTime) -> KimiActivity {
    let Some(path) = latest_named_file(&kimi_dir.join("sessions"), "wire.jsonl") else { return KimiActivity { state: "idle", detail: None }; };
    let age = fs::metadata(&path).ok().and_then(|meta| meta.modified().ok())
        .and_then(|modified| now.duration_since(modified).ok()).unwrap_or(Duration::MAX);
    if age > Duration::from_secs(30 * 60) { return KimiActivity { state: "idle", detail: None }; }
    let Ok(raw) = read_file_tail(&path, 256 * 1024) else { return KimiActivity { state: "error", detail: Some("无法读取 Kimi Code 会话状态".into()) }; };
    for line in raw.lines().rev() {
        let Ok(event) = serde_json::from_str::<Value>(line) else { continue };
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        if event_type == "permission.request" || event_type == "ask_user" || event_type == "user_input.request" {
            return KimiActivity { state: "waiting", detail: Some("Kimi Code 正在等待确认".into()) };
        }
        if event_type.contains("error") {
            return KimiActivity { state: "error", detail: Some("Kimi Code 最近一次任务发生错误".into()) };
        }
        if event_type == "llm.request" { return KimiActivity { state: "active", detail: None }; }
        if event_type == "context.append_message" {
            let role = event.pointer("/message/role").and_then(Value::as_str);
            if role == Some("assistant") { return KimiActivity { state: "idle", detail: None }; }
            if role == Some("user") { return KimiActivity { state: "active", detail: None }; }
        }
        if event_type == "permission.record_approval_result" { return KimiActivity { state: "active", detail: None }; }
        if event_type == "context.append_loop_event" {
            let nested = event.pointer("/event/type").and_then(Value::as_str).unwrap_or("");
            if nested.contains("error") { return KimiActivity { state: "error", detail: Some("Kimi Code 最近一次任务发生错误".into()) }; }
            if nested == "step.end" {
                let reason = event.pointer("/event/finishReason").and_then(Value::as_str).unwrap_or("");
                return KimiActivity { state: if reason == "end_turn" { "idle" } else if reason.contains("error") { "error" } else { "active" }, detail: None };
            }
            if matches!(nested, "step.begin" | "content.part" | "tool.call" | "tool.result") { return KimiActivity { state: "active", detail: None }; }
        }
    }
    KimiActivity { state: if age <= Duration::from_secs(4) { "active" } else { "idle" }, detail: None }
}

fn latest_named_file(path: &Path, name: &str) -> Option<PathBuf> {
    let mut latest: Option<(SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(path).ok()?.flatten() {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            if let Some(candidate) = latest_named_file(&entry_path, name) {
                if let Ok(modified) = fs::metadata(&candidate).and_then(|meta| meta.modified()) {
                    if latest.as_ref().is_none_or(|(current, _)| modified > *current) { latest = Some((modified, candidate)); }
                }
            }
        } else if entry.file_name() == name {
            if let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) {
                if latest.as_ref().is_none_or(|(current, _)| modified > *current) { latest = Some((modified, entry_path)); }
            }
        }
    }
    latest.map(|(_, path)| path)
}

fn read_file_tail(path: &Path, maximum: u64) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    let skip = length.saturating_sub(maximum);
    if skip > 0 {
        // 二进制定位 + 按 UTF-8 边界调整,避免截断点多字节字符导致 read_to_string 失败。
        file.seek(SeekFrom::Start(skip))?;
        let mut bytes = Vec::new();
        file.take(maximum).read_to_end(&mut bytes)?;
        // 向前回退到最近的 UTF-8 字符边界。
        let mut cut = bytes.len();
        while cut > 0 && !is_utf8_boundary(&bytes, cut) { cut -= 1; }
        bytes.truncate(cut);
        // 丢弃可能残留的半行,从第一个换行符之后开始。
        let raw = String::from_utf8_lossy(&bytes).into_owned();
        if let Some(newline) = raw.find('\n') { return Ok(raw[newline + 1..].to_string()); }
        Ok(raw)
    } else {
        let mut raw = String::new();
        file.read_to_string(&mut raw)?;
        Ok(raw)
    }
}

/// 判断 `index` 是否为 UTF-8 字符边界:该位置的字节不是续字节(0b10xxxxxx)。
fn is_utf8_boundary(bytes: &[u8], index: usize) -> bool {
    index == 0 || bytes.get(index).is_none_or(|b| b & 0b1100_0000 != 0b1000_0000)
}

fn recent_file_activity(sources: &[(&'static str, Vec<PathBuf>)], now: SystemTime, window: Duration) -> Option<&'static str> {
    let threshold = now.checked_sub(window).unwrap_or(UNIX_EPOCH);
    sources.iter().find_map(|(source, paths)| {
        paths.iter().any(|path| latest_modified(path).is_some_and(|time| time >= threshold)).then_some(*source)
    })
}

fn codex_desktop_active(path: &Path, now: SystemTime) -> bool {
    let Ok(connection) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX) else { return false };
    let now_seconds = now.duration_since(UNIX_EPOCH).map(|value| value.as_secs() as i64).unwrap_or(0);
    let Ok(max_id) = connection.query_row("SELECT COALESCE(MAX(id), 0) FROM logs", [], |row| row.get::<_, i64>(0)) else { return false };
    if codex_turn_active(&connection, max_id) == Some(true) {
        return true;
    }
    let lower_id = max_id.saturating_sub(20_000);
    let mut statement = match connection.prepare(
        "SELECT ts, target, feedback_log_body FROM logs \
         WHERE id > ?1 AND (target = 'codex_api::sse::responses' OR target LIKE 'codex_core::tools%') \
         ORDER BY id DESC LIMIT 512"
    ) { Ok(statement) => statement, Err(_) => return false };
    let rows = match statement.query_map([lower_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))) {
        Ok(rows) => rows,
        Err(_) => return false,
    };
    let mut latest_sse = None;
    let mut completed_tools = HashSet::new();
    let mut started_tools = Vec::new();
    for row in rows.flatten() {
        let Some(body) = row.2 else { continue };
        if row.1 == "codex_api::sse::responses" {
            latest_sse.get_or_insert((row.0, body.clone()));
            if let Some(call_id) = custom_tool_call_id(&body) { started_tools.push((row.0, call_id)); }
        } else if let Some(call_id) = completed_tool_call_id(&body) {
            completed_tools.insert(call_id);
        }
    }
    if let Some((timestamp, body)) = latest_sse {
        let terminal = body.contains("\"type\":\"response.completed\"")
            || body.contains("\"type\":\"response.failed\"")
            || body.contains("\"type\":\"response.incomplete\"");
        if !terminal && timestamp >= now_seconds.saturating_sub(5) { return true; }
    }
    started_tools.into_iter().any(|(timestamp, call_id)| {
        timestamp >= now_seconds.saturating_sub(30 * 60) && !completed_tools.contains(&call_id)
    })
}

fn codex_turn_active(connection: &Connection, max_id: i64) -> Option<bool> {
    let lower_id = max_id.saturating_sub(200_000);
    let mut statement = connection.prepare(
        "SELECT feedback_log_body FROM logs \
         WHERE id > ?1 AND target = 'codex_app_server::outgoing_message' \
         AND (feedback_log_body LIKE 'app-server event: turn/started%' \
              OR feedback_log_body LIKE 'app-server event: turn/completed%') \
         ORDER BY id ASC LIMIT 2048"
    ).ok()?;
    let rows = statement.query_map([lower_id], |row| row.get::<_, String>(0)).ok()?;
    let mut active_turns = 0_u32;
    let mut observed = false;
    for body in rows.flatten() {
        if body.starts_with("app-server event: turn/started") {
            active_turns = active_turns.saturating_add(1);
            observed = true;
        } else if body.starts_with("app-server event: turn/completed") {
            active_turns = active_turns.saturating_sub(1);
            observed = true;
        }
    }
    observed.then_some(active_turns > 0)
}

fn custom_tool_call_id(body: &str) -> Option<String> {
    let raw = body.strip_prefix("SSE event: ")?;
    let event = serde_json::from_str::<Value>(raw).ok()?;
    (event.get("type")?.as_str()? == "response.output_item.done").then_some(())?;
    let item = event.get("item")?;
    (item.get("type")?.as_str()? == "custom_tool_call").then_some(())?;
    item.get("call_id")?.as_str().map(str::to_owned)
}

fn completed_tool_call_id(body: &str) -> Option<String> {
    body.contains("tool call completed").then_some(())?;
    body.split_whitespace().find_map(|part| part.strip_prefix("call_id=")).map(str::to_owned)
}

fn latest_modified(path: &Path) -> Option<SystemTime> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.is_file() { return metadata.modified().ok(); }
    let mut latest = metadata.modified().ok();
    for entry in fs::read_dir(path).ok()?.flatten() {
        if let Some(value) = latest_modified(&entry.path()) { if latest.is_none_or(|current| value > current) { latest = Some(value); } }
    }
    latest
}

#[cfg(test)]
mod activity_tests {
    use super::*;
    use tempfile::tempdir;

    fn logs_db() -> (tempfile::TempDir, PathBuf, Connection) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("logs_2.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE logs (id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, target TEXT NOT NULL, feedback_log_body TEXT);").unwrap();
        (directory, path, connection)
    }

    fn insert(connection: &Connection, ts: i64, target: &str, body: &str) {
        connection.execute("INSERT INTO logs (ts, target, feedback_log_body) VALUES (?1, ?2, ?3)", (ts, target, body)).unwrap();
    }

    #[test]
    fn codex_uses_real_sse_events_not_database_heartbeats() {
        let (_directory, path, connection) = logs_db();
        let now = SystemTime::now();
        let timestamp = now.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        insert(&connection, timestamp, "codex_app_server_transport::transport::remote_control::websocket", "periodic heartbeat");
        assert!(!codex_desktop_active(&path, now));
        insert(&connection, timestamp, "codex_api::sse::responses", r#"SSE event: {"type":"response.output_text.delta","delta":"hello"}"#);
        assert!(codex_desktop_active(&path, now));
        insert(&connection, timestamp, "codex_api::sse::responses", r#"SSE event: {"type":"response.completed","response":{"id":"response-1"}}"#);
        assert!(!codex_desktop_active(&path, now));
    }

    #[test]
    fn codex_tracks_an_outstanding_tool_call() {
        let (_directory, path, connection) = logs_db();
        let now = SystemTime::now();
        let timestamp = now.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        insert(&connection, timestamp, "codex_api::sse::responses", r#"SSE event: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call-1"}}"#);
        insert(&connection, timestamp, "codex_api::sse::responses", r#"SSE event: {"type":"response.completed","response":{"id":"response-1"}}"#);
        assert!(codex_desktop_active(&path, now));
        insert(&connection, timestamp, "codex_core::tools::parallel", "tool call completed call_id=call-1 tool_name=exec");
        assert!(!codex_desktop_active(&path, now));
    }

    #[test]
    fn codex_turn_stays_active_between_model_and_tool_rounds() {
        let (_directory, path, connection) = logs_db();
        let now = SystemTime::now();
        let timestamp = now.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        insert(&connection, timestamp, "codex_app_server::outgoing_message", "app-server event: turn/started targeted_connections=1");
        insert(&connection, timestamp, "codex_api::sse::responses", r#"SSE event: {"type":"response.completed","response":{"id":"response-1"}}"#);
        insert(&connection, timestamp, "codex_core::tools::parallel", "tool call completed call_id=call-1 tool_name=exec");
        assert!(codex_desktop_active(&path, now));
        insert(&connection, timestamp, "codex_app_server::outgoing_message", "app-server event: turn/completed targeted_connections=1");
        assert!(!codex_desktop_active(&path, now));
    }

    #[test]
    fn codex_turn_tracking_handles_parallel_tasks() {
        let (_directory, path, connection) = logs_db();
        let now = SystemTime::now();
        let timestamp = now.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        insert(&connection, timestamp, "codex_app_server::outgoing_message", "app-server event: turn/started targeted_connections=1");
        insert(&connection, timestamp, "codex_app_server::outgoing_message", "app-server event: turn/started targeted_connections=1");
        insert(&connection, timestamp, "codex_app_server::outgoing_message", "app-server event: turn/completed targeted_connections=1");
        assert!(codex_desktop_active(&path, now));
        insert(&connection, timestamp, "codex_app_server::outgoing_message", "app-server event: turn/completed targeted_connections=1");
        assert!(!codex_desktop_active(&path, now));
    }

    #[test]
    fn file_activity_includes_zcode_wal_and_workbuddy() {
        let directory = tempdir().unwrap();
        let zcode_wal = directory.path().join("db.sqlite-wal");
        fs::write(&zcode_wal, b"active").unwrap();
        let sources = [("ZCode", vec![directory.path().join("db.sqlite"), zcode_wal])];
        assert_eq!(recent_file_activity(&sources, SystemTime::now(), Duration::from_secs(2)), Some("ZCode"));
        let projects = directory.path().join("projects");
        fs::create_dir(&projects).unwrap();
        fs::write(projects.join("session.jsonl"), b"active").unwrap();
        assert_eq!(recent_file_activity(&[("WorkBuddy", vec![projects])], SystemTime::now(), Duration::from_secs(2)), Some("WorkBuddy"));
    }

    #[test]
    fn codex_quota_skips_null_events_and_reads_primary_only_window() {
        let directory = tempdir().unwrap();
        let day = directory.path().join("2026/08/02");
        fs::create_dir_all(&day).unwrap();
        fs::write(day.join("rollout-z-null.jsonl"), r#"{"timestamp":"2026-08-03T03:17:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":null,"secondary":null}}}"#).unwrap();
        fs::write(day.join("rollout-a-valid.jsonl"), r#"{"timestamp":"2026-08-03T03:16:58.589Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"plan_type":"plus","primary":{"used_percent":47.0,"window_minutes":10080,"resets_at":1800001200},"secondary":null,"credits":{"has_credits":false,"unlimited":false,"balance":"0"}}}}"#).unwrap();
        let scan = read_codex_quota_from(directory.path(), 1_800_000_000.0);
        let snapshot = scan.snapshot.unwrap();
        assert_eq!(snapshot.plan.as_deref(), Some("Plus"));
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].kind.as_deref(), Some("primary"));
        assert_eq!(snapshot.windows[0].label, "7 天");
        assert_eq!(snapshot.windows[0].remaining_percent, Some(53.0));        assert_eq!(snapshot.windows[0].resets_at, Some(1_800_001_200_000));
        assert_eq!(snapshot.credits.unwrap().balance.as_deref(), Some("0"));
    }

    #[test]
    fn codex_quota_selects_latest_valid_event_across_sessions() {
        let directory = tempdir().unwrap();
        let day = directory.path().join("2026/08/02");
        fs::create_dir_all(&day).unwrap();
        fs::write(day.join("rollout-z-new-name.jsonl"), r#"{"timestamp":"2026-08-03T03:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":91.0,"window_minutes":300,"resets_at":1799999999}}}}"#).unwrap();
        fs::write(day.join("rollout-a-old-name.jsonl"), r#"{"timestamp":"2026-08-03T04:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":12.5,"window_minutes":300,"resets_at":1800001200}}}}"#).unwrap();
        let snapshot = read_codex_quota_from(directory.path(), 1_800_000_000.0).snapshot.unwrap();
        assert_eq!(snapshot.windows[0].used_percent, Some(12.5));
        assert_eq!(snapshot.windows[0].remaining_percent, Some(87.5));
    }

    #[test]
    fn codex_quota_keeps_older_valid_event_when_newer_event_is_empty() {
        let directory = tempdir().unwrap();
        let day = directory.path().join("2026/08/02");
        fs::create_dir_all(&day).unwrap();
        fs::write(day.join("rollout.jsonl"), concat!(
            r#"{"timestamp":"2026-08-03T03:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":30.0,"window_minutes":300,"resets_at":1800001200}}}}"#, "\n",
            r#"{"timestamp":"2026-08-03T04:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":null,"secondary":null}}}"#
        )).unwrap();
        let snapshot = read_codex_quota_from(directory.path(), 1_800_000_000.0).snapshot.unwrap();
        assert_eq!(snapshot.windows[0].remaining_percent, Some(70.0));
    }

    #[test]
    fn codex_account_distinguishes_available_and_unavailable() {
        let directory = tempdir().unwrap();
        let auth = directory.path().join("auth.json");
        fs::write(&auth, r#"{"auth_mode":"chatgpt","tokens":{"access_token":"test-access-token"}}"#).unwrap();
        assert!(codex_chatgpt_login(&auth));
        let unavailable = codex_account(CodexQuotaScan::default(), true, 1_800_000_000.0);
        assert_eq!(unavailable.status, "unavailable");
    }

    #[test]
    fn kimi_wire_events_distinguish_running_waiting_and_idle() {
        let directory = tempdir().unwrap();
        let session = directory.path().join("sessions/session/agents/main");
        fs::create_dir_all(&session).unwrap();
        let wire = session.join("wire.jsonl");
        fs::write(&wire, r#"{"type":"llm.request","time":1}
"#).unwrap();
        assert_eq!(kimi_activity_state(directory.path(), SystemTime::now()).state, "active");
        fs::write(&wire, r#"{"type":"permission.request","time":2}
"#).unwrap();
        assert_eq!(kimi_activity_state(directory.path(), SystemTime::now()).state, "waiting");
        fs::write(&wire, r#"{"type":"context.append_loop_event","event":{"type":"step.end","finishReason":"end_turn"},"time":3}
"#).unwrap();
        assert_eq!(kimi_activity_state(directory.path(), SystemTime::now()).state, "idle");
    }

    #[test]
    fn kimi_quota_uses_remaining_percentage() {
        let value = serde_json::json!({
            "limit": 1000,
            "used": 240,
            "name": "每周",
            "resetAt": "2026-08-03T00:01:00+08:00"
        });
        let window = parse_quota_window(&value, "额度").unwrap();
        assert_eq!(window.remaining_percent, Some(76.0));
        assert_eq!(window.used_percent, Some(24.0));        assert_eq!(window.resets_at, Some(1_785_686_460_000));
    }

    #[test]
    fn quota_consumption_uses_the_same_agent_activity_sources() {
        let sources = vec!["Codex".to_string(), "WorkBuddy".to_string()];
        assert!(account_is_consuming("codex", &sources));
        assert!(account_is_consuming("workbuddy", &sources));
        assert!(!account_is_consuming("kimi", &sources));
    }

    #[test]
    fn wham_reset_at_seconds_are_converted_to_millis() {
        // wham/usage 接口 reset_at 是 Unix 秒（如 1786176090 ≈ 2026-08-08）；
        // QuotaWindow.resets_at 约定毫秒（前端 new Date 直接用）。转换后必须是毫秒量级,
        // 否则前端显示 1970-01-22（秒被当毫秒）。
        let wham = CodexWhamUsage {
            used_percent: Some(100.0),
            reset_at: Some(1_786_176_090),
            window_seconds: Some(604_800),
            plan_type: Some("plus".into()),
            limit_reached: Some(true),
        };
        let windows = wham.to_windows();
        assert_eq!(windows.len(), 1);
        let reset_ms = windows[0].resets_at.unwrap();
        assert!(reset_ms > 1_700_000_000_000, "必须是毫秒量级,实际 {reset_ms}");
        assert_eq!(reset_ms, 1_786_176_090_000);
        // reset_in_human 用秒计算（内部逻辑不变）。
        assert!(!wham.reset_in_human().is_empty());
    }

    #[test]
    fn plan_override_matches_exact_subscription_tier() {
        // 用户手动指定档位（plan_overrides）时,订阅价应精确匹配该档位而非第一个非零档。
        let plans = metera_core::pricing::plan_entries();
        let match_price = |provider: &str, account_plan: &str| -> Option<f64> {
            plans.iter()
                .filter(|p| p.provider == provider && p.price_monthly > 0.0)
                .find(|p| {
                    let plan = p.plan.to_lowercase();
                    let account = account_plan.to_lowercase();
                    account.contains(&plan) || plan.contains(&account)
                })
                .map(|p| p.price_monthly)
                .or_else(|| plans.iter()
                    .filter(|p| p.provider == provider && p.price_monthly > 0.0)
                    .min_by_key(|p| p.tier)
                    .map(|p| p.price_monthly))
        };
        // ChatGPT:覆盖为 Plus → 20（而非回退 Go 8）。
        assert_eq!(match_price("chatgpt", "Plus"), Some(20.0));
        assert_eq!(match_price("chatgpt", "Pro-5x"), Some(100.0));
        // Kimi:覆盖为 Allegretto → 199（而非回退日常使用 49）。
        assert_eq!(match_price("kimi", "Allegretto"), Some(199.0));
        assert_eq!(match_price("kimi", "Allegro"), Some(699.0));
        // 无覆盖、自动检测到的 plan 也能匹配（如 codex 读到的 "Plus"）。
        assert_eq!(match_price("chatgpt", "ChatGPT Plus"), Some(20.0));
        // 未知档位 → 回退第一个非零档。
        assert_eq!(match_price("kimi", "不存在的档位"), Some(49.0));
    }
}

#[tauri::command]
pub fn trigger_scan(app: AppHandle) { tauri::async_runtime::spawn(async move { services::local_scanner::run(app).await; }); }
#[tauri::command]
pub fn get_scan_state(app: AppHandle) -> SyncState { app.state::<AppCtx>().sync_state.lock().unwrap().clone() }
#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    let mut settings = app.state::<AppCtx>().settings.lock().unwrap().clone();
    // 不回传 SMTP 授权码明文;前端留空提交 = 不修改密码。
    settings.report_smtp_password.clear();
    settings
}

#[tauri::command]
pub fn set_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    {
        let ctx = app.state::<AppCtx>();
        let mut current = ctx.settings.lock().unwrap_or_else(|e| e.into_inner());
        // get_settings 已脱敏密码;前端整对象回传时密码为空 = 不修改密码,保留旧值,避免误清空。
        if settings.report_smtp_password.trim().is_empty() {
            let mut settings = settings;
            settings.report_smtp_password = current.report_smtp_password.clone();
            *current = settings;
        } else {
            *current = settings;
        }
    }
    let settings = app.state::<AppCtx>().settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    app.state::<AppCtx>().save_settings()?;
    if let Some(window) = app.get_webview_window(windows::WIDGET) { let _ = window.set_always_on_top(settings.widget_always_on_top); }
    windows::set_widget_visible(&app, settings.widget_visible);
    windows::emit_settings(&app);
    crate::tray::refresh_tooltip(&app);
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailSettingsInput {
    pub enabled: bool,
    pub email: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_password: String,
}

/// 保存每日邮件报告的 SMTP 配置（独立命令，避免整对象 set_settings 重置未随附字段）。
#[tauri::command]
pub fn set_email_settings(app: AppHandle, input: EmailSettingsInput) -> Result<(), String> {
    let email = input.email.trim().to_string();
    let smtp_host = input.smtp_host.trim().to_string();
    if email.is_empty() || smtp_host.is_empty() {
        return Err("邮箱与 SMTP 服务器不能为空".into());
    }
    // 密码留空 = 保持原密码(见下方分支);这里不做前置拒绝,否则该分支永不执行。
    {
        let ctx = app.state::<AppCtx>();
        let mut settings = ctx.settings.lock().unwrap();
        settings.daily_report_enabled = input.enabled;
        settings.report_email = email;
        settings.report_smtp_host = smtp_host;
        settings.report_smtp_port = input.smtp_port;
        // 密码留空 = 保持原密码(get_settings 已脱敏,避免误清空)。
        if !input.smtp_password.trim().is_empty() {
            settings.report_smtp_password = input.smtp_password;
        }
    }
    app.state::<AppCtx>().save_settings()?;
    windows::emit_settings(&app);
    Ok(())
}

/// 用当前输入的值发送一封测试邮件（不保存，便于先测后存）。
#[tauri::command]
pub async fn send_test_email(
    email: String,
    smtp_host: String,
    smtp_port: u16,
    smtp_password: String,
) -> Result<String, String> {
    let email = email.trim().to_string();
    let smtp_host = smtp_host.trim().to_string();
    if email.is_empty() || smtp_host.is_empty() || smtp_password.is_empty() {
        return Err("请先填写邮箱、SMTP 服务器与授权码".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        services::mailer::send_test_email(&email, &smtp_host, smtp_port, &smtp_password)
    })
    .await
    .map_err(|error| format!("发送任务失败: {error}"))?
}

/// 立即发送昨日报告（强制执行，无视「今日已发」检查；验收与调试入口，§4）。
#[tauri::command]
pub async fn send_report_now(app: AppHandle) -> Result<String, String> {
    services::report::run_report_flow(&app).await
}

#[tauri::command] pub fn get_launch_at_login() -> Result<bool, String> { services::auto_launch::get() }
#[tauri::command] pub fn set_launch_at_login(enabled: bool) -> Result<(), String> { services::auto_launch::set(enabled) }
#[tauri::command] pub fn show_dashboard(app: AppHandle) { windows::show_dashboard(&app); }
#[tauri::command] pub fn toggle_widget(app: AppHandle) { windows::toggle_widget(&app); }
#[tauri::command] pub fn close_widget(app: AppHandle) { windows::set_widget_visible(&app, false); }
#[tauri::command] pub fn collapse_widget(app: AppHandle) { windows::collapse_widget(&app); }
#[tauri::command] pub fn expand_widget(app: AppHandle) { windows::expand_widget(&app); }
#[tauri::command] pub fn set_widget_compact(app: AppHandle, compact: bool) { windows::set_widget_compact(&app, compact); }
#[tauri::command] pub fn start_widget_drag(app: AppHandle) { windows::start_widget_drag(&app); }
#[tauri::command] pub fn quit_app(app: AppHandle) { app.exit(0); }
