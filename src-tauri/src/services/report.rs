//! 每日邮件报告（阶段 2，设计定稿 v7：docs/handoff/daily-mail-phase2-proposal.md）。
//!
//! - 内容窗口恒为「昨日全天」（本地日历日 00:00–24:00，与发送时刻无关）
//! - 触发模型「每日首次存活即发送」：启动扫描完成后立即检查 + 之后每分钟检查
//! - 邮件 = 模块数组顺序拼接（M0 开场白 / M1 总览 / M2 分工具 / M6 分模型 /
//!   M7 分项目 / M3 数据彩蛋 / 固定署名），每模块一个纯函数，全部可单测
//! - 双格式渲染：纯文本（fallback）+ HTML（table 布局全内联样式、CID 品牌图标、
//!   堆叠占比条 / 24h 热力带 / 7 日竖条图），数据同源，只换渲染器
//! - 拿不到的数据显示 `--` 或整行/整节省略，绝不伪造、不用 0 冒充未知

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveTime, TimeZone, Timelike, Utc, Weekday};
use metera_core::pricing::estimate_cost;
use metera_core::usage::{UsageBucket, UsageRepository, UsageSession};
use tauri::{AppHandle, Manager};

use crate::state::AppCtx;

// ============================== 文案模板区（调整文案只改这里）==============================

const SUBJECT_PREFIX: &str = "🤖 Metera 日报 · ";
const OPENING_RECORD_HIGH: &str = "🎉 历史新高！昨天是你近 30 天最猛的一天。";
const OPENING_FALLBACK: &str = "☀️ 早上好，这是你的昨日 AI 使用日报。";
const REST_DAY_LINE: &str = "没有记录到 AI 工具使用——看来是休息的一天。";
const SIGNATURE: &str = "—— Metera";
const CACHE_SUFFIX: &str = "（缓存立大功）";
const DASH: &str = "--";
const SECTION_WIDTH: usize = 32; // 分节标题 + 分隔线的固定字符数（emoji 记 1，VS16 不计）
const LABEL_WIDTH: usize = 14; // 指标行标签列宽（显示宽度，CJK 记 2）
const VALUE_WIDTH: usize = 10; // 指标行数值列宽
const GROUP_NAME_WIDTH: usize = 15; // 分组行名称列宽
const GROUP_VALUE_WIDTH: usize = 9; // 分组行数值列宽
const SPARK_CHARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// ============================== 格式化 helpers（口径对齐前端 widget.ts / analytics.ts）==============================

fn is_wide(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x20000..=0x3FFFD)
}

fn display_width(text: &str) -> usize {
    text.chars().map(|ch| if is_wide(ch) { 2 } else { 1 }).sum()
}

fn pad_display(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(display_width(text))))
}

fn pad_display_end(text: &str, width: usize) -> String {
    format!("{}{text}", " ".repeat(width.saturating_sub(display_width(text))))
}

/// 与 analytics.ts `formatTokens` 一致：>=1e9 → x.xB，>=1e6 → x.xM，>=1e3 → x.xK，否则取整。
fn format_tokens(value: f64) -> String {
    if value >= 1e9 {
        format!("{:.1}B", value / 1e9)
    } else if value >= 1e6 {
        format!("{:.1}M", value / 1e6)
    } else if value >= 1e3 {
        format!("{:.1}K", value / 1e3)
    } else {
        format!("{}", value.round() as i64)
    }
}

/// 与 analytics.ts `formatCost` 一致。
fn format_cost(value: f64) -> String {
    format!("${value:.2}")
}

/// 与 analytics.ts `formatDuration` 一致。
fn format_duration(seconds: i64) -> String {
    let safe = seconds.max(0);
    format!("{}h {}m", safe / 3600, safe % 3600 / 60)
}

/// 与 analytics.ts `displaySource` 一致（未知名称原样显示）。
fn display_source(source: &str) -> String {
    match source.to_lowercase().as_str() {
        "codex" => "Codex",
        "claude-code" => "Claude Code",
        "kimi-code" => "Kimi Code",
        "workbuddy" => "WorkBuddy",
        "zcode" => "ZCode",
        "reasonix" => "Reasonix",
        "dsh" => "DeepSeek Harness",
        _ => source,
    }
    .to_string()
}

/// 与 widget.ts `bucketTokens` 一致。
fn bucket_tokens(bucket: &UsageBucket) -> i64 {
    bucket.input_tokens + bucket.cached_input_tokens + bucket.output_tokens + bucket.reasoning_output_tokens
}

fn section_header(emoji: &str, title: &str) -> String {
    let prefix = format!("{emoji} {title} ");
    let count = prefix.chars().filter(|ch| *ch != '\u{FE0F}').count();
    format!("{prefix}{}", "─".repeat(SECTION_WIDTH.saturating_sub(count)))
}

fn metric_row(label: &str, value: &str, annotation: Option<&str>) -> String {
    let mut line = pad_display(label, LABEL_WIDTH);
    match annotation {
        Some(note) => {
            line.push_str(&pad_display(value, VALUE_WIDTH));
            line.push_str(note);
        }
        None => line.push_str(value),
    }
    line.trim_end().to_string()
}

/// 名称超出列宽时截断加省略号，保证分组表不错位。
fn truncate_display(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_string();
    }
    let mut result = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let ch_width = if is_wide(ch) { 2 } else { 1 };
        if used + ch_width > width - 1 {
            break;
        }
        result.push(ch);
        used += ch_width;
    }
    format!("{result}…")
}

fn group_row(name: &str, tokens: i64, total: i64) -> String {
    let percent = if total > 0 { (tokens as f64 / total as f64 * 100.0).round() as i64 } else { 0 };
    format!(
        "{}{}{}",
        pad_display(&truncate_display(name, GROUP_NAME_WIDTH - 1), GROUP_NAME_WIDTH),
        pad_display(&format_tokens(tokens as f64), GROUP_VALUE_WIDTH),
        pad_display_end(&format!("{percent}%"), 3)
    )
}
// ============================== 日期 / 时区（§3.4：本地日界 → UTC ISO 字符串）==============================

fn weekday_cn(date: NaiveDate) -> &'static str {
    match date.weekday() {
        Weekday::Mon => "周一",
        Weekday::Tue => "周二",
        Weekday::Wed => "周三",
        Weekday::Thu => "周四",
        Weekday::Fri => "周五",
        Weekday::Sat => "周六",
        Weekday::Sun => "周日",
    }
}

/// 「8月3日 周日」（主题用）
fn subject_date(date: NaiveDate) -> String {
    format!("{}月{}日 {}", date.month(), date.day(), weekday_cn(date))
}

/// 「8 月 3 日·周日」（正文用）
fn spoken_date(date: NaiveDate) -> String {
    format!("{} 月 {} 日·{}", date.month(), date.day(), weekday_cn(date))
}

/// 本地某日 00:00；处理夏令时歧义/缺口（取最早解释，缺口逐分钟后移）。
pub fn local_day_start(date: NaiveDate) -> DateTime<Local> {
    let mut naive = date.and_hms_opt(0, 0, 0).unwrap();
    for _ in 0..180 {
        match Local.from_local_datetime(&naive) {
            LocalResult::Single(moment) => return moment,
            LocalResult::Ambiguous(earliest, _) => return earliest,
            LocalResult::None => naive += Duration::minutes(1),
        }
    }
    DateTime::<Utc>::from_naive_utc_and_offset(date.and_hms_opt(0, 0, 0).unwrap(), Utc).with_timezone(&Local)
}

/// 与前端 `toISOString()` 严格一致的格式（SQL 字符串比较依赖格式统一）。
fn to_utc_iso(moment: DateTime<Local>) -> String {
    moment.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// 本地日 [00:00, 次日00:00) 的 UTC ISO 边界，用于 `buckets_between` / `sessions_between`。
pub fn day_bounds_utc(date: NaiveDate) -> (String, String) {
    (to_utc_iso(local_day_start(date)), to_utc_iso(local_day_start(date + Duration::days(1))))
}

/// 解析「HH:MM」发送时刻；非法输入回退默认 08:00。
pub fn parse_send_time(raw: &str) -> NaiveTime {
    NaiveTime::parse_from_str(raw.trim(), "%H:%M")
        .unwrap_or_else(|_| NaiveTime::from_hms_opt(8, 0, 0).unwrap())
}

fn parse_timestamp(raw: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(raw).ok().map(|moment| moment.timestamp_millis())
}

fn local_date_of(raw: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|moment| moment.with_timezone(&Local).date_naive())
}

// ============================== 活跃时长：分钟槽去重（移植 widget.ts activeSeconds24hOf，窗口改为昨日日界）==============================

pub fn active_seconds_in_window(
    sessions: &[UsageSession],
    start: DateTime<Local>,
    end: DateTime<Local>,
) -> i64 {
    let slot_ms = 60_000i64;
    let start_ms = start.timestamp_millis();
    let end_ms = end.timestamp_millis();
    let slot_count = ((end_ms - start_ms) / slot_ms).max(0) as usize;
    if slot_count == 0 {
        return 0;
    }
    let mut slots = vec![0f64; slot_count];
    for session in sessions {
        let (Some(first), Some(last)) = (parse_timestamp(&session.first_message_at), parse_timestamp(&session.last_message_at)) else {
            continue;
        };
        if last < start_ms || first > end_ms {
            continue;
        }
        let duration_ms = (last - first).max(0);
        if duration_ms == 0 {
            let index = ((first - start_ms) / slot_ms).clamp(0, slot_count as i64 - 1) as usize;
            slots[index] = slots[index].max(session.active_seconds.clamp(0, 60) as f64);
            continue;
        }
        let overlap_start = first.max(start_ms);
        let overlap_end = last.min(end_ms);
        let ratio = (session.active_seconds.max(0) as f64 / (duration_ms as f64 / 1000.0)).clamp(0.0, 1.0);
        let first_slot = ((overlap_start - start_ms) / slot_ms).max(0) as usize;
        let last_slot = (((overlap_start.max(overlap_end - 1)) - start_ms) / slot_ms).min(slot_count as i64 - 1) as usize;
        for index in first_slot..=last_slot {
            let slot_start = start_ms + index as i64 * slot_ms;
            let slot_end = slot_start + slot_ms;
            let overlap_seconds = (overlap_end.min(slot_end) - overlap_start.max(slot_start)).max(0) as f64 / 1000.0;
            slots[index] = slots[index].max(overlap_seconds * ratio);
        }
    }
    slots.iter().sum::<f64>().round() as i64
}
// ============================== 报告数据与渲染（§1.5 模块化：取数 → 纯函数渲染）==============================

