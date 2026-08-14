use super::{extract_sessions, provider_key, ParserOutput, SessionEvent};
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

// 6：2026-08-04 活跃算法改为 gap 切断 + 回合上限，旧缓存（算法 5）必须失效重算。
const CACHE_VERSION: u32 = 6;
const CONFIG_MODIFIED_KEY: &str = "__config_modified_ms";
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
        let provider_config = load_provider_config(&self.codex_home);
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
        let session_map = selected.iter().filter_map(|header| header.session_id.clone().map(|id| (id, header.clone()))).collect::<HashMap<_,_>>();
        let mut output = ParserOutput {
            files_scanned: selected.len(),
            ..ParserOutput::default()
        };
        let mut buckets = BTreeMap::<BucketKey, UsageBucket>::new();
        let mut sessions = Vec::new();

        for header in selected {
            let parsed = match self.load_cache(&header, hostname, include_project, &provider_config) {
                Some(cached) => cached,
                None => {
                    let parsed = parse_file(&header, replay_boundary(&header, &session_map), hostname, include_project, &provider_config)?;
                    let _ = self.save_cache(&header, hostname, include_project, &provider_config, &parsed);
                    parsed
                }
            };
            output.usage_records += parsed.usage_records;
            output.malformed_lines += parsed.malformed_lines + header.malformed_lines;
            merge_buckets(&mut buckets, parsed.buckets);
            sessions.extend(parsed.sessions);
        }

        output.buckets = buckets.into_values().collect();
        output.sessions = sessions;
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
        provider_config: &BTreeMap<String, String>,
    ) -> Option<ParserOutput> {
        let raw = fs::read(self.cache_path(&header.path)).ok()?;
        let cached = serde_json::from_slice::<CacheEntry>(&raw).ok()?;
        (cached.version == CACHE_VERSION
            && cached.path == header.path
            && cached.size == header.size
            && cached.modified_ms == header.modified_ms
            && cached.hostname == hostname
            && cached.include_project == include_project)
            .then_some(())
            .filter(|_| &cached.provider_config == provider_config)
            .map(|_| cached.output)
    }

    fn save_cache(
        &self,
        header: &FileHeader,
        hostname: &str,
        include_project: bool,
        provider_config: &BTreeMap<String, String>,
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
            provider_config: provider_config.clone(),
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
    session_meta_count: usize,
    parsed_record_count: usize,
    malformed_lines: usize,
    token_times: Vec<i64>,
    token_fingerprints: Vec<String>,
    task_boundaries: Vec<TaskBoundary>,
    first_task_boundary: Option<TaskBoundary>,
    own_task_boundary: Option<TaskBoundary>,
}

#[derive(Debug, Clone, Copy)]
struct TaskBoundary { record_index: usize, raw_token_count: usize, started_at_ms: Option<i64> }
#[derive(Debug, Clone, Copy, Default)]
struct ReplayBoundary { record_index: Option<usize>, raw_token_count: usize }

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    version: u32,
    path: PathBuf,
    size: u64,
    modified_ms: u64,
    hostname: String,
    include_project: bool,
    provider_config: BTreeMap<String, String>,
    output: ParserOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BucketKey {
    bucket_start: String,
    provider: String,
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
        self.input + self.output + self.cached + self.reasoning
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
        session_meta_count: 0,
        parsed_record_count: 0,
        malformed_lines: 0,
        token_times: Vec::new(),
        token_fingerprints: Vec::new(),
        task_boundaries: Vec::new(),
        first_task_boundary: None,
        own_task_boundary: None,
    };
    let mut raw_token_count = 0usize;
    let mut logical_timestamp = i64::MIN;
    let mut pending_token_times = Vec::new();
    let reader = BufReader::new(File::open(&header.path)?).take(header.size);
    for line in reader.lines() {
        let line = match line { Ok(line) => line, Err(_) => { header.malformed_lines += 1; continue; } };
        let Ok(record) = serde_json::from_str::<Value>(&line) else { continue };
        header.parsed_record_count += 1;
        let record_timestamp = string_at(&record, "/timestamp").and_then(timestamp_ms);
        if let Some(timestamp) = record_timestamp {
            logical_timestamp = logical_timestamp.max(timestamp);
            for index in pending_token_times.drain(..) { header.token_times[index] = logical_timestamp; }
        }
        if record.get("type").and_then(Value::as_str) == Some("session_meta") {
            header.session_meta_count += 1;
            if header.session_meta_count == 1 {
                let Some(meta) = record.get("payload") else { continue };
                header.session_id = string_at(meta, "/id").map(str::to_owned);
                header.forked_from_id = string_at(meta, "/forked_from_id").map(str::to_owned);
                header.parent_id = string_at(meta, "/parent_thread_id").or_else(|| string_at(meta, "/source/subagent/thread_spawn/parent_thread_id")).map(str::to_owned);
                header.is_subagent = is_subagent(meta); header.project = extract_project(meta);
                header.started_ms = string_at(meta, "/timestamp").and_then(timestamp_ms).or(record_timestamp);
            }
        } else if record.pointer("/payload/type").and_then(Value::as_str) == Some("token_count") && record.get("type").and_then(Value::as_str) == Some("event_msg") {
            raw_token_count += 1;
            let payload = record.get("payload").unwrap_or(&Value::Null);
            header.token_fingerprints.push(hex::encode(Sha256::digest(serde_json::to_vec(payload).unwrap_or_default()))[..16].to_string());
            if record_timestamp.is_some() { header.token_times.push(logical_timestamp); } else { header.token_times.push(i64::MAX); pending_token_times.push(header.token_times.len()-1); }
        } else if record.get("type").and_then(Value::as_str) == Some("event_msg") && matches!(record.pointer("/payload/type").and_then(Value::as_str), Some("task_started" | "turn_started")) {
            let boundary = TaskBoundary { record_index: header.parsed_record_count, raw_token_count, started_at_ms: record.pointer("/payload/started_at").and_then(epoch_ms) };
            header.task_boundaries.push(boundary); if header.first_task_boundary.is_none() { header.first_task_boundary = Some(boundary); }
            if header.started_ms.zip(boundary.started_at_ms).is_some_and(|(started, boundary)| (boundary-started).abs() <= OWN_TASK_START_WINDOW_MS) { header.own_task_boundary = Some(boundary); }
        }
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
            Some(existing) if existing.parsed_record_count >= header.parsed_record_count => {}
            _ => {
                selected.insert(key, header);
            }
        }
    }
    let mut values = selected.into_values().collect::<Vec<_>>();
    values.sort_by(|left, right| left.path.cmp(&right.path));
    values
}

