use super::{extract_sessions, ParserOutput, SessionEvent};
use crate::usage::UsageBucket;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const HALF_HOUR_MS: i64 = 30 * 60 * 1000;

pub struct KimiCodeParser {
    current_root: PathBuf,
    legacy_root: PathBuf,
}

impl KimiCodeParser {
    pub fn new(current_root: impl Into<PathBuf>, legacy_root: impl Into<PathBuf>) -> Self {
        Self {
            current_root: current_root.into(),
            legacy_root: legacy_root.into(),
        }
    }

    pub fn parse(&self, hostname: &str, include_project: bool) -> io::Result<ParserOutput> {
        let mut output = ParserOutput::default();
        let mut buckets = BTreeMap::<BucketKey, UsageBucket>::new();
        let mut events = Vec::new();
        self.parse_current(hostname, include_project, &mut output, &mut buckets, &mut events)?;
        self.parse_legacy(hostname, include_project, &mut output, &mut buckets, &mut events)?;
        output.buckets = buckets.into_values().collect();
        output.sessions = extract_sessions(events);
        Ok(output)
    }

    fn parse_current(
        &self,
        hostname: &str,
        include_project: bool,
        output: &mut ParserOutput,
        buckets: &mut BTreeMap<BucketKey, UsageBucket>,
        events: &mut Vec<SessionEvent>,
    ) -> io::Result<()> {
        let sessions_root = self.current_root.join("sessions");
        let wires = find_named_files(&sessions_root, "wire.jsonl")?;
        let projects = load_current_projects(&self.current_root)?;
        output.files_scanned += wires.len();

        for wire in wires {
            let session_dir = wire
                .ancestors()
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with("session_"))
                })
                .unwrap_or(wire.parent().unwrap_or(&wire));
            let fallback = session_dir
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .map(project_from_bucket)
                .unwrap_or_else(|| "unknown".into());
            let indexed = projects.get(&normalize_path(session_dir));
            let project = visible_project(indexed.cloned().unwrap_or(fallback), include_project);
            let session_id = wire.to_string_lossy().into_owned();

            for line in read_snapshot_lines(&wire)? {
                let event = match serde_json::from_str::<Value>(&line) {
                    Ok(event) => event,
                    Err(_) => {
                        output.malformed_lines += 1;
                        continue;
                    }
                };
                let event_type = event.get("type").and_then(Value::as_str);
                let timestamp = event.get("time").and_then(Value::as_i64);
                if event_type == Some("turn.prompt") && event.pointer("/origin/kind").and_then(Value::as_str) == Some("user") {
                    if let Some(timestamp_ms) = timestamp { events.push(SessionEvent { session_id: session_id.clone(), source: "kimi-code".into(), project: project.clone(), hostname: hostname.into(), timestamp_ms, is_user: true }); }
                    continue;
                }
                if event_type != Some("usage.record") {
                    continue;
                }
                let Some(timestamp_ms) = timestamp else {
                    continue;
                };
                events.push(SessionEvent { session_id: session_id.clone(), source: "kimi-code".into(), project: project.clone(), hostname: hostname.into(), timestamp_ms, is_user: false });
                let Some(usage) = event.get("usage") else {
                    continue;
                };
                let input = token(usage, "inputOther") + token(usage, "inputCacheCreation");
                let output_tokens = token(usage, "output");
                let cached = token(usage, "inputCacheRead");
                if input == 0 && output_tokens == 0 && cached == 0 {
                    continue;
                }
                add_bucket(
                    buckets,
                    hostname,
                    event
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown"),
                    &project,
                    timestamp_ms,
                    input,
                    output_tokens,
                    cached,
                );
                output.usage_records += 1;
            }
        }
        Ok(())
    }

    fn parse_legacy(
        &self,
        hostname: &str,
        include_project: bool,
        output: &mut ParserOutput,
        buckets: &mut BTreeMap<BucketKey, UsageBucket>,
        events: &mut Vec<SessionEvent>,
    ) -> io::Result<()> {
        let wires = find_named_files(&self.legacy_root.join("sessions"), "wire.jsonl")?;
        let default_model =
            legacy_default_model(&self.legacy_root).unwrap_or_else(|| "unknown".into());
        let mut seen_ids = HashSet::new();
        output.files_scanned += wires.len();

        for wire in wires {
            let project = visible_project(
                wire.parent()
                    .and_then(Path::parent)
                    .and_then(Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".into()),
                include_project,
            );
            let mut current_model = default_model.clone();
            let session_id = wire.to_string_lossy().into_owned();
            let mut last_timestamp_ms = None;
            for line in read_snapshot_lines(&wire)? {
                let raw = match serde_json::from_str::<Value>(&line) {
                    Ok(raw) => raw,
                    Err(_) => {
                        output.malformed_lines += 1;
                        continue;
                    }
                };
                let envelope = raw.get("message").unwrap_or(&raw);
                let payload = envelope.get("payload").or_else(|| raw.get("payload"));
                let Some(payload) = payload else { continue };
                last_timestamp_ms = numeric_timestamp_ms(raw.get("timestamp"))
                    .or_else(|| numeric_timestamp_ms(payload.get("timestamp")))
                    .or(last_timestamp_ms);
                if let Some(model) = payload.get("model").and_then(Value::as_str) {
                    current_model = model.into();
                }
                let record_type = envelope
                    .get("type")
                    .or_else(|| raw.get("type"))
                    .and_then(Value::as_str);
                if let Some(timestamp_ms) = last_timestamp_ms {
                    events.push(SessionEvent { session_id: session_id.clone(), source: "kimi-code".into(), project: project.clone(), hostname: hostname.into(), timestamp_ms, is_user: matches!(record_type, Some("TurnBegin" | "UserMessage" | "user_message" | "Input")) });
                }
                if record_type != Some("StatusUpdate") {
                    continue;
                }
                let Some(usage) = payload.get("token_usage") else {
                    continue;
                };
                if let Some(id) = payload.get("message_id").and_then(Value::as_str) {
                    if !seen_ids.insert(id.to_string()) {
                        continue;
                    }
                }
                let Some(timestamp_ms) = last_timestamp_ms else {
                    continue;
                };
                let input = token(usage, "input_other") + token(usage, "input_cache_creation");
                let output_tokens = token(usage, "output");
                let cached = token(usage, "input_cache_read");
                if input == 0 && output_tokens == 0 && cached == 0 {
                    continue;
                }
                add_bucket(
                    buckets,
                    hostname,
                    &current_model,
                    &project,
                    timestamp_ms,
                    input,
                    output_tokens,
                    cached,
                );
                output.usage_records += 1;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BucketKey {
    bucket_start: String,
    model: String,
    project: String,
    hostname: String,
}

fn add_bucket(
    buckets: &mut BTreeMap<BucketKey, UsageBucket>,
    hostname: &str,
    model: &str,
    project: &str,
    timestamp_ms: i64,
    input: i64,
    output: i64,
    cached: i64,
) {
    let rounded = timestamp_ms.div_euclid(HALF_HOUR_MS) * HALF_HOUR_MS;
    let Some(bucket_start) = DateTime::<Utc>::from_timestamp_millis(rounded)
        .map(|date| date.to_rfc3339_opts(SecondsFormat::Millis, true))
    else {
        return;
    };
    let key = BucketKey {
        bucket_start: bucket_start.clone(),
        model: model.into(),
        project: project.into(),
        hostname: hostname.into(),
    };
    let bucket = buckets.entry(key).or_insert_with(|| UsageBucket {
        source: "kimi-code".into(),
        provider: "api.kimi.com".into(),
        model: model.chars().take(100).collect(),
        project: project.chars().take(200).collect(),
        hostname: hostname.into(),
        bucket_start,
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        reasoning_output_tokens: 0,
        total_tokens: 0,
    });
    bucket.input_tokens += input;
    bucket.output_tokens += output;
    bucket.cached_input_tokens += cached;
    bucket.total_tokens = bucket.input_tokens + bucket.output_tokens + bucket.cached_input_tokens + bucket.reasoning_output_tokens;
}

fn load_current_projects(root: &Path) -> io::Result<HashMap<String, String>> {
    let path = root.join("session_index.jsonl");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let mut projects = HashMap::new();
    for line in read_snapshot_lines(&path)? {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let (Some(session_dir), Some(work_dir)) = (
            entry.get("sessionDir").and_then(Value::as_str),
            entry.get("workDir").and_then(Value::as_str),
        ) else {
            continue;
        };
        let path = PathBuf::from(session_dir);
        let resolved = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        projects.insert(normalize_path(&resolved), project_name(work_dir));
    }
    Ok(projects)
}

fn legacy_default_model(root: &Path) -> Option<String> {
    let raw = fs::read_to_string(root.join("config.toml")).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("default_model") {
            return value
                .split_once('=')
                .map(|(_, value)| value.trim().trim_matches(['\'', '"']).to_string())
                .filter(|value| !value.is_empty());
        }
    }
    None
}

fn find_named_files(root: &Path, name: &str) -> io::Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                directories.push(path);
            } else if entry.file_name() == name {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn read_snapshot_lines(path: &Path) -> io::Result<Vec<String>> {
    let size = fs::metadata(path)?.len();
    let mut raw = String::new();
    File::open(path)?.take(size).read_to_string(&mut raw)?;
    Ok(raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect())
}

fn token(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_f64().map(|n| n.round() as i64))
        })
        .unwrap_or(0)
        .max(0)
}

fn numeric_timestamp_ms(value: Option<&Value>) -> Option<i64> {
    let number = value?.as_f64()?;
    number.is_finite().then(|| {
        if number.abs() < 1_000_000_000_000.0 {
            (number * 1000.0) as i64
        } else {
            number as i64
        }
    })
}

fn visible_project(project: String, include_project: bool) -> String {
    if include_project {
        project
    } else {
        "unknown".into()
    }
}

fn project_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .into()
}

fn project_from_bucket(name: &str) -> String {
    let stripped = name.strip_prefix("wd_").unwrap_or(name);
    stripped
        .rsplit_once('_')
        .filter(|(_, suffix)| {
            suffix
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        })
        .map(|(project, _)| project)
        .unwrap_or(stripped)
        .into()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}