pub struct ReportInput {
    pub yesterday: NaiveDate,
    pub include_project_names: bool,
    pub buckets_yesterday: Vec<UsageBucket>,
    pub buckets_previous: Vec<UsageBucket>,
    pub buckets_30d: Vec<UsageBucket>,
    pub sessions_yesterday: Vec<UsageSession>,
    /// 与昨日窗口时间重叠的会话（含跨日界），仅用于活跃时长窗口裁剪统计。
    pub sessions_yesterday_overlap: Vec<UsageSession>,
}

pub struct ReportMessage {
    pub subject: String,
    pub body: String,
    pub html: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Period {
    Dawn,
    Morning,
    Afternoon,
    Evening,
}

impl Period {
    fn label(self) -> (&'static str, u32, u32) {
        match self {
            Period::Dawn => ("凌晨", 0, 6),
            Period::Morning => ("上午", 6, 12),
            Period::Afternoon => ("下午", 12, 18),
            Period::Evening => ("晚上", 18, 24),
        }
    }
}

/// 一次聚合、各模块共享的事实表（所有文案条件都来自这里，缺数据即回退/省略）。
struct Facts {
    total_yesterday: i64,
    total_previous: Option<i64>,
    prev_change_pct: Option<i64>,
    streak: u32,
    is_30d_max: bool,
    period: Option<(Period, f64)>,
    cache_rate: Option<f64>,
    active_seconds: Option<i64>,
    span_seconds: i64,
    session_count: usize,
    messages: i64,
    user_messages: i64,
    cost: f64,
    priced_buckets: usize,
    peak_hour: Option<(u32, i64)>,
    hourly_tokens: [i64; 24],
    week_tokens: Vec<i64>, // 近 7 日（含昨日）
}

impl Facts {
    fn compute(input: &ReportInput) -> Self {
        let total_yesterday = input.buckets_yesterday.iter().map(bucket_tokens).sum::<i64>();
        let total_previous = (!input.buckets_previous.is_empty())
            .then(|| input.buckets_previous.iter().map(bucket_tokens).sum::<i64>());
        let prev_change_pct = total_previous
            .filter(|previous| *previous > 0)
            .map(|previous| ((total_yesterday - previous) as f64 / previous as f64 * 100.0).round() as i64);

        let mut daily: HashMap<NaiveDate, (i64, i64)> = HashMap::new();
        for bucket in &input.buckets_30d {
            if let Some(date) = local_date_of(&bucket.bucket_start) {
                let entry = daily.entry(date).or_default();
                entry.0 += bucket_tokens(bucket);
                entry.1 += 1;
            }
        }
        let daily_30d = (0..30)
            .map(|offset| {
                let date = input.yesterday - Duration::days(29 - offset);
                let (tokens, count) = daily.get(&date).copied().unwrap_or_default();
                (date, tokens, count)
            })
            .collect::<Vec<_>>();
        let streak = daily_30d.iter().rev().take_while(|(_, _, count)| *count > 0).count() as u32;
        let max_30d = daily_30d.iter().map(|(_, tokens, _)| *tokens).max().unwrap_or(0);
        let is_30d_max = total_yesterday > 0 && total_yesterday == max_30d;

        let mut prompts = [0i64; 24];
        for session in &input.sessions_yesterday {
            for (hour, count) in session.user_prompt_hours.iter().enumerate() {
                if hour < 24 {
                    prompts[hour] += count;
                }
            }
        }
        let prompt_total = prompts.iter().sum::<i64>();
        let period = (prompt_total > 0).then(|| {
            let segments = [
                (Period::Dawn, prompts[0..6].iter().sum::<i64>()),
                (Period::Morning, prompts[6..12].iter().sum()),
                (Period::Afternoon, prompts[12..18].iter().sum()),
                (Period::Evening, prompts[18..24].iter().sum()),
            ];
            let max_value = segments.iter().map(|(_, value)| *value).max().unwrap();
            let (period, value) = segments.iter().find(|(_, value)| *value == max_value).unwrap();
            (*period, *value as f64 / prompt_total as f64)
        });

        let input_tokens = input.buckets_yesterday.iter().map(|bucket| bucket.input_tokens).sum::<i64>();
        let cached_tokens = input.buckets_yesterday.iter().map(|bucket| bucket.cached_input_tokens).sum::<i64>();
        let cache_rate = (input_tokens + cached_tokens > 0)
            .then(|| cached_tokens as f64 / (input_tokens + cached_tokens) as f64);

        let active_seconds = (!input.sessions_yesterday_overlap.is_empty()).then(|| {
            active_seconds_in_window(
                &input.sessions_yesterday_overlap,
                local_day_start(input.yesterday),
                local_day_start(input.yesterday + Duration::days(1)),
            )
        });
        let span_seconds = input.sessions_yesterday.iter().map(|session| session.duration_seconds).sum();
        let session_count = input
            .sessions_yesterday
            .iter()
            .map(|session| session.session_hash.as_str())
            .collect::<HashSet<_>>()
            .len();
        let messages = input.sessions_yesterday.iter().map(|session| session.message_count).sum();
        let user_messages = input.sessions_yesterday.iter().map(|session| session.user_message_count).sum();

        let mut cost = 0.0;
        let mut priced_buckets = 0usize;
        for bucket in &input.buckets_yesterday {
            if let Some(value) = estimate_cost(bucket) {
                cost += value;
                priced_buckets += 1;
            }
        }

        let mut hourly = [0i64; 24];
        for bucket in &input.buckets_yesterday {
            if let Ok(moment) = DateTime::parse_from_rfc3339(&bucket.bucket_start) {
                hourly[moment.with_timezone(&Local).hour() as usize] += bucket_tokens(bucket);
            }
        }
        let peak_hour = {
            let max_value = hourly.iter().copied().max().unwrap_or(0);
            (max_value > 0).then(|| {
                let hour = hourly.iter().position(|value| *value == max_value).unwrap() as u32;
                (hour, max_value)
            })
        };

        let week_tokens = daily_30d.iter().skip(23).map(|(_, tokens, _)| *tokens).collect::<Vec<_>>();

        Self {
            total_yesterday,
            total_previous,
            prev_change_pct,
            streak,
            is_30d_max,
            period,
            cache_rate,
            active_seconds,
            span_seconds,
            session_count,
            messages,
            user_messages,
            cost,
            priced_buckets,
            peak_hour,
            hourly_tokens: hourly,
            week_tokens,
        }
    }
}
/// M0 开场白：§1.3.1 规则表，按优先级取第一条命中，条件缺数据则跳过。
fn opening(facts: &Facts) -> String {
    if facts.is_30d_max {
        return OPENING_RECORD_HIGH.into();
    }
    if let Some(change) = facts.prev_change_pct {
        if change >= 50 {
            return format!("🔥 火力全开！昨天 Token 用量比前天涨了 {change}%。");
        }
        if change <= -50 {
            let remaining = (facts.total_yesterday as f64 / facts.total_previous.unwrap_or(1) as f64 * 100.0).round() as i64;
            return format!("🍃 昨天放缓了节奏，用量只有前天的 {remaining}%。");
        }
    }
    if matches!(facts.period, Some((Period::Dawn, _))) {
        return "🌙 夜猫子模式：昨天凌晨是你最忙的时段。".into();
    }
    if facts.streak >= 7 {
        return format!("💪 连续第 {} 天记录到使用，保持住！", facts.streak);
    }
    OPENING_FALLBACK.into()
}

/// M1 昨日总览。
fn overview_lines(input: &ReportInput, facts: &Facts) -> Vec<String> {
    let change = facts
        .prev_change_pct
        .map(|pct| format!("较前日 {pct:+}%"))
        .unwrap_or_else(|| format!("较前日 {DASH}"));
    vec![
        metric_row("Token 总量", &format_tokens(facts.total_yesterday as f64), Some(&change)),
        if facts.priced_buckets == 0 {
            metric_row("估算花销", DASH, None)
        } else {
            let coverage = (facts.priced_buckets as f64 / input.buckets_yesterday.len() as f64 * 100.0).round() as i64;
            metric_row("估算花销", &format_cost(facts.cost), Some(&format!("定价覆盖率 {coverage}%")))
        },
        match facts.active_seconds {
            Some(active) => metric_row("活跃时长", &format_duration(active), Some(&format!("总跨度 {}", format_duration(facts.span_seconds)))),
            None => metric_row("活跃时长", DASH, None),
        },
        metric_row(
            "会话数",
            &facts.session_count.to_string(),
            Some(&format!("消息 {} 条（你发起 {} 条）", facts.messages, facts.user_messages)),
        ),
    ]
}

/// M2/M6/M7 共用：分组排序 Top 5 + 其他（口径同 groupBySource）。
fn group_lines(buckets: &[UsageBucket], key: impl Fn(&UsageBucket) -> String, name: impl Fn(&str) -> String) -> Vec<String> {
    let mut groups: HashMap<String, i64> = HashMap::new();
    for bucket in buckets {
        let key = key(bucket);
        let key = if key.is_empty() { "unknown".into() } else { key };
        *groups.entry(key).or_default() += bucket_tokens(bucket);
    }
    if groups.is_empty() {
        return Vec::new();
    }
    let total = groups.values().sum::<i64>();
    let mut ranked = groups.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let mut lines = ranked
        .iter()
        .take(5)
        .map(|(key, tokens)| group_row(&name(key), *tokens, total))
        .collect::<Vec<_>>();
    if ranked.len() > 5 {
        let rest = ranked.iter().skip(5).map(|(_, tokens)| *tokens).sum::<i64>();
        lines.push(group_row("其他", rest, total));
    }
    lines
}

/// M3 数据彩蛋（聚合 M4/M5/M8，各占一行；条件不满足的行省略）。
fn egg_lines(facts: &Facts) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some((hour, tokens)) = facts.peak_hour {
        lines.push(metric_row(
            "峰值时段",
            &format!("{}:00–{}:00（{} tokens）", hour, hour + 1, format_tokens(tokens as f64)),
            None,
        ));
    }
    if let Some((period, share)) = facts.period {
        let (label, from, to) = period.label();
        let pct = (share * 100.0).round() as i64;
        lines.push(metric_row("最活跃时段", &format!("{label}（{from}–{to} 点）占全天 {pct}%"), None));
    }
    if let Some(rate) = facts.cache_rate {
        let pct = (rate * 100.0).round() as i64;
        let suffix = if rate >= 0.5 { CACHE_SUFFIX } else { "" };
        lines.push(metric_row("缓存命中率", &format!("{pct}%{suffix}"), None));
    }
    let mut intensity = Vec::new();
    if let Some(active) = facts.active_seconds.filter(|active| *active > 0) {
        let per_hour = facts.total_yesterday as f64 / (active as f64 / 3600.0);
        intensity.push(format!("{}/活跃小时", format_tokens(per_hour)));
    }
    if facts.priced_buckets > 0 && facts.total_yesterday > 0 {
        intensity.push(format!("{}/百万 token", format_cost(facts.cost / facts.total_yesterday as f64 * 1e6)));
    }
    if !intensity.is_empty() {
        lines.push(metric_row("强度", &intensity.join(" · "), None));
    }
    let max_week = facts.week_tokens.iter().copied().max().unwrap_or(0);
    let days_with_data = facts.week_tokens.iter().filter(|tokens| **tokens > 0).count();
    if days_with_data >= 2 && max_week > 0 {
        let bars = facts
            .week_tokens
            .iter()
            .map(|tokens| SPARK_CHARS[((*tokens as f64 / max_week as f64) * 7.0).round().min(7.0) as usize])
            .collect::<String>();
        let mean = facts.week_tokens.iter().sum::<i64>() as f64 / facts.week_tokens.len() as f64;
        let mut line = format!("{bars} 日均 {}", format_tokens(mean));
        if facts.total_yesterday == max_week && facts.total_yesterday > 0 {
            line.push_str(" · 昨日为 7 日最高 🏆");
        }
        lines.push(metric_row("近 7 日", &line, None));
    }
    lines
}