fn upper_bound(sorted: &[i64], target: i64) -> usize {
    let (mut low, mut high) = (0, sorted.len());
    while low < high { let mid = low + (high-low)/2; if sorted[mid] <= target { low=mid+1 } else { high=mid } }
    low
}

fn longest_replay_prefix(child: &[String], parent: &[String]) -> usize {
    if child.is_empty() || parent.is_empty() { return 0; }
    let mut prefix=vec![0usize;child.len()]; let mut matched=0;
    for index in 1..child.len(){while matched>0&&child[index]!=child[matched]{matched=prefix[matched-1];}if child[index]==child[matched]{matched+=1;}prefix[index]=matched;}
    matched=0;
    for (index,fingerprint) in parent.iter().enumerate(){while matched>0&&fingerprint!=&child[matched]{matched=prefix[matched-1];}if fingerprint==&child[matched]{matched+=1;}if matched==child.len()&&index<parent.len()-1{matched=prefix[matched-1];}}
    matched
}

fn replay_boundary(header: &FileHeader, sessions: &HashMap<String, FileHeader>) -> ReplayBoundary {
    let parent_id = header.forked_from_id.as_ref().or_else(|| header.is_subagent.then_some(()).and_then(|_| header.parent_id.as_ref()));
    let replay_count = parent_id.and_then(|id| sessions.get(id)).and_then(|parent| header.started_ms.map(|started| (parent,upper_bound(&parent.token_times,started)))).map(|(parent,count)| longest_replay_prefix(&header.token_fingerprints,&parent.token_fingerprints[..count])).unwrap_or(0);
    if header.is_subagent {
        let matched = (replay_count>0).then(|| header.task_boundaries.iter().rev().find(|boundary| boundary.raw_token_count==replay_count && boundary.started_at_ms.zip(header.started_ms).is_some_and(|(boundary,started)| boundary >= started.div_euclid(1000)*1000)).copied()).flatten();
        let direct = matched.or(header.own_task_boundary).or_else(|| (header.session_meta_count==1&&header.forked_from_id.is_none()).then_some(header.first_task_boundary).flatten());
        if let Some(boundary)=direct { return ReplayBoundary { raw_token_count: replay_count.max(boundary.raw_token_count), record_index: Some(boundary.record_index) }; }
    }
    ReplayBoundary { raw_token_count: replay_count, record_index: None }
}

