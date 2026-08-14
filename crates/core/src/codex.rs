use super::ParserOutput;
use crate::{config::atomic_write, usage::UsageBucket};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const CACHE_VERSION: u32 = 1;
const HALF_HOUR_MS: i64 = 30 * 60 * 1000;
const OWN_TASK_START_WINDOW_MS: i64 = 5_000;

pub struct CodexParser {
    codex_home: PathBuf,
    cache_dir: PathBuf,
}

impl CodexParser {
    pub fn new(codex_home: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            codex_home: codex_home.into(),
            cache_dir: cache_dir.into(),
        }
    }

    pub fn parse(&self, hostname: &str, include_project: bool) -> io::Result<ParserOutput> {
        let mut files = Vec::new();
        for directory in [
            self.codex_home.join("sessions"),
            self.codex_home.join("archived_sessions"),
        ] {
            files.extend(find_jsonl_files(&directory)?);
        }

        let mut headers = Vec::new();
        for path in files {
            if let Ok(metadata) = fs::metadata(&path) {
                if metadata.len() == 0 {
                    continue;
                }
                headers.push(read_header(path, &metadata)?);
            }
        }
        let selected = select_physical_files(headers);
        let mut output = ParserOutput {
            files_scanned: selected.len(),
            ..ParserOutput::default()
        };
        let mut buckets = BTreeMap::<BucketKey, UsageBucket>::new();

        for header in selected {
            let parsed = match self.load_cache(&header, hostname, include_project) {
                Some(cached) => cached,
                None => {
                    let parsed = parse_file(&header, hostname, include_project)?;
                    let _ = self.save_cache(&header, hostname, include_project, &parsed);
                    parsed
                }
            };
            output.usage_records += parsed.usage_records;
            output.malformed_lines += parsed.malformed_lines;
            merge_buckets(&mut buckets, parsed.buckets);
        }

        output.buckets = buckets.into_values().collect();
        Ok(output)
    }

    fn cache_path(&self, path: &Path) -> PathBuf {
        let digest = hex::encode(Sha256::digest(path.to_string_lossy().as_bytes()));
        self.cache_dir.join(format!("{}.json", &digest[..32]))
    }

    fn load_cache(
        &self,
        header: &FileHeader,
        hostname: &str,
        include_project: bool,
    ) -> Option<ParserOutput> {
        let raw = fs::read(self.cache_path(&header.path)).ok()?;
        let cached = serde_json::from_slice::<CacheEntry>(&raw).ok()?;
        (cached.version == CACHE_VERSION
            && cached.path == header.path
            && cached.size == header.size
            && cached.modified_ms == header.modified_ms
            && cached.hostname == hostname
            && cached.include_project == include_project)
            .then_some(cached.output)
    }

    fn save_cache(
        &self,
        header: &FileHeader,
        hostname: &str,
        include_project: bool,
        output: &ParserOutput,
    ) -> io::Result<()> {
        let target = self.cache_path(&header.path);
        let data = serde_json::to_vec(&CacheEntry {
            version: CACHE_VERSION,
            path: header.path.clone(),
            size: header.size,
            modified_ms: header.modified_ms,
            hostname: hostname.into(),
            include_project,
            output: output.clone(),
        })?;
        atomic_write(&target, &data)
    }
}