fn signature(streak: u32) -> String {
    if streak >= 2 {
        format!("—— Metera · 连续记录第 {streak} 天")
    } else {
        SIGNATURE.into()
    }
}

pub fn render(input: &ReportInput) -> ReportMessage {
    let subject = format!("{SUBJECT_PREFIX}{}", subject_date(input.yesterday));
    if input.buckets_yesterday.is_empty() {
        // §1.4 无数据日收缩版（仍发送）
        return ReportMessage {
            subject,
            body: format!("🌿 昨天（{}）{REST_DAY_LINE}\n\n{SIGNATURE}", spoken_date(input.yesterday)),
            html: render_html_rest_day(input.yesterday),
        };
    }
    let facts = Facts::compute(input);
    let html = render_html(input, &facts);
    let mut blocks = vec![
        opening(&facts),
        format!("{}\n{}", section_header("📊", "昨日总览"), overview_lines(input, &facts).join("\n")),
    ];
    let tools = group_lines(&input.buckets_yesterday, |bucket| bucket.source.clone(), |key| display_source(key));
    if !tools.is_empty() {
        blocks.push(format!("{}\n{}", section_header("🛠️", "分工具"), tools.join("\n")));
    }
    let models = group_lines(&input.buckets_yesterday, |bucket| bucket.model.clone(), |key| key.to_string());
    if !models.is_empty() {
        blocks.push(format!("{}\n{}", section_header("🤖", "分模型（Top 5）"), models.join("\n")));
    }
    // M7 分项目：渲染层尊重 include_project_names（scanner 硬编码 include_projects=true，项目名总在库中）
    let all_unknown = input.buckets_yesterday.iter().all(|bucket| bucket.project.is_empty() || bucket.project == "unknown");
    if input.include_project_names && !all_unknown {
        let projects = group_lines(&input.buckets_yesterday, |bucket| bucket.project.clone(), |key| key.to_string());
        if !projects.is_empty() {
            blocks.push(format!("{}\n{}", section_header("📁", "分项目（Top 5）"), projects.join("\n")));
        }
    }
    let eggs = egg_lines(&facts);
    if !eggs.is_empty() {
        blocks.push(format!("{}\n{}", section_header("✨", "数据彩蛋"), eggs.join("\n")));
    }
    blocks.push(signature(facts.streak));
    ReportMessage { subject, body: blocks.join("\n\n"), html }
}
// ============================== HTML 渲染器（table 布局全内联样式；与纯文本同源数据，只换渲染器）==============================
// 设计稿：artifacts/mail-html-preview.html；配色对齐 dashboard-v2 深色 token。
// 兼容约定：全 table 布局、样式全内联 + bgcolor 双写、品牌图标走 cid（mailer 内嵌）、无渐变无阴影。

const H_BG: &str = "#0d0f0e";
const H_PANEL: &str = "#141615";
const H_RECESS: &str = "#111311";
const H_LINE: &str = "#242624";
const H_TEXT: &str = "#f1f3f2";
const H_SUB: &str = "#c9d4cc";
const H_MUTED: &str = "#8d928f";
const H_SOFT: &str = "#656a67";
const H_GREEN: &str = "#4ade9b";
const H_YELLOW: &str = "#e5bc4b";
const H_FONT: &str = "font-family:'Segoe UI','Microsoft YaHei',sans-serif;";
/// 24h 热力带五档（由暗到亮）。
const HEAT_COLORS: [&str; 5] = ["#1a2a21", "#234434", "#2f5f45", "#3ec98c", "#4ade9b"];
/// 堆叠占比条分段配色。
const STACK_COLORS: [&str; 6] = ["#4ade9b", "#2e8f66", "#1f4d3a", "#7ddba8", "#3ec98c", "#234434"];

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// 模型名 → 品牌图标 CID（lobe-icons，由 mailer 编译期内嵌）。
fn model_icon(model: &str) -> Option<&'static str> {
    let lower = model.to_lowercase();
    if lower.contains("gpt") || lower.contains("openai") {
        Some("openai")
    } else if lower.contains("kimi") || lower.contains("moonshot") {
        Some("moonshot")
    } else if lower.contains("deepseek") {
        Some("deepseek")
    } else if lower.contains("qwen") {
        Some("qwen")
    } else if lower.contains("claude") {
        Some("claude")
    } else {
        None
    }
}

/// 与 group_lines 同口径的分组排序，返回结构化 Top 5 + 其他（供 HTML 渲染）。
fn ranked_groups(buckets: &[UsageBucket], key: impl Fn(&UsageBucket) -> String) -> Vec<(String, i64)> {
    let mut groups: HashMap<String, i64> = HashMap::new();
    for bucket in buckets {
        let key = key(bucket);
        let key = if key.is_empty() { "unknown".into() } else { key };
        *groups.entry(key).or_default() += bucket_tokens(bucket);
    }
    let mut ranked = groups.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    if ranked.len() > 5 {
        let rest = ranked.split_off(5);
        let rest_sum = rest.iter().map(|(_, tokens)| *tokens).sum();
        ranked.push(("其他".into(), rest_sum));
    }
    ranked
}