fn parse_file(
    header: &FileHeader,
    boundary: ReplayBoundary,
    hostname: &str,
    include_project: bool,
    provider_config: &BTreeMap<String, String>,
) -> io::Result<ParserOutput> {
    let reader = BufReader::new(File::open(&header.path)?).take(header.size);
    let mut output = ParserOutput {
        files_scanned: 1,
        ..ParserOutput::default()
    };
    let mut buckets = BTreeMap::<BucketKey, UsageBucket>::new();
    let mut model = "unknown".to_string();
    let mut provider = "codex-provider:unknown".to_string();
    let mut previous_total = None::<TokenTotals>;
    let mut previous_cumulative_total = None::<i64>;
    let mut raw_token_seen = 0usize;
    let mut parsed_record_index = 0usize;
    let mut session_events = Vec::new();
    let session_id = header.session_id.clone().unwrap_or_else(|| header.path.to_string_lossy().into_owned());
    let project = if include_project { header.project.clone() } else { "unknown".into() };
    let mut first_session_meta_seen = false;

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => { output.malformed_lines += 1; continue; }
        };
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
        parsed_record_index += 1;
        let in_replay = boundary.record_index.is_some_and(|index| parsed_record_index < index) || raw_token_seen < boundary.raw_token_count;

        let record_type = record.get("type").and_then(Value::as_str);
        let payload_type = record.pointer("/payload/type").and_then(Value::as_str);
        let is_session_meta = record_type == Some("session_meta");
        let is_canonical_meta = is_session_meta && !first_session_meta_seen;
        let is_own_meta = is_session_meta && string_at(&record, "/payload/id").is_some_and(|id| header.session_id.as_deref() == Some(id));
        if is_session_meta { first_session_meta_seen = true; }
        if is_canonical_meta {
            if let Some(name) = string_at(&record, "/payload/model_provider") {
                provider = resolve_provider(provider_config, name, header.started_ms);
            }
        }
        let is_heartbeat = record_type == Some("event_msg")
            && matches!(payload_type, Some("token_count" | "agent_reasoning"));
        let keep_event = if is_session_meta { is_canonical_meta || (is_own_meta && !in_replay) } else { !in_replay && !is_heartbeat };
        if keep_event {
          if let Some(timestamp) = string_at(&record, "/timestamp").and_then(timestamp_ms) {
            session_events.push(SessionEvent {
                session_id: session_id.clone(), source: "codex".into(), project: project.clone(),
                hostname: hostname.into(), timestamp_ms: timestamp,
                is_user: matches!(record_type, Some("turn_context" | "session_meta")),
            });
          }
        }

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
        if payload_type != Some("token_count") {
            continue;
        }
        let is_replayed_history = in_replay;
        raw_token_seen += 1;
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
        if is_replayed_history { continue; }
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
            &provider,
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
    output.sessions = extract_sessions(session_events);
    Ok(output)
}

fn add_bucket(
    buckets: &mut BTreeMap<BucketKey, UsageBucket>,
    timestamp_ms: i64,
    model: &str,
    provider: &str,
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
        provider: provider.into(),
        model: model.into(),
        project: project.into(),
        hostname: hostname.into(),
    };
    let bucket = buckets.entry(key).or_insert_with(|| UsageBucket {
        source: "codex".into(),
        provider: provider.into(),
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
            provider: bucket.provider.clone(),
            model: bucket.model.clone(),
            project: bucket.project.clone(),
            hostname: bucket.hostname.clone(),
        };
        let current = target.entry(key).or_insert_with(|| UsageBucket {
            source: "codex".into(),
            provider: bucket.provider.clone(),
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

fn load_provider_config(root: &Path) -> BTreeMap<String, String> {
    let mut providers = BTreeMap::new();
    let config_path = root.join("config.toml");
    let Some(value) = fs::read_to_string(&config_path).ok()
        .and_then(|raw| raw.parse::<toml::Value>().ok()) else { return providers };
    if let Some(modified_ms) = fs::metadata(&config_path).ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().to_string())
    {
        providers.insert(CONFIG_MODIFIED_KEY.into(), modified_ms);
    }
    let Some(table) = value.get("model_providers").and_then(toml::Value::as_table) else { return providers };
    for (name, config) in table {
        let fallback = format!("codex-provider:{}", name.to_ascii_lowercase());
        let endpoint = config.get("base_url").and_then(toml::Value::as_str);
        let key = if endpoint.is_none() && name.eq_ignore_ascii_case("openai") {
            "api.openai.com".to_string()
        } else {
            provider_key(endpoint, &fallback)
        };
        providers.insert(name.to_ascii_lowercase(), key);
    }
    providers
}

fn resolve_provider(
    providers: &BTreeMap<String, String>,
    name: &str,
    session_started_ms: Option<i64>,
) -> String {
    let fallback = || format!("codex-provider:{}", name.to_ascii_lowercase());
    let Some(config_modified_ms) = providers.get(CONFIG_MODIFIED_KEY)
        .and_then(|value| value.parse::<i64>().ok()) else { return fallback() };
    if !session_started_ms.is_some_and(|started| started >= config_modified_ms.saturating_sub(5_000)) {
        return fallback();
    }
    providers.get(&name.to_ascii_lowercase()).cloned().unwrap_or_else(fallback)
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

#[cfg(test)]
mod provider_tests {
    use super::*;

    #[test]
    fn only_applies_current_provider_config_to_sessions_created_after_it() {
        let providers = BTreeMap::from([
            (CONFIG_MODIFIED_KEY.into(), "20000".into()),
            ("openai".into(), "free.example.com".into()),
        ]);

        assert_eq!(resolve_provider(&providers, "OpenAI", Some(20_000)), "free.example.com");
        assert_eq!(resolve_provider(&providers, "OpenAI", Some(10_000)), "codex-provider:openai");
        assert_eq!(resolve_provider(&providers, "OpenAI", None), "codex-provider:openai");
    }
}