#[derive(Debug, Clone)]
struct FileHeader {
    path: PathBuf,
    size: u64,
    modified_ms: u64,
    session_id: Option<String>,
    parent_id: Option<String>,
    forked_from_id: Option<String>,
    is_subagent: bool,
    project: String,
    started_ms: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    version: u32,
    path: PathBuf,
    size: u64,
    modified_ms: u64,
    hostname: String,
    include_project: bool,
    output: ParserOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BucketKey {
    bucket_start: String,
    model: String,
    project: String,
    hostname: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct TokenTotals {
    input: i64,
    output: i64,
    cached: i64,
    reasoning: i64,
}

impl TokenTotals {
    fn from_value(value: &Value) -> Self {
        Self {
            input: token(value, &["input_tokens", "prompt_tokens"]),
            output: token(value, &["output_tokens", "completion_tokens"]),
            cached: token(
                value,
                &[
                    "cached_input_tokens",
                    "cache_read_input_tokens",
                    "prompt_cache_hit_tokens",
                ],
            ),
            reasoning: token(value, &["reasoning_output_tokens"]),
        }
    }

    fn delta(self, previous: Self) -> Option<Self> {
        let delta = Self {
            input: self.input - previous.input,
            output: self.output - previous.output,
            cached: self.cached - previous.cached,
            reasoning: self.reasoning - previous.reasoning,
        };
        (delta.input >= 0 && delta.output >= 0 && delta.cached >= 0 && delta.reasoning >= 0)
            .then_some(delta)
    }

    fn cumulative_total(self) -> i64 {
        self.input + self.output
    }
}

fn read_header(path: PathBuf, metadata: &fs::Metadata) -> io::Result<FileHeader> {
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0);
    let mut header = FileHeader {
        path,
        size: metadata.len(),
        modified_ms,
        session_id: None,
        parent_id: None,
        forked_from_id: None,
        is_subagent: false,
        project: "unknown".into(),
        started_ms: None,
    };
    let reader = BufReader::new(File::open(&header.path)?).take(header.size);
    for line in reader.lines() {
        let Ok(record) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        if record.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }
        let Some(meta) = record.get("payload") else {
            continue;
        };
        header.session_id = string_at(meta, "/id").map(str::to_owned);
        header.forked_from_id = string_at(meta, "/forked_from_id").map(str::to_owned);
        header.parent_id = string_at(meta, "/parent_thread_id")
            .or_else(|| string_at(meta, "/source/subagent/thread_spawn/parent_thread_id"))
            .map(str::to_owned);
        header.is_subagent = is_subagent(meta);
        header.project = extract_project(meta);
        header.started_ms = string_at(meta, "/timestamp")
            .and_then(timestamp_ms)
            .or_else(|| string_at(&record, "/timestamp").and_then(timestamp_ms));
        break;
    }
    Ok(header)
}

fn select_physical_files(headers: Vec<FileHeader>) -> Vec<FileHeader> {
    let mut selected = HashMap::<String, FileHeader>::new();
    for header in headers {
        let key = header
            .session_id
            .clone()
            .unwrap_or_else(|| header.path.to_string_lossy().into_owned());
        match selected.get(&key) {
            Some(existing) if existing.size >= header.size => {}
            _ => {
                selected.insert(key, header);
            }
        }
    }
    let mut values = selected.into_values().collect::<Vec<_>>();
    values.sort_by(|left, right| left.path.cmp(&right.path));
    values
}

fn parse_file(
    header: &FileHeader,
    hostname: &str,
    include_project: bool,
) -> io::Result<ParserOutput> {
    let reader = BufReader::new(File::open(&header.path)?).take(header.size);
    let mut output = ParserOutput {
        files_scanned: 1,
        ..ParserOutput::default()
    };
    let mut buckets = BTreeMap::<BucketKey, UsageBucket>::new();
    let mut model = "unknown".to_string();
    let mut previous_total = None::<TokenTotals>;
    let mut previous_cumulative_total = None::<i64>;
    let replay_child = header.is_subagent || header.forked_from_id.is_some();
    let mut own_work = !replay_child;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record = match serde_json::from_str::<Value>(&line) {
            Ok(record) => record,
            Err(_) => {
                output.malformed_lines += 1;
                continue;
            }
        };

        if record.get("type").and_then(Value::as_str) == Some("turn_context") {
            if let Some(value) = string_at(&record, "/payload/model") {
                model = value.to_string();
            }
            continue;
        }
        if record.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        let Some(payload) = record.get("payload") else {
            continue;
        };
        let payload_type = payload.get("type").and_then(Value::as_str);
        if matches!(payload_type, Some("task_started" | "turn_started")) && replay_child {
            let boundary_ms = payload.get("started_at").and_then(epoch_ms);
            if header.started_ms.is_none()
                || boundary_ms
                    .zip(header.started_ms)
                    .is_some_and(|(boundary, started)| {
                        (boundary - started).abs() <= OWN_TASK_START_WINDOW_MS
                    })
            {
                own_work = true;
                previous_total = None;
                previous_cumulative_total = None;
            }
            continue;
        }
        if payload_type != Some("token_count") || !own_work {
            continue;
        }
        let Some(info) = payload.get("info") else {
            continue;
        };
        let current_total = info.get("total_token_usage").map(TokenTotals::from_value);
        let cumulative_total = current_total.map(TokenTotals::cumulative_total);
        let duplicate = cumulative_total.is_some_and(|total| {
            total > 0 && previous_cumulative_total.is_some_and(|previous| previous == total)
        });
        if let Some(total) = cumulative_total {
            previous_cumulative_total = Some(total);
        }
        let usage = info
            .get("last_token_usage")
            .map(TokenTotals::from_value)
            .or_else(|| {
                current_total.map(|current| {
                    previous_total
                        .and_then(|previous| current.delta(previous))
                        .unwrap_or(current)
                })
            });
        if let Some(current) = current_total {
            previous_total = Some(current);
        }
        let Some(usage) = usage else {
            continue;
        };
        if duplicate {
            continue;
        }
        let Some(timestamp) = string_at(&record, "/timestamp").and_then(timestamp_ms) else {
            continue;
        };
        let event_model = string_at(info, "/model")
            .or_else(|| string_at(payload, "/model"))
            .unwrap_or(&model)
            .to_string();
        let input = usage.input.saturating_sub(usage.cached);
        let output_tokens = usage.output.saturating_sub(usage.reasoning);
        if input == 0 && output_tokens == 0 && usage.cached == 0 && usage.reasoning == 0 {
            continue;
        }
        add_bucket(
            &mut buckets,
            timestamp,
            &event_model,
            if include_project {
                &header.project
            } else {
                "unknown"
            },
            hostname,
            TokenTotals {
                input,
                output: output_tokens,
                cached: usage.cached,
                reasoning: usage.reasoning,
            },
        );
        output.usage_records += 1;
    }

    output.buckets = buckets.into_values().collect();
    Ok(output)
}

