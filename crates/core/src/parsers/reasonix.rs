use super::ParserOutput;
use crate::usage::UsageBucket;
use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const HALF_HOUR_MS: i64 = 30 * 60 * 1000;
const TELEMETRY_SUFFIX: &str = ".telemetry.json";

pub struct ReasonixParser {
    projects_root: PathBuf,
    cache_path: Option<PathBuf>,
}

impl ReasonixParser {
    pub fn new(projects_root: impl Into<PathBuf>) -> Self {
        Self {
            projects_root: projects_root.into(),
            cache_path: None,
        }
    }

    pub fn with_cache(projects_root: impl Into<PathBuf>, cache_path: impl Into<PathBuf>) -> Self {
        Self {
            projects_root: projects_root.into(),
            cache_path: Some(cache_path.into()),
        }
    }

    pub fn parse(&self, hostname: &str, include_project: bool) -> io::Result<ParserOutput> {
        let files = find_telemetry_files(&self.projects_root)?;
        let mut output = ParserOutput {
            files_scanned: files.len(),
            ..ParserOutput::default()
        };
        let mut snapshots = BTreeMap::<String, ParsedSnapshot>::new();

        for path in files {
            match parse_telemetry(&path, hostname, include_project) {
                Ok(Some(snapshot)) => {
                    let key = telemetry_chain_key(&path);
                    match snapshots.get(&key) {
                        Some(current) if current.observed_ms > snapshot.observed_ms => {}
                        _ => {
                            snapshots.insert(key, snapshot);
                        }
                    }
                }
                Ok(None) => {}
                Err(_) => output.malformed_lines += 1,
            }
        }

        output.buckets = if let Some(cache_path) = &self.cache_path {
            incremental_buckets(cache_path, snapshots)?
        } else {
            snapshots.into_values().map(|snapshot| snapshot.bucket).collect()
        };
        output.usage_records = output.buckets.len();
        output.buckets.sort_by(|left, right| {
            left.bucket_start
                .cmp(&right.bucket_start)
                .then_with(|| left.model.cmp(&right.model))
                .then_with(|| left.project.cmp(&right.project))
        });
        Ok(output)
    }
}

struct ParsedSnapshot {
    bucket: UsageBucket,
    observed_ms: i64,
    last: TokenCounters,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TokenCounters {
    input: i64,
    output: i64,
    cached: i64,
    reasoning: i64,
}

impl TokenCounters {
    fn from_bucket(bucket: &UsageBucket) -> Self {
        Self {
            input: bucket.input_tokens,
            output: bucket.output_tokens,
            cached: bucket.cached_input_tokens,
            reasoning: bucket.reasoning_output_tokens,
        }
    }

    fn total(&self) -> i64 {
        self.input + self.output + self.cached + self.reasoning
    }

    fn delta_from(&self, previous: &Self) -> Self {
        if self.input < previous.input
            || self.output < previous.output
            || self.cached < previous.cached
            || self.reasoning < previous.reasoning
        {
            return self.clone();
        }
        Self {
            input: self.input - previous.input,
            output: self.output - previous.output,
            cached: self.cached - previous.cached,
            reasoning: self.reasoning - previous.reasoning,
        }
    }

