pub mod codex;
pub mod claude_code;
pub mod dsh;
pub mod kimi_code;
pub mod reasonix;
pub mod workbuddy;
pub mod zcode;

use crate::usage::{UsageBucket, UsageSession};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub fn provider_key(raw: Option<&str>, fallback: &str) -> String {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return fallback.to_string();
    };
    let candidate = if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
    url::Url::parse(&candidate)
        .ok()
        .and_then(|value| value.host_str().map(|host| host.trim_start_matches("www.").to_ascii_lowercase()))
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| raw.to_ascii_lowercase())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParserOutput {
    pub buckets: Vec<UsageBucket>,
    pub sessions: Vec<UsageSession>,
    pub files_scanned: usize,
    pub usage_records: usize,
    pub malformed_lines: usize,
}

#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub session_id: String,
    pub source: String,
    pub project: String,
    pub hostname: String,
    pub timestamp_ms: i64,
    pub is_user: bool,
}

/// 同一回合内，相邻事件间隔超过该值（毫秒）视为中断，间隔不计入活跃。
const ACTIVE_GAP_MS: i64 = 5 * 60 * 1000;
/// 单个用户回合（一次提问到下一次提问）最多计入的活跃时长（毫秒）。
const TURN_ACTIVE_CAP_MS: i64 = 15 * 60 * 1000;

pub fn extract_sessions(events: Vec<SessionEvent>) -> Vec<UsageSession> {
    let mut groups = BTreeMap::<String, Vec<SessionEvent>>::new();
    for event in events { groups.entry(event.session_id.clone()).or_default().push(event); }
    groups.into_values().filter_map(|mut events| {
        events.sort_by_key(|event| event.timestamp_ms);
        let first = events.first()?.clone();
        let last = events.last()?.clone();
        let mut active_seconds = 0i64;
        let mut active_ms = 0i64;
        let mut last_event_ms = None::<i64>;
        let mut waiting = false;
        let mut user_prompt_hours = vec![0i64; 24];
        let mut user_message_count = 0i64;
        for event in &events {
            if event.is_user {
                active_seconds += (active_ms.min(TURN_ACTIVE_CAP_MS) + 500) / 1000;
                active_ms = 0; last_event_ms = None; waiting = true;
                user_message_count += 1;
                if let Some(time) = DateTime::<Utc>::from_timestamp_millis(event.timestamp_ms) {
                    user_prompt_hours[time.format("%H").to_string().parse::<usize>().unwrap_or(0)] += 1;
                }
            } else if waiting {
                last_event_ms = Some(event.timestamp_ms); waiting = false;
            } else if let Some(previous) = last_event_ms {
                let gap = event.timestamp_ms - previous;
                if gap > 0 && gap <= ACTIVE_GAP_MS { active_ms += gap; }
                last_event_ms = Some(event.timestamp_ms);
            }
        }
        if last_event_ms.is_some() {
            active_seconds += (active_ms.min(TURN_ACTIVE_CAP_MS) + 500) / 1000;
        }
        let session_hash = hex::encode(Sha256::digest(first.session_id.as_bytes()))[..16].to_string();
        Some(UsageSession {
            source: first.source, project: first.project, hostname: first.hostname,
            session_hash,
            first_message_at: DateTime::<Utc>::from_timestamp_millis(first.timestamp_ms)?.to_rfc3339_opts(SecondsFormat::Millis, true),
            last_message_at: DateTime::<Utc>::from_timestamp_millis(last.timestamp_ms)?.to_rfc3339_opts(SecondsFormat::Millis, true),
            duration_seconds: ((last.timestamp_ms - first.timestamp_ms + 500) / 1000).max(0),
            active_seconds, message_count: events.len() as i64, user_message_count, user_prompt_hours,
        })
    }).collect()
}