/// 横条轨道（填充段 + 轨道段，两单元格背景色拼接）。
fn h_track_bar(pct: i64, color: &str) -> String {
    let pct = pct.clamp(0, 100);
    let mut bar = String::from(r#"<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr>"#);
    if pct > 0 {
        bar.push_str(&format!(
            r#"<td width="{pct}%" height="8" bgcolor="{color}" style="background:{color};font-size:0;line-height:0;border-radius:4px 0 0 4px;">&nbsp;</td>"#
        ));
    }
    bar.push_str(&format!(
        r#"<td height="8" bgcolor="{H_LINE}" style="background:{H_LINE};font-size:0;line-height:0;border-radius:0 4px 4px 0;">&nbsp;</td></tr></table>"#
    ));
    bar
}

fn h_section_title(emoji: &str, title: &str) -> String {
    format!(
        r#"<tr><td style="padding:26px 32px 0 32px;{H_FONT}font-size:13px;letter-spacing:1px;color:{H_MUTED};">{emoji}&nbsp;&nbsp;{title}</td></tr>"#
    )
}

/// Hero 下的一句观察（与近 7 日日均对比）。
fn quip_hero(facts: &Facts) -> Option<String> {
    let days_with_data = facts.week_tokens.iter().filter(|tokens| **tokens > 0).count();
    if days_with_data < 2 {
        return None;
    }
    let mean = facts.week_tokens.iter().sum::<i64>() as f64 / facts.week_tokens.len() as f64;
    if mean <= 0.0 {
        return None;
    }
    let ratio = facts.total_yesterday as f64 / mean;
    let text = if ratio > 1.05 {
        format!("比近 7 日日均（{}）略高一截，稳中有升。", format_tokens(mean))
    } else if ratio < 0.95 {
        format!("低于近 7 日日均（{}），节奏放缓。", format_tokens(mean))
    } else {
        format!("与近 7 日日均（{}）基本持平。", format_tokens(mean))
    };
    Some(text)
}

fn quip_tools(ranked: &[(String, i64)]) -> Option<String> {
    let (name, tokens) = ranked.first()?;
    let total: i64 = ranked.iter().map(|(_, value)| *value).sum();
    if total <= 0 {
        return None;
    }
    let share = *tokens as f64 / total as f64;
    if share >= 0.8 {
        let assist = ranked
            .get(1)
            .filter(|(_, second)| *second as f64 / total as f64 >= 0.02)
            .map(|(second, _)| format!("，{} 负责打辅助", html_escape(second)))
            .unwrap_or_default();
        Some(format!("{} 几乎包办了全天的活{assist}。", html_escape(name)))
    } else if share >= 0.5 {
        Some(format!("{} 是昨天最主力的工具，扛了 {:.0}%。", html_escape(name), share * 100.0))
    } else {
        None
    }
}

fn quip_models(ranked: &[(String, i64)]) -> Option<String> {
    let (name, tokens) = ranked.first()?;
    let total: i64 = ranked.iter().map(|(_, value)| *value).sum();
    if total <= 0 {
        return None;
    }
    let share = *tokens as f64 / total as f64;
    (share >= 0.85).then(|| {
        format!(
            "{} 一个模型扛了 {} 成，是昨日绝对主力。",
            html_escape(name),
            (share * 10.0).round() as i64
        )
    })
}

fn quip_projects(ranked: &[(String, i64)]) -> Option<String> {
    let (name, tokens) = ranked.first()?;
    let total: i64 = ranked.iter().map(|(_, value)| *value).sum();
    if total <= 0 {
        return None;
    }
    let share = *tokens as f64 / total as f64;
    (share >= 0.5).then(|| {
        format!(
            "{} 成火力都给了 {}。",
            (share * 10.0).round() as i64,
            html_escape(name)
        )
    })
}

fn quip_week(facts: &Facts) -> Option<String> {
    let max_week = facts.week_tokens.iter().copied().max().unwrap_or(0);
    if max_week <= 0 {
        return None;
    }
    if facts.total_yesterday == max_week && facts.total_yesterday > 0 {
        return Some("昨日是近 7 日的峰值。🏆".into());
    }
    let mean = facts.week_tokens.iter().sum::<i64>() as f64 / facts.week_tokens.len() as f64;
    if mean <= 0.0 {
        return None;
    }
    let stable = facts
        .week_tokens
        .iter()
        .filter(|tokens| (**tokens as f64 - mean).abs() <= mean * 0.15)
        .count();
    (stable >= 5).then(|| format!("7 天里 {stable} 天在日均上下徘徊，发挥相当稳定。"))
}

fn h_quip(text: &str) -> String {
    format!(
        r#"<div style="{H_FONT}font-size:12px;color:{H_SOFT};padding-top:10px;">{}</div>"#,
        html_escape(text)
    )
}

/// 分工具：100% 堆叠分段条 + 色块图例。
fn h_tools_section(ranked: &[(String, i64)]) -> String {
    let total: i64 = ranked.iter().map(|(_, value)| *value).sum();
    let mut out = h_section_title("🛠️", "分工具");
    out.push_str(r#"<tr><td style="padding:12px 32px 0 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr>"#);
    let last = ranked.len().saturating_sub(1);
    for (index, (_, tokens)) in ranked.iter().enumerate() {
        let pct = if total > 0 { (*tokens as f64 / total as f64 * 100.0).round() as i64 } else { 0 };
        let color = STACK_COLORS[index % STACK_COLORS.len()];
        let radius = match (index == 0, index == last) {
            (true, true) => "border-radius:7px;",
            (true, false) => "border-radius:7px 0 0 7px;",
            (false, true) => "border-radius:0 7px 7px 0;",
            (false, false) => "",
        };
        let width = if index == last { String::new() } else { format!(r#"width="{}%""#, pct.max(1)) };
        out.push_str(&format!(
            r#"<td {width} height="14" bgcolor="{color}" style="background:{color};font-size:0;line-height:0;{radius}">&nbsp;</td>"#
        ));
    }
    out.push_str(r#"</tr></table><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0">"#);
    for (index, (name, tokens)) in ranked.iter().enumerate() {
        let pct = if total > 0 { (*tokens as f64 / total as f64 * 100.0).round() as i64 } else { 0 };
        let color = STACK_COLORS[index % STACK_COLORS.len()];
        out.push_str(&format!(
            r#"<tr><td width="20" style="padding:8px 0 0 0;"><span style="display:inline-block;width:8px;height:8px;background:{color};border-radius:2px;font-size:0;">&nbsp;</span></td><td style="{H_FONT}font-size:13px;color:{H_TEXT};padding:8px 0 0 6px;">{}</td><td align="right" style="{H_FONT}font-size:13px;color:{H_SUB};padding:8px 0 0 0;">{}&nbsp;&nbsp;<span style="color:{H_MUTED};font-size:12px;">{pct}%</span></td></tr>"#,
            html_escape(name),
            format_tokens(*tokens as f64),
        ));
    }
    out.push_str("</table>");
    if let Some(quip) = quip_tools(ranked) {
        out.push_str(&h_quip(&quip));
    }
    out.push_str("</td></tr>");
    out
}

/// 分模型：品牌图标行 + 横条。
fn h_models_section(ranked: &[(String, i64)]) -> String {
    let total: i64 = ranked.iter().map(|(_, value)| *value).sum();
    let mut out = h_section_title("🤖", "分模型（Top 5）");
    out.push_str(r#"<tr><td style="padding:10px 32px 0 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0">"#);
    for (name, tokens) in ranked {
        let pct = if total > 0 { (*tokens as f64 / total as f64 * 100.0).round() as i64 } else { 0 };
        let icon = model_icon(name)
            .map(|cid| format!(r#"<img src="cid:{cid}" width="18" height="18" alt="{cid}" style="display:block;border:0;">"#))
            .unwrap_or_default();
        out.push_str(&format!(
            r#"<tr><td width="24" style="padding:6px 0;">{icon}</td><td width="118" style="{H_FONT}font-size:13px;color:{H_TEXT};padding:6px 0 6px 8px;">{}</td><td style="padding:6px 12px;">{}</td><td width="62" align="right" style="{H_FONT}font-size:13px;color:{H_SUB};padding:6px 0;">{}</td><td width="40" align="right" style="{H_FONT}font-size:12px;color:{H_MUTED};padding:6px 0;">{pct}%</td></tr>"#,
            html_escape(&truncate_display(name, 13)),
            h_track_bar(pct, H_GREEN),
            format_tokens(*tokens as f64),
        ));
    }
    out.push_str("</table>");
    if let Some(quip) = quip_models(ranked) {
        out.push_str(&h_quip(&quip));
    }
    out.push_str("</td></tr>");
    out
}

/// 分项目：横条（无图标）。
fn h_projects_section(ranked: &[(String, i64)]) -> String {
    let total: i64 = ranked.iter().map(|(_, value)| *value).sum();
    let mut out = h_section_title("📁", "分项目（Top 5）");
    out.push_str(r#"<tr><td style="padding:10px 32px 0 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0">"#);
    for (name, tokens) in ranked {
        let pct = if total > 0 { (*tokens as f64 / total as f64 * 100.0).round() as i64 } else { 0 };
        out.push_str(&format!(
            r#"<tr><td width="142" style="{H_FONT}font-size:13px;color:{H_TEXT};padding:5px 0;">{}</td><td style="padding:5px 12px;">{}</td><td width="62" align="right" style="{H_FONT}font-size:13px;color:{H_SUB};padding:5px 0;">{}</td><td width="40" align="right" style="{H_FONT}font-size:12px;color:{H_MUTED};padding:5px 0;">{pct}%</td></tr>"#,
            html_escape(&truncate_display(name, 15)),
            h_track_bar(pct, "#7ddba8"),
            format_tokens(*tokens as f64),
        ));
    }
    out.push_str("</table>");
    if let Some(quip) = quip_projects(ranked) {
        out.push_str(&h_quip(&quip));
    }
    out.push_str("</td></tr>");
    out
}

/// 全天候节奏：24h 热力带。
fn h_heat_section(facts: &Facts) -> Option<String> {
    let max_hour = facts.hourly_tokens.iter().copied().max().unwrap_or(0);
    if max_hour <= 0 {
        return None;
    }
    let mut cells = String::new();
    for tokens in facts.hourly_tokens {
        let color = if tokens <= 0 {
            "#151b18"
        } else {
            let ratio = tokens as f64 / max_hour as f64;
            let level = if ratio < 0.2 { 0 } else if ratio < 0.45 { 1 } else if ratio < 0.7 { 2 } else if ratio < 0.9 { 3 } else { 4 };
            HEAT_COLORS[level]
        };
        cells.push_str(&format!(
            r#"<td height="26" bgcolor="{color}" style="background:{color};font-size:0;line-height:0;border-radius:3px;">&nbsp;</td>"#
        ));
    }
    let mut notes = Vec::new();
    if let Some((hour, tokens)) = facts.peak_hour {
        notes.push(format!(
            "峰值在 <b style=\"color:{H_GREEN};\">{}:00–{}:00</b>（{} tokens）",
            hour,
            hour + 1,
            format_tokens(tokens as f64)
        ));
    }
    if let Some((period, share)) = facts.period {
        let (label, from, to) = period.label();
        notes.push(format!(
            "{}（{from}–{to} 点）占全天 {:.0}%，是最勤奋的时段",
            label,
            share * 100.0
        ));
    }
    Some(format!(
        r#"<tr><td style="padding:26px 32px 0 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" bgcolor="{H_RECESS}" style="background:{H_RECESS};border:1px solid {H_LINE};border-radius:8px;"><tr><td style="padding:18px 20px;{H_FONT}"><div style="font-size:13px;letter-spacing:1px;color:{H_MUTED};padding-bottom:12px;">🕐&nbsp;&nbsp;全天候节奏</div><table role="presentation" width="100%" cellpadding="0" cellspacing="2" border="0"><tr>{cells}</tr></table><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr><td align="left" style="{H_FONT}font-size:10px;color:{H_SOFT};padding-top:6px;">0 时</td><td align="center" style="{H_FONT}font-size:10px;color:{H_SOFT};padding-top:6px;">6 时</td><td align="center" style="{H_FONT}font-size:10px;color:{H_SOFT};padding-top:6px;">12 时</td><td align="center" style="{H_FONT}font-size:10px;color:{H_SOFT};padding-top:6px;">18 时</td><td align="right" style="{H_FONT}font-size:10px;color:{H_SOFT};padding-top:6px;">24 时</td></tr></table><div style="font-size:12px;color:{H_MUTED};padding-top:10px;line-height:18px;">{}。</div></td></tr></table></td></tr>"#,
        notes.join("；")
    ))
}

/// 近 7 日竖条图（柱顶数值 + 日期轴，昨日高亮）。
fn h_week_section(input: &ReportInput, facts: &Facts) -> Option<String> {
    let max_week = facts.week_tokens.iter().copied().max().unwrap_or(0);
    let days_with_data = facts.week_tokens.iter().filter(|tokens| **tokens > 0).count();
    if days_with_data < 2 || max_week <= 0 {
        return None;
    }
    let mean = facts.week_tokens.iter().sum::<i64>() as f64 / facts.week_tokens.len() as f64;
    let mut labels = String::new();
    let mut bars = String::new();
    let mut dates = String::new();
    let last = facts.week_tokens.len() - 1;
    for (index, tokens) in facts.week_tokens.iter().enumerate() {
        let is_yesterday = index == last;
        let value_color = if is_yesterday { H_GREEN } else { H_MUTED };
        let weight = if is_yesterday { "font-weight:700;" } else { "" };
        labels.push_str(&format!(
            r#"<td align="center" style="{H_FONT}font-size:10px;color:{value_color};{weight}">{}</td>"#,
            format_tokens(*tokens as f64)
        ));
        let height = if *tokens <= 0 { 4 } else { ((*tokens as f64 / max_week as f64) * 56.0).round().max(8.0) as i64 };
        let color = if is_yesterday { H_GREEN } else { "#2c3a31" };
        bars.push_str(&format!(
            r#"<td align="center" valign="bottom" height="72" style="height:72px;"><div style="width:26px;height:{height}px;background:{color};font-size:0;line-height:0;border-radius:3px 3px 0 0;">&nbsp;</div></td>"#
        ));
        let date = input.yesterday - Duration::days((last - index) as i64);
        let date_label = if index == 0 { format!("{}/{}", date.month(), date.day()) } else { format!("{}", date.day()) };
        let date_color = if is_yesterday { H_GREEN } else { H_SOFT };
        dates.push_str(&format!(
            r#"<td align="center" style="{H_FONT}font-size:10px;color:{date_color};padding-top:6px;{weight}">{date_label}</td>"#
        ));
    }
    let mut out = format!(
        r#"<tr><td style="padding:20px 32px 0 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" bgcolor="{H_RECESS}" style="background:{H_RECESS};border:1px solid {H_LINE};border-radius:8px;"><tr><td style="padding:18px 20px;{H_FONT}"><div style="font-size:13px;letter-spacing:1px;color:{H_MUTED};padding-bottom:8px;">✨&nbsp;&nbsp;近 7 日走势 · 日均 {}</div><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr>{labels}</tr><tr>{bars}</tr><tr>{dates}</tr></table>"#,
        format_tokens(mean)
    );
    if let Some(quip) = quip_week(facts) {
        out.push_str(&h_quip(&quip));
    }
    out.push_str("</td></tr></table></td></tr>");
    Some(out)
}

/// 昨日一句话：由真实榜首数据与时段/缓存条件拼成。
fn h_closing_section(tools: &[(String, i64)], models: &[(String, i64)], facts: &Facts) -> Option<String> {
    let (tool, _) = tools.first()?;
    let (model, _) = models.first()?;
    let period_phrase = match facts.period {
        Some((Period::Dawn, _)) => "深夜爆发",
        Some((Period::Morning, _)) => "上午勤奋",
        Some((Period::Afternoon, _)) => "午后发力",
        Some((Period::Evening, _)) => "夜间鏖战",
        None => "全天在线",
    };
    let cache_phrase = match facts.cache_rate {
        Some(rate) if rate >= 0.5 => "缓存还顺手替你省了一大笔。",
        Some(_) => "没有缓存加持，全靠硬实力。",
        None => "新的一天继续。",
    };
    Some(format!(
        r#"<tr><td style="padding:26px 32px 0 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr><td style="border-top:1px solid {H_LINE};padding-top:20px;{H_FONT}font-size:14px;line-height:24px;color:{H_SUB};"><span style="color:{H_SOFT};font-size:12px;letter-spacing:1px;">昨日一句话</span><br>「{} 开路、{} 扛旗，{period_phrase}——{cache_phrase}」</td></tr></table></td></tr>"#,
        html_escape(tool),
        html_escape(model)
    ))
}

fn h_signature(streak: u32) -> String {
    let text = if streak >= 2 {
        format!("—— Metera · 连续记录第 {streak} 天 ——")
    } else {
        "—— Metera ——".to_string()
    };
    format!(
        r#"<tr><td align="center" style="padding:26px 32px 28px 32px;{H_FONT}font-size:12px;color:{H_SOFT};">{text}</td></tr>"#
    )
}

fn h_shell(content: &str, preheader: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Metera 日报</title>
</head>
<body style="margin:0;padding:0;background:#e9ebe9;">
<span style="display:none;font-size:0;line-height:0;max-height:0;overflow:hidden;">{preheader}</span>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" bgcolor="#e9ebe9" style="background:#e9ebe9;">
<tr><td align="center" style="padding:32px 12px;">
<table role="presentation" width="600" cellpadding="0" cellspacing="0" border="0" bgcolor="{H_BG}" style="width:600px;background:{H_BG};border-radius:14px;">
<tr><td height="4" bgcolor="{H_GREEN}" style="height:4px;line-height:4px;font-size:0;background:{H_GREEN};border-radius:14px 14px 0 0;">&nbsp;</td></tr>
{content}
</table>
</td></tr>
</table>
</body>
</html>"##
    )
}

fn h_header(date: NaiveDate) -> String {
    format!(
        r#"<tr><td style="padding:28px 32px 0 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr><td align="left" style="{H_FONT}font-size:12px;letter-spacing:3px;color:{H_MUTED};"><span style="color:{H_GREEN};">●</span>&nbsp;&nbsp;METERA · DAILY REPORT</td><td align="right" style="{H_FONT}font-size:12px;letter-spacing:1px;color:{H_SOFT};">DAILY DIGEST</td></tr></table></td></tr>
<tr><td style="padding:14px 32px 0 32px;{H_FONT}"><span style="font-size:26px;font-weight:700;color:{H_TEXT};">{}月{}日</span><span style="font-size:14px;color:{H_MUTED};">&nbsp;{} · 昨日 AI 使用日报</span></td></tr>"#,
        date.month(),
        date.day(),
        weekday_cn(date)
    )
}

/// 完整 HTML 日报（与纯文本 render 同源）。
fn render_html(input: &ReportInput, facts: &Facts) -> String {
    let change = facts
        .prev_change_pct
        .map(|pct| format!("{pct:+}%"))
        .unwrap_or_else(|| DASH.to_string());
    let streak_note = if facts.streak >= 2 {
        format!(" · 连续记录第 {} 天", facts.streak)
    } else {
        String::new()
    };
    let preheader = format!(
        "昨日 Token {} · 较前日 {change}{streak_note}",
        format_tokens(facts.total_yesterday as f64)
    );

    let mut content = h_header(input.yesterday);

    // 开场白 tint 条（复用纯文本同款文案）
    content.push_str(&format!(
        r##"<tr><td style="padding:16px 32px 0 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr><td bgcolor="#16241c" style="background:#16241c;border-left:3px solid {H_GREEN};padding:12px 16px;{H_FONT}font-size:14px;line-height:22px;color:{H_SUB};border-radius:6px;">{}</td></tr></table></td></tr>"##,
        html_escape(&opening(facts))
    ));

    // Hero 大数字
    content.push_str(&format!(
        r#"<tr><td style="padding:26px 32px 0 32px;{H_FONT}"><div style="font-size:11px;letter-spacing:2px;color:{H_SOFT};">TOKEN 总量</div><div style="padding-top:4px;"><span style="font-size:44px;font-weight:700;color:{H_GREEN};letter-spacing:-1px;">{}</span><span style="font-size:13px;color:{H_MUTED};">&nbsp;&nbsp;较前日&nbsp;<b style="color:{H_YELLOW};">{}</b></span></div>"#,
        format_tokens(facts.total_yesterday as f64),
        change
    ));
    if let Some(quip) = quip_hero(facts) {
        content.push_str(&format!(
            r#"<div style="font-size:12px;color:{H_SOFT};padding-top:6px;">{}</div>"#,
            html_escape(&quip)
        ));
    }
    content.push_str("</td></tr>");

    // KPI 2×2
    let cost_value = if facts.priced_buckets == 0 { DASH.to_string() } else { format_cost(facts.cost) };
    let cost_note = if facts.priced_buckets == 0 {
        "暂无定价数据".to_string()
    } else {
        let coverage = (facts.priced_buckets as f64 / input.buckets_yesterday.len() as f64 * 100.0).round() as i64;
        format!("定价覆盖率 {coverage}%")
    };
    let (active_value, active_note) = match facts.active_seconds {
        Some(active) => (format_duration(active), format!("总跨度 {}", format_duration(facts.span_seconds))),
        None => (DASH.to_string(), "昨日无会话记录".to_string()),
    };
    let cache_value = facts.cache_rate.map(|rate| format!("{:.0}%", rate * 100.0)).unwrap_or_else(|| DASH.to_string());
    let cache_note = match facts.cache_rate {
        Some(rate) if rate >= 0.5 => "缓存立大功，替你省下一大截",
        Some(_) => "缓存参与有限",
        None => "昨日无缓存数据",
    };
    let kpi_cell = |label: &str, value: &str, note: &str| {
        format!(
            r#"<td width="50%" bgcolor="{H_PANEL}" style="background:{H_PANEL};padding:14px 18px;{H_FONT}"><div style="font-size:11px;letter-spacing:1px;color:{H_SOFT};">{label}</div><div style="font-size:22px;font-weight:700;color:{H_TEXT};padding-top:2px;">{value}</div><div style="font-size:11px;color:{H_MUTED};padding-top:2px;">{}</div></td>"#,
            html_escape(note)
        )
    };
    content.push_str(&format!(
        r#"<tr><td style="padding:22px 32px 0 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="1" border="0" bgcolor="{H_LINE}" style="background:{H_LINE};border-radius:8px;"><tr>{}{}</tr><tr>{}{}</tr></table></td></tr>"#,
        kpi_cell("估算花销", &cost_value, &cost_note),
        kpi_cell("活跃时长", &active_value, &active_note),
        kpi_cell(
            "会话数",
            &facts.session_count.to_string(),
            &format!("消息 {} 条（你发起 {} 条）", facts.messages, facts.user_messages)
        ),
        kpi_cell("缓存命中率", &cache_value, cache_note)
    ));

    // 分工具 / 分模型 / 分项目
    let tools = ranked_groups(&input.buckets_yesterday, |bucket| display_source(&bucket.source));
    if !tools.is_empty() {
        content.push_str(&h_tools_section(&tools));
    }
    let models = ranked_groups(&input.buckets_yesterday, |bucket| bucket.model.clone());
    if !models.is_empty() {
        content.push_str(&h_models_section(&models));
    }
    let all_unknown = input.buckets_yesterday.iter().all(|bucket| bucket.project.is_empty() || bucket.project == "unknown");
    if input.include_project_names && !all_unknown {
        let projects = ranked_groups(&input.buckets_yesterday, |bucket| bucket.project.clone());
        if !projects.is_empty() {
            content.push_str(&h_projects_section(&projects));
        }
    }

    if let Some(heat) = h_heat_section(facts) {
        content.push_str(&heat);
    }
    if let Some(week) = h_week_section(input, facts) {
        content.push_str(&week);
    }
    if let Some(closing) = h_closing_section(&tools, &models, facts) {
        content.push_str(&closing);
    }
    content.push_str(&h_signature(facts.streak));

    h_shell(&content, &html_escape(&preheader))
}

/// §1.4 无数据日收缩版（HTML）。
fn render_html_rest_day(yesterday: NaiveDate) -> String {
    let content = format!(
        r##"{}
<tr><td style="padding:20px 32px 0 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr><td bgcolor="#16241c" style="background:#16241c;border-left:3px solid {H_GREEN};padding:12px 16px;{H_FONT}font-size:14px;line-height:22px;color:{H_SUB};border-radius:6px;">🌿 昨天（{}）没有记录到 AI 工具使用——看来是休息的一天。</td></tr></table></td></tr>
{}"##,
        h_header(yesterday),
        spoken_date(yesterday),
        h_signature(0)
    );
    h_shell(&content, &html_escape(&format!("昨天（{}）没有记录到 AI 工具使用", spoken_date(yesterday))))
}

// ============================== 取数（DB → ReportInput）==============================

pub fn collect_input(
    usage: &UsageRepository,
    include_project_names: bool,
    now: DateTime<Local>,
) -> Result<ReportInput, String> {
    let today = now.date_naive();
    let yesterday = today - Duration::days(1);
    let previous = yesterday - Duration::days(1);
    let window_start = yesterday - Duration::days(29);
    let (yesterday_start, yesterday_end) = day_bounds_utc(yesterday);
    let (previous_start, _) = day_bounds_utc(previous);
    let (window_start_utc, _) = day_bounds_utc(window_start);
    Ok(ReportInput {
        yesterday,
        include_project_names,
        buckets_yesterday: usage.buckets_between(&yesterday_start, &yesterday_end).map_err(|error| error.to_string())?,
        buckets_previous: usage.buckets_between(&previous_start, &yesterday_start).map_err(|error| error.to_string())?,
        buckets_30d: usage.buckets_between(&window_start_utc, &yesterday_end).map_err(|error| error.to_string())?,
        sessions_yesterday: usage.sessions_between(&yesterday_start, &yesterday_end).map_err(|error| error.to_string())?,
        sessions_yesterday_overlap: usage.sessions_overlapping(&yesterday_start, &yesterday_end).map_err(|error| error.to_string())?,
    })
}

// ============================== 发送流程 / 调度循环（§3.1 时机 A+B）==============================

fn update_status(app: &AppHandle, sent_at: Option<String>, error: Option<String>) {
    let ctx = app.state::<AppCtx>();
    {
        let mut settings = ctx.settings.lock().unwrap();
        if let Some(sent_at) = sent_at {
            settings.report_last_sent_at = Some(sent_at);
        }
        settings.report_last_error = error;
    }
    if let Err(save_error) = ctx.save_settings() {
        log::warn!("保存报告发送状态失败: {save_error}");
    }
    crate::windows::emit_settings(app);
}

/// 串行化所有报告流程(自动调度 + 手动发送),防止同一天双发。
static REPORT_FLOW_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// 完整流程：刷新数据 → 聚合昨日 → 渲染 → 发送 → 更新状态。
/// 不做「今日已发」检查——自动路径的查重在调度循环里，`send_report_now` 强制执行（§4）。
pub async fn run_report_flow(app: &AppHandle) -> Result<String, String> {
    let _flow_guard = REPORT_FLOW_LOCK.get_or_init(|| tokio::sync::Mutex::new(())).lock().await;
    // 发送前必扫一次；先等进行中的扫描结束，避免与周期扫描并发冲突（§3.1）
    {
        let ctx = app.state::<AppCtx>();
        let _guard = ctx.sync_running.lock().await;
    }
    crate::services::local_scanner::run(app.clone()).await;

    let ctx = app.state::<AppCtx>();
    let settings = ctx.settings.lock().unwrap().clone();
    let email = settings.report_email.trim().to_string();
    let host = settings.report_smtp_host.trim().to_string();
    if email.is_empty() || host.is_empty() || settings.report_smtp_password.is_empty() {
        return Err("请先在设置中完善邮箱、SMTP 服务器与授权码".into());
    }
    let message = {
        let usage = ctx.usage.lock().unwrap();
        let input = collect_input(&usage, settings.include_project_names, Local::now())?;
        render(&input)
    };
    let port = settings.report_smtp_port;
    let password = settings.report_smtp_password.clone();
    let target = email.clone();
    let subject = message.subject;
    let body = message.body;
    let html = message.html;
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        crate::services::mailer::send_report(&email, &host, port, &password, &subject, &body, &html)
    })
    .await
    .map_err(|error| format!("发送任务失败: {error}"))?;
    match outcome {
        Ok(()) => {
            update_status(app, Some(Local::now().to_rfc3339()), None);
            Ok(format!("昨日报告已发送到 {target}"))
        }
        Err(error) => {
            update_status(app, None, Some(error.clone()));
            Err(error)
        }
    }
}

/// 触发条件：启用 && 上次发送的本地日期 != 今天 && now >= 今天@report_send_time（§3.1）。
fn due_to_send(enabled: bool, send_time: NaiveTime, last_sent_at: Option<&str>, now: DateTime<Local>) -> bool {
    if !enabled {
        return false;
    }
    let already_sent = last_sent_at
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|moment| moment.with_timezone(&Local).date_naive())
        == Some(now.date_naive());
    !already_sent && now.time() >= send_time
}

/// 独立 tokio 任务：启动时（await 一次扫描完成）立即检查，之后每分钟检查；
/// 失败 15 分钟后重试，每日最多 3 次（内存计数，重启清零），仍失败则当天放弃（§4）。
pub fn start_daily_report(app: AppHandle) {
    if let Some(task) = app.state::<AppCtx>().report_task.lock().unwrap().take() {
        task.abort();
    }
    let worker = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        {
            let ctx = worker.state::<AppCtx>();
            let _guard = ctx.sync_running.lock().await;
        }
        crate::services::local_scanner::run(worker.clone()).await;
        let mut failures = 0u32;
        let mut next_retry_at: Option<DateTime<Local>> = None;
        let mut gave_up_on: Option<NaiveDate> = None;
        loop {
            let now = Local::now();
            let today = now.date_naive();
            let (enabled, send_time, last_sent_at) = {
                let ctx = worker.state::<AppCtx>();
                let settings = ctx.settings.lock().unwrap();
                (
                    settings.daily_report_enabled,
                    parse_send_time(&settings.report_send_time),
                    settings.report_last_sent_at.clone(),
                )
            };
            if !enabled || gave_up_on == Some(today) {
                if !enabled {
                    failures = 0;
                    next_retry_at = None;
                    gave_up_on = None;
                }
                tokio::time::sleep(StdDuration::from_secs(60)).await;
                continue;
            }
            let due = match next_retry_at {
                Some(retry_at) => now >= retry_at,
                None => due_to_send(enabled, send_time, last_sent_at.as_deref(), now),
            };
            if due {
                match run_report_flow(&worker).await {
                    Ok(message) => {
                        log::info!("每日报告：{message}");
                        failures = 0;
                        next_retry_at = None;
                    }
                    Err(error) => {
                        log::warn!("每日报告发送失败（第 {} 次）: {error}", failures + 1);
                        failures += 1;
                        if failures >= 3 {
                            gave_up_on = Some(today);
                            next_retry_at = None;
                            failures = 0;
                        } else {
                            next_retry_at = Some(Local::now() + Duration::minutes(15));
                        }
                    }
                }
            }
            tokio::time::sleep(StdDuration::from_secs(60)).await;
        }
    });
    *app.state::<AppCtx>().report_task.lock().unwrap() = Some(task);
}
// ============================== 测试 ==============================

#[cfg(test)]
mod tests {
    use super::*;

    fn yesterday() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 1, 7).unwrap() // 周日
    }

    /// 本地时刻 → UTC ISO 串（本地→UTC→本地往返一致，测试机器时区无关）。
    fn local_ts(date: NaiveDate, hour: u32, minute: u32) -> String {
        let naive = date.and_hms_opt(hour, minute, 0).unwrap();
        let local = Local.from_local_datetime(&naive).single().unwrap();
        to_utc_iso(local)
    }

    fn bucket_at(start: &str, source: &str, model: &str, project: &str, tokens: i64) -> UsageBucket {
        UsageBucket {
            source: source.into(),
            provider: "provider.test".into(),
            model: model.into(),
            project: project.into(),
            hostname: "host".into(),
            bucket_start: start.into(),
            input_tokens: tokens,
            output_tokens: 0,
            cached_input_tokens: 0,
            reasoning_output_tokens: 0,
            total_tokens: tokens,
        }
    }

    fn session(start: &str, end: &str, active: i64, duration: i64) -> UsageSession {
        UsageSession {
            source: "codex".into(),
            project: "proj".into(),
            hostname: "host".into(),
            session_hash: format!("{start}-{end}"),
            first_message_at: start.into(),
            last_message_at: end.into(),
            duration_seconds: duration,
            active_seconds: active,
            message_count: 10,
            user_message_count: 4,
            user_prompt_hours: vec![0; 24],
        }
    }

    fn input_with(
        buckets_y: Vec<UsageBucket>,
        buckets_p: Vec<UsageBucket>,
        buckets_30d: Vec<UsageBucket>,
        sessions: Vec<UsageSession>,
    ) -> ReportInput {
        ReportInput {
            yesterday: yesterday(),
            include_project_names: true,
            buckets_yesterday: buckets_y,
            buckets_previous: buckets_p,
            buckets_30d,
            sessions_yesterday: sessions.clone(),
            sessions_yesterday_overlap: sessions,
        }
    }

    fn iso8601_millis_z(value: &str) -> bool {
        let bytes = value.as_bytes();
        bytes.len() == 24
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes[10] == b'T'
            && bytes[13] == b':'
            && bytes[16] == b':'
            && bytes[19] == b'.'
            && bytes[23] == b'Z'
            && bytes[..4].iter().chain(&bytes[5..7]).chain(&bytes[8..10]).chain(&bytes[11..13]).chain(&bytes[14..16]).chain(&bytes[17..19]).chain(&bytes[20..23]).all(u8::is_ascii_digit)
    }

    #[test]
    fn day_bounds_follow_local_midnight_and_iso_format() {
        let (start, end) = day_bounds_utc(yesterday());
        assert!(iso8601_millis_z(&start), "格式须与 toISOString 一致: {start}");
        assert!(iso8601_millis_z(&end), "格式须与 toISOString 一致: {end}");
        let start_local = DateTime::parse_from_rfc3339(&start).unwrap().with_timezone(&Local).date_naive();
        let end_local = DateTime::parse_from_rfc3339(&end).unwrap().with_timezone(&Local).date_naive();
        assert_eq!(start_local, yesterday());
        assert_eq!(end_local, yesterday() + Duration::days(1));
    }

    #[test]
    fn parse_send_time_accepts_hh_mm_and_falls_back_to_0800() {
        assert_eq!(parse_send_time("08:00"), NaiveTime::from_hms_opt(8, 0, 0).unwrap());
        assert_eq!(parse_send_time("23:59"), NaiveTime::from_hms_opt(23, 59, 0).unwrap());
        assert_eq!(parse_send_time(" garbage "), NaiveTime::from_hms_opt(8, 0, 0).unwrap());
        assert_eq!(parse_send_time("25:61"), NaiveTime::from_hms_opt(8, 0, 0).unwrap());
    }

    #[test]
    fn weekday_names_match_known_week() {
        let monday = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(); // 已知周一
        let names = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
        for (offset, expected) in names.iter().enumerate() {
            assert_eq!(weekday_cn(monday + Duration::days(offset as i64)), *expected);
        }
    }

    #[test]
    fn rest_day_version_when_yesterday_empty() {
        let message = render(&input_with(Vec::new(), Vec::new(), Vec::new(), Vec::new()));
        assert_eq!(message.subject, "🤖 Metera 日报 · 1月7日 周日");
        assert_eq!(
            message.body,
            "🌿 昨天（1 月 7 日·周日）没有记录到 AI 工具使用——看来是休息的一天。\n\n—— Metera"
        );
    }
    #[test]
    fn full_report_matches_sample_structure() {
        let day = yesterday();
        let previous = day - Duration::days(1);
        let mut buckets_y = Vec::new();
        // Codex 8.2M / Claude 2.6M / Kimi 1.1M / ZCode 0.5M；峰值 14 点（4.2M）
        buckets_y.push(bucket_at(&local_ts(day, 9, 0), "codex", "gpt-5.5", "metera", 4_000_000));
        buckets_y.push(bucket_at(&local_ts(day, 14, 0), "codex", "gpt-5.5", "metera", 4_200_000));
        buckets_y.push(bucket_at(&local_ts(day, 10, 0), "claude-code", "mystery", "research", 2_600_000));
        buckets_y.push(bucket_at(&local_ts(day, 11, 0), "kimi-code", "kimi-k3", "unknown", 1_100_000));
        buckets_y.push(bucket_at(&local_ts(day, 12, 0), "zcode", "mystery", "misc", 500_000));
        let buckets_p = vec![bucket_at(&local_ts(previous, 10, 0), "codex", "gpt-5.5", "metera", 7_654_321)];
        let mut buckets_30d = buckets_y.clone();
        buckets_30d.extend(buckets_p.clone());
        // 近 30 日最高另有其人（5 天前 20M）→ 命中规则 2 而非规则 1
        buckets_30d.push(bucket_at(&local_ts(day - Duration::days(5), 10, 0), "codex", "gpt-5.5", "metera", 20_000_000));
        let mut first = session(&local_ts(day, 9, 0), &local_ts(day, 10, 0), 1800, 3600);
        first.user_prompt_hours[14] = 3;
        let mut second = session(&local_ts(day, 15, 0), &local_ts(day, 16, 0), 1800, 3600);
        second.user_prompt_hours[15] = 1;
        let message = render(&input_with(buckets_y, buckets_p, buckets_30d, vec![first, second]));

        assert_eq!(message.subject, "🤖 Metera 日报 · 1月7日 周日");
        let body = &message.body;
        assert!(body.starts_with("🔥 火力全开！昨天 Token 用量比前天涨了 62%。"), "开场白命中规则 2: {body}");
        assert!(body.contains(&format!("📊 昨日总览 {}", "─".repeat(25))), "总览分节: {body}");
        assert!(body.contains("Token 总量    12.4M     较前日 +62%"), "总览行: {body}");
        assert!(body.contains("估算花销      $44.06    定价覆盖率 60%"), "花销行: {body}");
        assert!(body.contains("活跃时长      1h 0m     总跨度 2h 0m"), "活跃行: {body}");
        assert!(body.contains("会话数        2         消息 20 条（你发起 8 条）"), "会话行: {body}");
        assert!(body.contains(&format!("🛠️ 分工具 {}", "─".repeat(26))), "工具分节: {body}");
        assert!(body.contains("Codex          8.2M     66%"), "分工具行: {body}");
        assert!(body.contains("Claude Code    2.6M     21%"), "分工具行: {body}");
        assert!(body.contains("Kimi Code      1.1M      9%"), "分工具行: {body}");
        assert!(body.contains("ZCode          500.0K    4%"), "分工具行: {body}");
        assert!(body.contains(&format!("🤖 分模型（Top 5） {}", "─".repeat(19))), "模型分节: {body}");
        assert!(body.contains("gpt-5.5        8.2M     66%"), "分模型行: {body}");
        assert!(body.contains("mystery        3.1M     25%"), "分模型行: {body}");
        assert!(body.contains(&format!("📁 分项目（Top 5） {}", "─".repeat(19))), "项目分节: {body}");
        assert!(body.contains("metera         8.2M     66%"), "分项目行: {body}");
        assert!(body.contains("research       2.6M     21%"), "分项目行: {body}");
        assert!(body.contains(&format!("✨ 数据彩蛋 {}", "─".repeat(25))), "彩蛋分节: {body}");
        assert!(body.contains("峰值时段      14:00–15:00（4.2M tokens）"), "峰值行: {body}");
        assert!(body.contains("最活跃时段    下午（12–18 点）占全天 100%"), "时段行: {body}");
        assert!(body.contains("缓存命中率    0%"), "缓存行: {body}");
        assert!(body.contains("强度          12.4M/活跃小时 · $3.55/百万 token"), "强度行: {body}");
        assert!(body.contains("近 7 日       ▁█▁▁▁▄▅ 日均 5.7M"), "走势行: {body}");
        assert!(body.ends_with("—— Metera · 连续记录第 2 天"), "署名: {body}");
    }

    #[test]
    fn opening_rules_priority_and_fallbacks() {
        let day = yesterday();
        // 规则 1：昨日为 30 日最高 → 优先于规则 2（+900%）
        let y = vec![bucket_at(&local_ts(day, 10, 0), "codex", "gpt-5.5", "p", 1_000_000)];
        let p = vec![bucket_at(&local_ts(day - Duration::days(1), 10, 0), "codex", "gpt-5.5", "p", 100_000)];
        let mut w = y.clone();
        w.extend(p.clone());
        let message = render(&input_with(y, p, w, Vec::new()));
        assert!(message.body.starts_with(OPENING_RECORD_HIGH), "30 日最高优先: {}", message.body);

        // 规则 3：较前日 ≤ -50%（前日有数据，昨日非 30 日最高）
        let y2 = vec![bucket_at(&local_ts(day, 10, 0), "codex", "gpt-5.5", "p", 100_000)];
        let p2 = vec![bucket_at(&local_ts(day - Duration::days(1), 10, 0), "codex", "gpt-5.5", "p", 400_000)];
        let mut w2 = y2.clone();
        w2.extend(p2.clone());
        let message = render(&input_with(y2, p2, w2, Vec::new()));
        assert!(message.body.starts_with("🍃 昨天放缓了节奏，用量只有前天的 25%。"), "规则 3: {}", message.body);

        // 规则 4：最活跃时段 = 凌晨（涨跌幅 ±50% 内、非 30 日最高、streak < 7）
        let y3 = vec![bucket_at(&local_ts(day, 3, 0), "codex", "gpt-5.5", "p", 1_000_000)];
        let p3 = vec![bucket_at(&local_ts(day - Duration::days(1), 10, 0), "codex", "gpt-5.5", "p", 1_000_000)];
        let mut w3 = y3.clone();
        w3.extend(p3.clone());
        for offset in 2..5 {
            w3.push(bucket_at(&local_ts(day - Duration::days(offset), 10, 0), "codex", "gpt-5.5", "p", 2_000_000));
        }
        let mut night = session(&local_ts(day, 3, 0), &local_ts(day, 4, 0), 1800, 3600);
        night.user_prompt_hours[3] = 5;
        let message = render(&input_with(y3, p3, w3, vec![night]));
        assert!(message.body.starts_with("🌙 夜猫子模式"), "规则 4: {}", message.body);

        // 规则 5：连续记录 ≥ 7 天
        let y4 = vec![bucket_at(&local_ts(day, 10, 0), "codex", "gpt-5.5", "p", 1_000_000)];
        let p4 = vec![bucket_at(&local_ts(day - Duration::days(1), 10, 0), "codex", "gpt-5.5", "p", 1_000_000)];
        let mut w4 = y4.clone();
        w4.extend(p4.clone());
        for offset in 2..7 {
            w4.push(bucket_at(&local_ts(day - Duration::days(offset), 10, 0), "codex", "gpt-5.5", "p", 2_000_000));
        }
        let message = render(&input_with(y4, p4, w4, Vec::new()));
        assert!(message.body.starts_with("💪 连续第 7 天记录到使用，保持住！"), "规则 5: {}", message.body);

        // 规则 6：兜底（前日有数据但涨跌幅 ±50% 内，非凌晨，streak < 7）
        let y5 = vec![bucket_at(&local_ts(day, 10, 0), "codex", "gpt-5.5", "p", 1_000_000)];
        let p5 = vec![bucket_at(&local_ts(day - Duration::days(1), 10, 0), "codex", "gpt-5.5", "p", 1_000_000)];
        let mut w5 = y5.clone();
        w5.extend(p5.clone());
        w5.push(bucket_at(&local_ts(day - Duration::days(5), 10, 0), "codex", "gpt-5.5", "p", 2_000_000));
        let message = render(&input_with(y5, p5, w5, Vec::new()));
        assert!(message.body.starts_with(OPENING_FALLBACK), "兜底: {}", message.body);
    }

    #[test]
    fn missing_data_shows_dashes_and_never_fabricates() {
        let day = yesterday();
        let y = vec![bucket_at(&local_ts(day, 10, 0), "codex", "mystery-model", "p", 1_000_000)];
        let message = render(&input_with(y.clone(), Vec::new(), y, Vec::new()));
        let body = &message.body;
        assert!(body.contains("较前日 --"), "前日无数据: {body}");
        assert!(body.contains("估算花销      --"), "未知模型整行 --: {body}");
        assert!(!body.contains("定价覆盖率"), "无定价不显示覆盖率: {body}");
        assert!(body.contains("活跃时长      --"), "无会话: {body}");
        assert!(body.contains("会话数        0         消息 0 条（你发起 0 条）"), "0 显示真实值: {body}");
        assert!(body.contains("缓存命中率    0%"), "分母非 0 时显示真实 0: {body}");
        assert!(!body.contains("近 7 日"), "不足 2 天数据行省略: {body}");
        assert!(!body.contains("强度"), "活跃/定价缺失 → 强度行省略: {body}");
    }

    #[test]
    fn project_section_respects_privacy_setting() {
        let day = yesterday();
        let y = vec![bucket_at(&local_ts(day, 10, 0), "codex", "gpt-5.5", "secret", 1_000_000)];
        let mut input = input_with(y.clone(), Vec::new(), y, Vec::new());
        input.include_project_names = false;
        let message = render(&input);
        assert!(!message.body.contains("分项目"), "设置关闭 → 整节省略: {}", message.body);

        let all_unknown = vec![bucket_at(&local_ts(day, 10, 0), "codex", "gpt-5.5", "unknown", 1_000_000)];
        let message = render(&input_with(all_unknown.clone(), Vec::new(), all_unknown, Vec::new()));
        assert!(!message.body.contains("分项目"), "全部 unknown → 整节省略: {}", message.body);
    }

    #[test]
    fn cache_suffix_only_when_hit_rate_at_least_half() {
        let day = yesterday();
        let mut high = bucket_at(&local_ts(day, 10, 0), "codex", "gpt-5.5", "p", 1_000_000);
        high.input_tokens = 400_000;
        high.cached_input_tokens = 600_000;
        let message = render(&input_with(vec![high.clone()], Vec::new(), vec![high], Vec::new()));
        assert!(message.body.contains("缓存命中率    60%（缓存立大功）"), "≥50% 带后缀: {}", message.body);

        let mut low = bucket_at(&local_ts(day, 10, 0), "codex", "gpt-5.5", "p", 1_000_000);
        low.input_tokens = 800_000;
        low.cached_input_tokens = 200_000;
        let message = render(&input_with(vec![low.clone()], Vec::new(), vec![low], Vec::new()));
        assert!(message.body.contains("缓存命中率    20%"), "命中率行: {}", message.body);
        assert!(!message.body.contains(CACHE_SUFFIX), "<50% 无后缀: {}", message.body);
    }

    #[test]
    fn week_sparkline_and_streak_signature() {
        let day = yesterday();
        let y = vec![bucket_at(&local_ts(day, 23, 0), "codex", "gpt-5.5", "p", 7_000_000)];
        let mut w = y.clone();
        for offset in 1..7 {
            w.push(bucket_at(&local_ts(day - Duration::days(offset), 10, 0), "codex", "gpt-5.5", "p", (7 - offset) as i64 * 1_000_000));
        }
        let message = render(&input_with(y, Vec::new(), w, Vec::new()));
        let body = &message.body;
        assert!(body.contains("▂▃▄▅▆▇█ 日均 4.0M · 昨日为 7 日最高 🏆"), "7 日走势: {body}");
        assert!(body.contains("连续记录第 7 天"), "streak 署名: {body}");
    }

    #[test]
    fn long_names_are_truncated_to_keep_columns_aligned() {
        let day = yesterday();
        let y = vec![
            bucket_at(&local_ts(day, 10, 0), "codex", "deepseek/deepseek-v4-flash", "c--users-suter-appdata-roaming-reasonix-global-workspace", 1_000_000),
            bucket_at(&local_ts(day, 11, 0), "codex", "gpt-5.5", "metera", 2_000_000),
        ];
        let message = render(&input_with(y.clone(), Vec::new(), y, Vec::new()));
        for line in message.body.lines() {
            let width = line.chars().map(|ch| if is_wide(ch) { 2 } else { 1 }).sum::<usize>();
            assert!(width <= 64, "行宽应保持克制: {line}");
        }
        assert!(message.body.contains("deepseek/deep… 1.0M"), "长模型名截断: {}", message.body);
        assert!(message.body.contains("c--users-sute… 1.0M"), "长项目名截断: {}", message.body);
    }

    #[test]
    fn html_report_uses_inline_tables_cids_and_no_removed_phrase() {
        let day = yesterday();
        let buckets_y = vec![
            bucket_at(&local_ts(day, 9, 0), "codex", "gpt-5.6-sol", "metera", 4_000_000),
            bucket_at(&local_ts(day, 14, 0), "codex", "gpt-5.6-sol", "metera", 4_200_000),
            bucket_at(&local_ts(day, 10, 0), "kimi-code", "kimi-code/k3", "research", 1_100_000),
        ];
        let mut window = buckets_y.clone();
        window.push(bucket_at(&local_ts(day - Duration::days(1), 10, 0), "codex", "gpt-5.6-sol", "metera", 7_000_000));
        let message = render(&input_with(buckets_y, Vec::new(), window, Vec::new()));
        let html = &message.html;
        assert!(html.contains(r#"width="600""#), "600px 容器: {html}");
        assert!(!html.contains("<style"), "样式全内联、无 <style> 块: {html}");
        assert!(html.contains(r#"src="cid:openai""#), "gpt 模型带 openai 图标: {html}");
        assert!(html.contains(r#"src="cid:moonshot""#), "kimi 模型带 moonshot 图标: {html}");
        assert!(!html.contains("数据未离开本机"), "该文案已删除: {html}");
        assert!(html.contains("全天候节奏"), "24h 热力带: {html}");
        assert!(html.contains("近 7 日走势"), "7 日竖条图: {html}");
        assert!(html.contains("gpt-5.6-sol 一个模型扛了"), "模型点评: {html}");
    }

    #[test]
    fn html_escapes_names_and_rest_day_has_html_version() {
        let day = yesterday();
        let y = vec![bucket_at(&local_ts(day, 10, 0), "codex", "gpt-<script>", "a&b", 1_000_000)];
        let message = render(&input_with(y.clone(), Vec::new(), y, Vec::new()));
        assert!(!message.html.contains("<script>"), "模型名必须转义: {}", message.html);
        assert!(message.html.contains("gpt-&lt;script&gt;"), "转义后保留可读名: {}", message.html);
        assert!(message.html.contains("a&amp;b"), "项目名 & 转义: {}", message.html);

        let rest = render(&input_with(Vec::new(), Vec::new(), Vec::new(), Vec::new()));
        assert!(rest.html.contains("休息的一天"), "无数据日 HTML 版: {}", rest.html);
        assert!(rest.html.contains(r#"width="600""#), "收缩版同宽: {}", rest.html);
    }

    #[test]
    fn minute_slots_dedupe_overlapping_sessions() {
        let day = yesterday();
        // 两个完全重叠的会话：同一时间段各报 30 分钟活跃 → 去重取 max 而非相加
        let first = session(&local_ts(day, 10, 0), &local_ts(day, 11, 0), 1800, 3600);
        let second = session(&local_ts(day, 10, 0), &local_ts(day, 11, 0), 1800, 3600);
        let seconds = active_seconds_in_window(
            &[first, second],
            local_day_start(day),
            local_day_start(day + Duration::days(1)),
        );
        assert_eq!(seconds, 1800, "重叠分钟槽取 max 而非相加");

        // 零时长会话：单槽最多记 60 秒
        let point = session(&local_ts(day, 12, 30), &local_ts(day, 12, 30), 45, 0);
        let seconds = active_seconds_in_window(
            &[point],
            local_day_start(day),
            local_day_start(day + Duration::days(1)),
        );
        assert_eq!(seconds, 45);
    }

    #[test]
    fn due_to_send_matches_trigger_model() {
        let day = yesterday();
        let nine = Local.from_local_datetime(&day.and_hms_opt(9, 0, 0).unwrap()).single().unwrap();
        let seven = Local.from_local_datetime(&day.and_hms_opt(7, 0, 0).unwrap()).single().unwrap();
        let send_at = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        assert!(due_to_send(true, send_at, None, nine), "启用且过点 → 触发");
        assert!(!due_to_send(true, send_at, None, seven), "未到时 → 等待");
        assert!(!due_to_send(false, send_at, None, nine), "未启用 → 不触发");
        assert!(!due_to_send(true, send_at, Some(&nine.to_rfc3339()), nine), "今日已发 → 跳过");
        let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        assert!(due_to_send(true, midnight, None, seven), "00:00 = 不等待");
    }

    /// 用真实本地库渲染一份报告打印出来（不发送），供人工对照 §1.2 样稿验收。
    #[test]
    #[ignore]
    fn render_real_data_for_manual_inspection() {
        let data_dir = std::env::var("METERA_DATA_DIR").unwrap_or_else(|_| r"D:\MeteraData".into());
        let usage = UsageRepository::open(std::path::PathBuf::from(data_dir).join("metera.db")).unwrap();
        let input = collect_input(&usage, true, Local::now()).unwrap();
        let message = render(&input);
        println!("主题：{}\n\n{}", message.subject, message.body);
    }

    /// 真实发送完整报告（HTML+纯文本双格式）：读应用 settings.json 里的邮箱配置，默认忽略。
    #[test]
    #[ignore]
    fn live_send_report_with_saved_settings() {
        let data_dir = std::env::var("METERA_DATA_DIR").unwrap_or_else(|_| r"D:\MeteraData".into());
        let raw = std::fs::read_to_string(std::path::PathBuf::from(&data_dir).join("settings.json"))
            .expect("读取 settings.json 失败（请先在应用中配置邮箱）");
        let settings: crate::state::AppSettings = serde_json::from_str(&raw).expect("解析 settings.json 失败");
        let usage = UsageRepository::open(std::path::PathBuf::from(&data_dir).join("metera.db")).unwrap();
        let input = collect_input(&usage, settings.include_project_names, Local::now()).unwrap();
        let message = render(&input);
        println!("主题：{}\n\n{}", message.subject, message.body);
        crate::services::mailer::send_report(
            &settings.report_email,
            &settings.report_smtp_host,
            settings.report_smtp_port,
            &settings.report_smtp_password,
            &message.subject,
            &message.body,
            &message.html,
        )
        .expect("真实发送失败");
    }
}