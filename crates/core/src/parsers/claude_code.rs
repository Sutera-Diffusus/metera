use super::{extract_sessions, ParserOutput, SessionEvent};
use crate::usage::UsageBucket;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

const HALF_HOUR_MS: i64 = 30 * 60 * 1000;

pub struct ClaudeCodeParser { roots: Vec<PathBuf> }

impl ClaudeCodeParser {
    pub fn new(home: impl AsRef<Path>) -> Self {
        let default = home.as_ref().join(".claude");
        let mut roots = vec![default.clone()];
        if let Some(custom) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
            let custom = PathBuf::from(custom);
            if canonical(&custom) != canonical(&default) { roots.push(custom); }
        }
        Self { roots }
    }

    pub fn parse(&self, hostname: &str, include_project: bool) -> io::Result<ParserOutput> {
        let mut buckets = BTreeMap::<String, UsageBucket>::new();
        let mut events = Vec::new();
        let mut seen_files = HashSet::new();
        let mut seen_sessions = HashSet::new();
        let mut seen_uuids = HashSet::new();
        let mut seen_events = HashSet::new();
        let mut output = ParserOutput::default();

        for root in &self.roots {
            let projects = root.join("projects");
            for path in jsonl_files(&projects)? {
                let relative = path.strip_prefix(&projects).unwrap_or(&path).to_string_lossy().to_string();
                if !seen_files.insert(relative.clone()) { continue; }
                let session_id = path.file_stem().and_then(|v| v.to_str()).unwrap_or("unknown").to_string();
                seen_sessions.insert(session_id.clone());
                let project = if include_project { project_from_relative(&relative) } else { "unknown".into() };
                parse_file(&path, &session_id, &project, hostname, true, &mut seen_uuids, &mut seen_events, &mut buckets, &mut events, &mut output)?;
            }
        }
        for root in &self.roots {
            for path in jsonl_files(&root.join("transcripts"))? {
                let session_id = path.file_stem().and_then(|v| v.to_str()).unwrap_or("unknown").to_string();
                if !seen_sessions.insert(session_id.clone()) { continue; }
                parse_file(&path, &session_id, "unknown", hostname, false, &mut seen_uuids, &mut seen_events, &mut buckets, &mut events, &mut output)?;
            }
        }
        output.buckets = buckets.into_values().collect();
        output.sessions = extract_sessions(events);
        Ok(output)
    }
}

fn parse_file(path: &Path, session_id: &str, project: &str, hostname: &str, tokens: bool,
    seen_uuids: &mut HashSet<String>, seen_events: &mut HashSet<String>, buckets: &mut BTreeMap<String, UsageBucket>,
    events: &mut Vec<SessionEvent>, output: &mut ParserOutput) -> io::Result<()> {
    output.files_scanned += 1;
    for line in BufReader::new(File::open(path)?).lines() {
        let line = match line { Ok(line) => line, Err(_) => { output.malformed_lines += 1; continue; } };
        if line.trim().is_empty() { continue; }
        let obj = match serde_json::from_str::<Value>(&line) { Ok(v) => v, Err(_) => { output.malformed_lines += 1; continue; } };
        let Some(timestamp) = obj.get("timestamp").and_then(Value::as_str).and_then(|v| DateTime::parse_from_rfc3339(v).ok()).map(|v| v.timestamp_millis()) else { continue };
        let kind = obj.get("type").and_then(Value::as_str).unwrap_or("");
        if matches!(kind, "user" | "assistant" | "tool_use" | "tool_result") {
            // 同一会话内按 uuid 去重事件,避免 recovery/append 文件重复统计活跃时长。
            let event_uuid = obj.get("uuid").and_then(Value::as_str).map(str::to_string)
                .unwrap_or_else(|| format!("{}:{}:{}", session_id, timestamp, kind));
            if seen_events.insert(event_uuid) {
                events.push(SessionEvent { session_id: session_id.into(), source: "claude-code".into(), project: project.into(), hostname: hostname.into(), timestamp_ms: timestamp, is_user: kind == "user" });
            }
        }
        if !tokens || kind != "assistant" { continue; }
        let Some(message) = obj.get("message") else { continue }; let Some(usage) = message.get("usage") else { continue };
        if let Some(uuid) = obj.get("uuid").and_then(Value::as_str) { if !seen_uuids.insert(uuid.into()) { continue; } }
        let input = number(usage, "input_tokens"); let output_tokens = number(usage, "output_tokens");
        let cached = number(usage, "cache_read_input_tokens");
        if input == 0 && output_tokens == 0 && cached == 0 { continue; }
        add_bucket(buckets, timestamp, message.get("model").and_then(Value::as_str).unwrap_or("unknown"), project, hostname, input, output_tokens, cached);
        output.usage_records += 1;
    }
    Ok(())
}

fn add_bucket(map: &mut BTreeMap<String, UsageBucket>, timestamp: i64, model: &str, project: &str, hostname: &str, input: i64, output: i64, cached: i64) {
    let start = timestamp.div_euclid(HALF_HOUR_MS) * HALF_HOUR_MS;
    let Some(bucket_start) = DateTime::<Utc>::from_timestamp_millis(start).map(|v| v.to_rfc3339_opts(SecondsFormat::Millis, true)) else { return };
    let key = [model, project, hostname, &bucket_start].join("\0");
    let provider = super::provider_key(std::env::var("ANTHROPIC_BASE_URL").ok().as_deref(), "api.anthropic.com");
    let bucket = map.entry(key).or_insert_with(|| UsageBucket { source: "claude-code".into(), provider, model: model.into(), project: project.into(), hostname: hostname.into(), bucket_start, input_tokens: 0, output_tokens: 0, cached_input_tokens: 0, reasoning_output_tokens: 0, total_tokens: 0 });
    bucket.input_tokens += input; bucket.output_tokens += output; bucket.cached_input_tokens += cached;
    bucket.total_tokens = bucket.input_tokens + bucket.output_tokens + bucket.cached_input_tokens + bucket.reasoning_output_tokens;
}

fn jsonl_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    if !root.exists() { return Ok(Vec::new()); }
    let mut result = Vec::new(); let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() { for entry in fs::read_dir(dir)? { let entry = entry?; let path = entry.path(); if entry.file_type()?.is_dir() { dirs.push(path); } else if path.extension().and_then(|v| v.to_str()) == Some("jsonl") { result.push(path); } } }
    result.sort(); Ok(result)
}
fn canonical(path: &Path) -> PathBuf { path.canonicalize().unwrap_or_else(|_| path.to_path_buf()) }
fn project_from_relative(relative: &str) -> String { relative.split(['/', '\\']).next().unwrap_or("unknown").split('-').filter(|v| !v.is_empty()).last().unwrap_or("unknown").into() }
fn number(value: &Value, key: &str) -> i64 { value.get(key).and_then(|v| v.as_i64().or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))).unwrap_or(0).max(0) }