    fn subtract(&self, value: &Self) -> Self {
        Self {
            input: self.input.saturating_sub(value.input),
            output: self.output.saturating_sub(value.output),
            cached: self.cached.saturating_sub(value.cached),
            reasoning: self.reasoning.saturating_sub(value.reasoning),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct IncrementalState {
    chains: BTreeMap<String, CachedChain>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedChain {
    counters: TokenCounters,
    buckets: Vec<UsageBucket>,
}

fn bucket_for(snapshot: &ParsedSnapshot, counters: &TokenCounters, bucket_start: String) -> UsageBucket {
    UsageBucket {
        source: snapshot.bucket.source.clone(),
        provider: snapshot.bucket.provider.clone(),
        model: snapshot.bucket.model.clone(),
        project: snapshot.bucket.project.clone(),
        hostname: snapshot.bucket.hostname.clone(),
        bucket_start,
        input_tokens: counters.input,
        output_tokens: counters.output,
        cached_input_tokens: counters.cached,
        reasoning_output_tokens: counters.reasoning,
        total_tokens: counters.total(),
    }
}

fn same_bucket(left: &UsageBucket, right: &UsageBucket) -> bool {
    left.source == right.source
        && left.provider == right.provider
        && left.model == right.model
        && left.project == right.project
        && left.hostname == right.hostname
        && left.bucket_start == right.bucket_start
}

fn merge_bucket(buckets: &mut Vec<UsageBucket>, incoming: UsageBucket) {
    if incoming.total_tokens <= 0 {
        return;
    }
    if let Some(bucket) = buckets.iter_mut().find(|bucket| same_bucket(bucket, &incoming)) {
        bucket.input_tokens += incoming.input_tokens;
        bucket.output_tokens += incoming.output_tokens;
        bucket.cached_input_tokens += incoming.cached_input_tokens;
        bucket.reasoning_output_tokens += incoming.reasoning_output_tokens;
        bucket.total_tokens = bucket.input_tokens + bucket.output_tokens
            + bucket.cached_input_tokens + bucket.reasoning_output_tokens;
    } else {
        buckets.push(incoming);
    }
}

fn incremental_buckets(
    cache_path: &Path,
    snapshots: BTreeMap<String, ParsedSnapshot>,
) -> io::Result<Vec<UsageBucket>> {
    let mut state = fs::read(cache_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<IncrementalState>(&bytes).ok())
        .unwrap_or_default();
    let mut next = IncrementalState::default();

    for (key, snapshot) in snapshots {
        let current = TokenCounters::from_bucket(&snapshot.bucket);
        let mut buckets = state.chains.remove(&key).map(|chain| {
            let delta = current.delta_from(&chain.counters);
            let mut buckets = chain.buckets;
            if delta.total() > 0 {
                if let Some(observed) = half_hour(snapshot.observed_ms) {
                    merge_bucket(&mut buckets, bucket_for(&snapshot, &delta, observed));
                }
            }
            buckets
        }).unwrap_or_else(|| {
            let mut buckets = Vec::new();
            let observed = half_hour(snapshot.observed_ms).unwrap_or_else(|| snapshot.bucket.bucket_start.clone());
            let recent = snapshot.last.clone();
            if recent.total() > 0 && recent.total() <= current.total() && observed != snapshot.bucket.bucket_start {
                merge_bucket(&mut buckets, bucket_for(&snapshot, &current.subtract(&recent), snapshot.bucket.bucket_start.clone()));
                merge_bucket(&mut buckets, bucket_for(&snapshot, &recent, observed));
            } else {
                merge_bucket(&mut buckets, snapshot.bucket.clone());
            }
            buckets
        });
        buckets.sort_by(|left, right| left.bucket_start.cmp(&right.bucket_start));
        next.chains.insert(key, CachedChain { counters: current, buckets });
    }

    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(cache_path, serde_json::to_vec_pretty(&next).map_err(io::Error::other)?)?;
    Ok(next.chains.into_values().flat_map(|chain| chain.buckets).collect())
}

fn parse_telemetry(
    telemetry_path: &Path,
    hostname: &str,
    include_project: bool,
) -> io::Result<Option<ParsedSnapshot>> {
    let telemetry = read_json(telemetry_path)?;
    let Some(usage) = telemetry.get("usage") else {
        return Ok(None);
    };

    let prompt = token(usage, "promptTokens");
    let cache_hit = token(usage, "cacheHitTokens");
    let cache_miss = token(usage, "cacheMissTokens");
    let completion = token(usage, "completionTokens");
    let reasoning = token(usage, "reasoningTokens").min(completion);
    let has_cache_split = cache_hit > 0 || cache_miss > 0;
    let input = if has_cache_split { cache_miss } else { prompt };
    let cached = if has_cache_split { cache_hit } else { 0 };
    let output = completion.saturating_sub(reasoning);
    let total = input + cached + output + reasoning;
    if total == 0 {
        return Ok(None);
    }

    let metadata_path = metadata_path(telemetry_path);
    let metadata = read_json(&metadata_path).ok();
    let model = metadata
        .as_ref()
        .and_then(|value| value.get("model"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| model_from_filename(telemetry_path));
    // bucket_start 使用链创建时间（来自文件名,稳定）,而不是 updated_at。
    // reasonix 的 telemetry 是会话级累计快照,updated_at 每次扫描都会漂移,
    // 若用它做 bucket_start,同一会话链每次扫描会落入不同的半小时桶,
    // 导致同一累计值在数据库中被重复插入（identity 含 bucket_start）。
    let chain_start = chain_start_ms(telemetry_path).unwrap_or_else(|| {
        metadata
            .as_ref()
            .and_then(|value| value.get("updated_at"))
            .and_then(Value::as_str)
            .and_then(timestamp_ms)
            .or_else(|| modified_ms(telemetry_path))
            .unwrap_or(0)
    });
    // observed_ms 用于快照新旧比较,必须用真正的更新时间（updated_at/文件修改时间）,不能用链创建时间。
    let observed_ms = metadata
        .as_ref()
        .and_then(|value| value.get("updated_at"))
        .and_then(Value::as_str)
        .and_then(timestamp_ms)
        .or_else(|| modified_ms(telemetry_path))
        .unwrap_or(0);
    let bucket_start = half_hour(chain_start)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid Reasonix timestamp"))?;
    let project = if include_project {
        project_name(telemetry_path)
    } else {
        "unknown".into()
    };
    let last_cache_hit = token(usage, "lastCacheHitTokens");
    let last_cache_miss = token(usage, "lastCacheMissTokens");
    let last_prompt = token(usage, "lastPromptTokens");
    let last_completion = token(usage, "lastCompletionTokens");
    let last = TokenCounters {
        input: if last_cache_hit > 0 || last_cache_miss > 0 { last_cache_miss } else { last_prompt },
        output: last_completion,
        cached: last_cache_hit,
        reasoning: 0,
    };

    Ok(Some(ParsedSnapshot {
        observed_ms,
        last,
        bucket: UsageBucket {
            source: "reasonix".into(),
            provider: provider_from_model(&model),
            model,
            project,
            hostname: hostname.into(),
            bucket_start,
            input_tokens: input,
            output_tokens: output,
            cached_input_tokens: cached,
            reasoning_output_tokens: reasoning,
            total_tokens: total,
        },
    }))
}

fn read_json(path: &Path) -> io::Result<Value> {
    let raw = fs::read(path)?;
    serde_json::from_slice(&raw).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn token(value: &Value, name: &str) -> i64 {
    value
        .get(name)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
        })
        .unwrap_or(0)
        .max(0)
}

fn metadata_path(telemetry_path: &Path) -> PathBuf {
    let name = telemetry_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .strip_suffix(TELEMETRY_SUFFIX)
        .unwrap_or_default();
    telemetry_path.with_file_name(format!("{name}.meta"))
}

fn telemetry_chain_key(path: &Path) -> String {
    let directory = path.parent().unwrap_or_else(|| Path::new(""));
    directory
        .join(telemetry_chain_root(path))
        .to_string_lossy()
        .into_owned()
}

fn telemetry_chain_root(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .strip_suffix(TELEMETRY_SUFFIX)
        .unwrap_or_default()
        .strip_suffix(".jsonl")
        .unwrap_or_default();
    let Some(recovery) = name.rfind("-recovery-") else {
        return name.to_string();
    };
    let recovery_id = &name[recovery + "-recovery-".len()..];
    if !is_hex_id(recovery_id) {
        return name.to_string();
    }

    let mut root = &name[..recovery];
    // 循环剥离 12 位十六进制分支段:recovery 链可嵌套多层
    // （如 ...-deepseek-v4-flash-19c3cf462078-c823fa29cc46-recovery-84e1f...）,只剥一层会把
    // 同一链拆成两条不同 identity 的记录。
    loop {
        let Some((prefix, branch_id)) = root.rsplit_once('-') else { break };
        if branch_id.len() == 12 && is_hex_id(branch_id) {
            root = prefix;
        } else {
            break;
        }
    }
    root.to_string()
}

fn is_hex_id(value: &str) -> bool {
    value.len() >= 8 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn model_from_filename(path: &Path) -> String {
    telemetry_chain_root(path)
        .splitn(3, '-')
        .nth(2)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

/// 从文件名提取会话链创建时间（UTC 毫秒）。文件名形如
/// `20260806-045103.602016100-deepseek-deepseek-v4-flash` 或
/// `20260806-041734.612787600-session`,前 15 字符是 `YYYYMMDD-HHMMSS`。
fn chain_start_ms(path: &Path) -> Option<i64> {
    let root = telemetry_chain_root(path);
    let stamp = root.get(..15)?;
    let year: i32 = stamp.get(0..4)?.parse().ok()?;
    let month: u32 = stamp.get(4..6)?.parse().ok()?;
    let day: u32 = stamp.get(6..8)?.parse().ok()?;
    let hour: u32 = stamp.get(9..11)?.parse().ok()?;
    let minute: u32 = stamp.get(11..13)?.parse().ok()?;
    let second: u32 = stamp.get(13..15)?.parse().ok()?;
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .map(|value| value.timestamp_millis())
}

fn project_name(path: &Path) -> String {
    path.parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn provider_from_model(model: &str) -> String {
    model
        .split_once('/')
        .map(|(provider, _)| provider.trim())
        .filter(|provider| !provider.is_empty())
        .unwrap_or("reasonix")
        .to_string()
}

fn timestamp_ms(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.timestamp_millis())
}

fn modified_ms(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_millis()).ok())
}

fn half_hour(timestamp_ms: i64) -> Option<String> {
    let start = timestamp_ms.div_euclid(HALF_HOUR_MS) * HALF_HOUR_MS;
    DateTime::<Utc>::from_timestamp_millis(start)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn find_telemetry_files(root: &Path) -> io::Result<Vec<PathBuf>> {
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
            } else if path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with(TELEMETRY_SUFFIX))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixture(usage: &str) -> (tempfile::TempDir, PathBuf) {
        let root = tempdir().unwrap();
        let sessions = root.path().join("sample-project").join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let telemetry = sessions.join("session.jsonl.telemetry.json");
        fs::write(
            &telemetry,
            format!(r#"{{"version":2,"usage":{usage}}}"#),
        )
        .unwrap();
        fs::write(
            sessions.join("session.jsonl.meta"),
            r#"{"updated_at":"2026-08-03T03:43:41.653Z","model":"deepseek-flash/deepseek-v4-flash"}"#,
        )
        .unwrap();
        (root, telemetry)
    }

    fn write_snapshot(sessions: &Path, name: &str, updated_at: &str, total: i64) {
        let telemetry = sessions.join(format!("{name}.jsonl.telemetry.json"));
        fs::write(
            telemetry,
            format!(
                r#"{{"version":2,"usage":{{"promptTokens":{total},"completionTokens":0}}}}"#
            ),
        )
        .unwrap();
        fs::write(
            sessions.join(format!("{name}.jsonl.meta")),
            format!(
                r#"{{"updated_at":"{updated_at}","model":"deepseek-flash/deepseek-v4-flash"}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn maps_reasonix_cache_and_reasoning_without_double_counting() {
        let (root, _) = fixture(
            r#"{"promptTokens":1000,"completionTokens":200,"reasoningTokens":80,"cacheHitTokens":700,"cacheMissTokens":300}"#,
        );
        let output = ReasonixParser::new(root.path()).parse("host", true).unwrap();
        assert_eq!(output.usage_records, 1);
        let bucket = &output.buckets[0];
        assert_eq!(bucket.source, "reasonix");
        assert_eq!(bucket.provider, "deepseek-flash");
        assert_eq!(bucket.model, "deepseek-flash/deepseek-v4-flash");
        assert_eq!(bucket.project, "sample-project");
        assert_eq!(bucket.bucket_start, "2026-08-03T03:30:00.000Z");
        assert_eq!(bucket.input_tokens, 300);
        assert_eq!(bucket.cached_input_tokens, 700);
        assert_eq!(bucket.output_tokens, 120);
        assert_eq!(bucket.reasoning_output_tokens, 80);
        assert_eq!(bucket.total_tokens, 1_200);
    }

    #[test]
    fn uses_prompt_tokens_when_cache_breakdown_is_absent() {
        let (root, _) = fixture(r#"{"promptTokens":900,"completionTokens":100}"#);
        let bucket = ReasonixParser::new(root.path())
            .parse("host", false)
            .unwrap()
            .buckets
            .remove(0);
        assert_eq!(bucket.input_tokens, 900);
        assert_eq!(bucket.cached_input_tokens, 0);
        assert_eq!(bucket.project, "unknown");
        assert_eq!(bucket.total_tokens, 1_000);
    }

    #[test]
    fn skips_zero_usage_and_counts_malformed_telemetry() {
        let (root, telemetry) = fixture(r#"{"promptTokens":0,"completionTokens":0}"#);
        let sessions = telemetry.parent().unwrap();
        fs::write(sessions.join("broken.jsonl.telemetry.json"), b"not json").unwrap();
        let output = ReasonixParser::new(root.path()).parse("host", true).unwrap();
        assert!(output.buckets.is_empty());
        assert_eq!(output.malformed_lines, 1);
    }

    #[test]
    fn keeps_only_the_latest_snapshot_from_a_recovery_chain() {
        let root = tempdir().unwrap();
        let sessions = root.path().join("sample-project").join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let base = "20260803-030108.307015100-deepseek-v4-flash";
        write_snapshot(&sessions, base, "2026-08-03T03:40:00Z", 4_218_770);
        write_snapshot(
            &sessions,
            &format!("{base}-recovery-9b85feb58bc54240"),
            "2026-08-03T03:45:20Z",
            4_346_186,
        );
        write_snapshot(
            &sessions,
            &format!("{base}-83a14754a585-recovery-3239b298f972d8a8"),
            "2026-08-03T03:45:21Z",
            4_346_186,
        );

        let output = ReasonixParser::new(root.path()).parse("host", true).unwrap();
        assert_eq!(output.files_scanned, 3);
        assert_eq!(output.usage_records, 1);
        assert_eq!(output.buckets.len(), 1);
        assert_eq!(output.buckets[0].total_tokens, 4_346_186);
    }

    #[test]
    fn falls_back_to_the_base_filename_when_metadata_is_invalid() {
        let root = tempdir().unwrap();
        let sessions = root.path().join("sample-project").join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let name = "20260803-030108.307015100-deepseek-v4-flash-recovery-9b85feb58bc54240";
        let telemetry = sessions.join(format!("{name}.jsonl.telemetry.json"));
        fs::write(
            &telemetry,
            r#"{"version":2,"usage":{"promptTokens":900,"completionTokens":100}}"#,
        )
        .unwrap();
        fs::write(
            sessions.join(format!("{name}.jsonl.meta")),
            [0xff, 0xfe, 0xfd],
        )
        .unwrap();

        let bucket = ReasonixParser::new(root.path())
            .parse("host", true)
            .unwrap()
            .buckets
            .remove(0);
        assert_eq!(bucket.model, "deepseek-v4-flash");
        assert_eq!(bucket.provider, "reasonix");
        assert_ne!(bucket.bucket_start, "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn merges_deeply_nested_recovery_chains_into_one_bucket() {
        // 真实文件名有多层 12 位 hex 分支段 + recovery（如 19c3cf462078-c823fa29cc46-recovery-84e1f8...），
        // 循环剥离后必须与基础链合并为同一条记录。
        let root = tempdir().unwrap();
        let sessions = root.path().join("sample-project").join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let base = "20260806-045103.602016100-deepseek-deepseek-v4-flash";
        write_snapshot(&sessions, base, "2026-08-06T05:00:00Z", 254_794_897);
        write_snapshot(
            &sessions,
            &format!("{base}-recovery-250de9c4a81176d3"),
            "2026-08-06T05:10:00Z",
            254_794_897,
        );
        write_snapshot(
            &sessions,
            &format!("{base}-19c3cf462078-recovery-471decd3714a09a4"),
            "2026-08-06T05:11:00Z",
            254_794_897,
        );
        write_snapshot(
            &sessions,
            &format!("{base}-19c3cf462078-c823fa29cc46-recovery-84e1f8161344e5de"),
            "2026-08-06T05:12:00Z",
            254_794_897,
        );

        let output = ReasonixParser::new(root.path()).parse("host", true).unwrap();
        assert_eq!(output.files_scanned, 4);
        assert_eq!(output.usage_records, 1, "嵌套 recovery 链必须合并为同一条");
        assert_eq!(output.buckets.len(), 1);
        assert_eq!(output.buckets[0].total_tokens, 254_794_897);
        // bucket_start 来自文件名链创建时间,而非 updated_at（05:12 会落到 05:00 半小时桶）。
        assert_eq!(output.buckets[0].bucket_start, "2026-08-06T04:30:00.000Z");
    }

    #[test]
    fn bucket_start_is_stable_across_snapshot_updates() {
        // 同一链快照 updated_at 漂移（模拟多次扫描）时,bucket_start 必须稳定,
        // 否则同一累计值会因 identity 含 bucket_start 而被重复入库。
        let root = tempdir().unwrap();
        let sessions = root.path().join("sample-project").join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let base = "20260806-041734.612787600-session";
        // 第一次扫描:快照 updated_at = 04:54
        write_snapshot(&sessions, base, "2026-08-06T04:54:29Z", 6_843_336);
        let first = ReasonixParser::new(root.path())
            .parse("host", true)
            .unwrap()
            .buckets
            .remove(0);
        // 第二次扫描:快照 updated_at 漂移到 05:20（模拟同一链继续被写入）
        fs::remove_file(sessions.join(format!("{base}.jsonl.telemetry.json"))).unwrap();
        fs::remove_file(sessions.join(format!("{base}.jsonl.meta"))).unwrap();
        write_snapshot(&sessions, base, "2026-08-06T05:20:00Z", 7_000_000);
        let second = ReasonixParser::new(root.path())
            .parse("host", true)
            .unwrap()
            .buckets
            .remove(0);
        // bucket_start 必须相同（链创建时间 04:17 → 04:00 半小时桶）,total 取最新累计值。
        assert_eq!(first.bucket_start, second.bucket_start);
        assert_eq!(second.bucket_start, "2026-08-06T04:00:00.000Z");
        assert_eq!(second.total_tokens, 7_000_000);
    }

    #[test]
    fn cached_parser_attributes_new_cumulative_usage_to_observed_time() {
        let root = tempdir().unwrap();
        let sessions = root.path().join("sample-project").join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let name = "20260806-041733.050384000-session";
        let telemetry = sessions.join(format!("{name}.jsonl.telemetry.json"));
        let metadata = sessions.join(format!("{name}.jsonl.meta"));
        let cache = root.path().join("cache").join("reasonix.json");

        fs::write(&telemetry, r#"{"version":2,"usage":{"promptTokens":1000,"completionTokens":0,"lastPromptTokens":100,"lastCompletionTokens":0}}"#).unwrap();
        fs::write(&metadata, r#"{"updated_at":"2026-08-07T17:10:00Z","model":"deepseek/deepseek-v4-flash"}"#).unwrap();
        let first = ReasonixParser::with_cache(root.path(), &cache).parse("host", true).unwrap();
        assert_eq!(first.buckets.iter().map(|bucket| bucket.total_tokens).sum::<i64>(), 1_000);
        assert_eq!(first.buckets.iter().find(|bucket| bucket.bucket_start == "2026-08-07T17:00:00.000Z").unwrap().total_tokens, 100);

        fs::write(&telemetry, r#"{"version":2,"usage":{"promptTokens":1150,"completionTokens":0,"lastPromptTokens":150,"lastCompletionTokens":0}}"#).unwrap();
        fs::write(&metadata, r#"{"updated_at":"2026-08-07T17:40:00Z","model":"deepseek/deepseek-v4-flash"}"#).unwrap();
        let second = ReasonixParser::with_cache(root.path(), &cache).parse("host", true).unwrap();
        assert_eq!(second.buckets.iter().map(|bucket| bucket.total_tokens).sum::<i64>(), 1_150);
        assert_eq!(second.buckets.iter().find(|bucket| bucket.bucket_start == "2026-08-07T17:30:00.000Z").unwrap().total_tokens, 150);
    }
}