fn add_bucket(
    buckets: &mut BTreeMap<BucketKey, UsageBucket>,
    timestamp_ms: i64,
    model: &str,
    project: &str,
    hostname: &str,
    tokens: TokenTotals,
) {
    let start_ms = timestamp_ms.div_euclid(HALF_HOUR_MS) * HALF_HOUR_MS;
    let Some(bucket_start) = DateTime::<Utc>::from_timestamp_millis(start_ms)
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
        source: "codex".into(),
        model: model.into(),
        project: project.into(),
        hostname: hostname.into(),
        bucket_start,
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        reasoning_output_tokens: 0,
        total_tokens: 0,
    });
    bucket.input_tokens += tokens.input;
    bucket.output_tokens += tokens.output;
    bucket.cached_input_tokens += tokens.cached;
    bucket.reasoning_output_tokens += tokens.reasoning;
    bucket.total_tokens = bucket.input_tokens
        + bucket.output_tokens
        + bucket.cached_input_tokens
        + bucket.reasoning_output_tokens;
}

fn merge_buckets(target: &mut BTreeMap<BucketKey, UsageBucket>, source: Vec<UsageBucket>) {
    for bucket in source {
        let key = BucketKey {
            bucket_start: bucket.bucket_start.clone(),
            model: bucket.model.clone(),
            project: bucket.project.clone(),
            hostname: bucket.hostname.clone(),
        };
        let current = target.entry(key).or_insert_with(|| UsageBucket {
            source: "codex".into(),
            model: bucket.model.clone(),
            project: bucket.project.clone(),
            hostname: bucket.hostname.clone(),
            bucket_start: bucket.bucket_start.clone(),
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            reasoning_output_tokens: 0,
            total_tokens: 0,
        });
        current.input_tokens += bucket.input_tokens;
        current.output_tokens += bucket.output_tokens;
        current.cached_input_tokens += bucket.cached_input_tokens;
        current.reasoning_output_tokens += bucket.reasoning_output_tokens;
        current.total_tokens += bucket.total_tokens;
    }
}

fn find_jsonl_files(root: &Path) -> io::Result<Vec<PathBuf>> {
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
            } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn is_subagent(meta: &Value) -> bool {
    string_at(meta, "/thread_source") == Some("subagent")
        || string_at(meta, "/source") == Some("subagent")
        || meta.pointer("/source/subagent").is_some()
        || meta
            .get("parent_thread_id")
            .is_some_and(|value| !value.is_null())
}

fn extract_project(meta: &Value) -> String {
    if let Some(repository) = string_at(meta, "/git/repository_url") {
        let normalized = repository.trim_end_matches(".git").trim_end_matches('/');
        let pieces = normalized.split('/').rev().take(2).collect::<Vec<_>>();
        if pieces.len() == 2 {
            return format!("{}/{}", pieces[1], pieces[0]);
        }
    }
    string_at(meta, "/cwd")
        .map(|path| {
            path.trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(path)
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn string_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn timestamp_ms(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.timestamp_millis())
}

fn epoch_ms(value: &Value) -> Option<i64> {
    if let Some(value) = value.as_i64() {
        return Some(if value.abs() < 1_000_000_000_000 {
            value * 1000
        } else {
            value
        });
    }
    value.as_str().and_then(|value| {
        value
            .parse::<i64>()
            .ok()
            .map(|number| {
                if number.abs() < 1_000_000_000_000 {
                    number * 1000
                } else {
                    number
                }
            })
            .or_else(|| timestamp_ms(value))
    })
}

fn token(value: &Value, names: &[&str]) -> i64 {
    names
        .iter()
        .find_map(|name| value.get(name))
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        })
        .unwrap_or(0)
        .max(0)
}
