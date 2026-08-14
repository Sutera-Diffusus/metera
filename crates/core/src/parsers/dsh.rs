use super::{extract_sessions, ParserOutput, SessionEvent};
use crate::usage::UsageBucket;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const HALF_HOUR_MS: i64 = 30 * 60 * 1000;
const ZSTD_SUFFIX: &str = "session.jsonl.zstd";
const PLAIN_SUFFIX: &str = "session.jsonl";

/// DSH 会话日志解析器。事件型数据源：usage 自带时间戳、日志追加式，
/// 复用 replace_source 全量替换语义实现幂等，无需增量缓存（§18.2）。
pub struct DshParser {
    sessions_root: PathBuf,
}

impl DshParser {
    pub fn new(sessions_root: impl Into<PathBuf>) -> Self {
        Self { sessions_root: sessions_root.into() }
    }

    pub fn parse(&self, hostname: &str, include_project: bool) -> io::Result<ParserOutput> {
        let files = find_session_files(&self.sessions_root)?;
        let mut output = ParserOutput { files_scanned: files.len(), ..ParserOutput::default() };
        // 跨文件统一桶表：多个会话文件会落到同一 (provider,model,project,hostname,30min 桶)，
        // 必须在解析器内先合并求和——否则 replace_source 按 identity upsert 时后写覆盖先写，
        // 只保留最后一份、其余全部丢失（2026-08-14 花费对不上事故根因）。
        let mut buckets = BTreeMap::<String, UsageBucket>::new();
        for path in files {
            if parse_session_file(&path, hostname, include_project, &mut output, &mut buckets).is_err() {
                output.malformed_lines += 1;
            }
        }
        output.buckets = buckets.into_values().collect();
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

/// 单步 token 采样（同 (turn, step) last-wins）。
struct StepSample {
    time_ms: i64,
    provider: String,
    model: String,
    input: i64,
    output: i64,
    cached: i64,
    reasoning: i64,
}

impl StepSample {
    fn total(&self) -> i64 {
        self.input + self.output + self.cached + self.reasoning
    }
}

fn parse_session_file(path: &Path, hostname: &str, include_project: bool, output: &mut ParserOutput, buckets: &mut BTreeMap<String, UsageBucket>) -> io::Result<()> {
    let (text, torn) = decode_log(path)?;
    if torn { output.malformed_lines += 1; }
    let fallback_session_id = path
        .parent()
        .and_then(Path::file_name)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".into());
    let fallback_project = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".into());
    let mut header: Option<(String, String)> = None;
    let mut current_provider = String::new();
    let mut current_model = String::new();
    let mut usage_by_step: BTreeMap<(i64, i64), StepSample> = BTreeMap::new();
    let mut events: Vec<SessionEvent> = Vec::new();

    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            output.malformed_lines += 1;
            continue;
        };
        let Some(kind) = value.get("type").and_then(Value::as_str) else { continue };
        let time = value.get("time").and_then(Value::as_i64).unwrap_or(0);
        match kind {
            "session" => {
                let session_id = value.get("id").and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| fallback_session_id.clone());
                let project = if include_project {
                    value.get("cwd").and_then(Value::as_str)
                        .map(project_name)
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| fallback_project.clone())
                } else {
                    "unknown".into()
                };
                header = Some((session_id, project));
            }
            "request/header" => {
                let config = value.pointer("/data/header/config");
                if let Some(model) = config.and_then(|c| c.get("model")).and_then(Value::as_str).filter(|v| !v.is_empty()) {
                    current_model = model.to_string();
                }
                if let Some(provider) = config.and_then(|c| c.get("provider")).and_then(Value::as_str).filter(|v| !v.is_empty()) {
                    current_provider = normalize_provider(provider);
                }
            }
            "user/message" => push_event(&header, &fallback_session_id, &fallback_project, &mut events, hostname, time, true),
            "assistant/chunk" => {
                if value.pointer("/data/chunk/type").and_then(Value::as_str) == Some("usage") {
                    if let Some(usage) = value.pointer("/data/chunk/usage") {
                        let turn = value.pointer("/data/turn").and_then(Value::as_i64).unwrap_or(0);
                        let step = value.pointer("/data/step").and_then(Value::as_i64).unwrap_or(0);
                        record_usage(&mut usage_by_step, turn, step, time, &current_provider, &current_model, usage);
                    }
                    push_event(&header, &fallback_session_id, &fallback_project, &mut events, hostname, time, false);
                }
            }
            "assistant/message" => {
                if let Some(usage) = value.pointer("/data/usage") {
                    let turn = value.pointer("/data/turn").and_then(Value::as_i64).unwrap_or(0);
                    let step = value.pointer("/data/step").and_then(Value::as_i64).unwrap_or(0);
                    record_usage(&mut usage_by_step, turn, step, time, &current_provider, &current_model, usage);
                }
                push_event(&header, &fallback_session_id, &fallback_project, &mut events, hostname, time, false);
            }
            "step/end" | "tool/call" | "tool/result" => {
                push_event(&header, &fallback_session_id, &fallback_project, &mut events, hostname, time, false);
            }
            // 打包 chunk 行：折叠为「末时刻单事件」（time0 + sum(dt)），保持活跃间隙诚实、避免数千事件。
            "reasoning-chunks" | "text-chunks" | "tool-call-chunks" => {
                let time0 = value.get("time0").and_then(Value::as_i64).unwrap_or(time);
                let dt_sum: i64 = value.get("data")
                    .and_then(|d| d.get("dt"))
                    .and_then(Value::as_array)
                    .map(|array| array.iter().filter_map(Value::as_i64).sum())
                    .unwrap_or(0);
                push_event(&header, &fallback_session_id, &fallback_project, &mut events, hostname, time0.saturating_add(dt_sum), false);
            }
            _ => {}
        }
    }

    let (_, project) = header.unwrap_or_else(|| (fallback_session_id, fallback_project));
    for (_, sample) in usage_by_step {
        add_bucket(buckets, sample, &project, hostname);
    }
    output.sessions.extend(extract_sessions(events));
    Ok(())
}

/// 记录一次 usage 采样。同 (turn, step) 的后续采样替换早先采样（DSH token-meter 同款 last-wins 语义）。
fn record_usage(
    map: &mut BTreeMap<(i64, i64), StepSample>,
    turn: i64,
    step: i64,
    time_ms: i64,
    provider: &str,
    model: &str,
    usage: &Value,
) {
    let output_total = usage.get("outputTokens").and_then(Value::as_i64).unwrap_or(0).max(0);
    let reasoning = usage.get("reasoningTokens").and_then(Value::as_i64).unwrap_or(0).max(0).min(output_total);
    let sample = StepSample {
        time_ms,
        provider: if provider.is_empty() { "dsh".into() } else { provider.into() },
        model: if model.is_empty() { "unknown".into() } else { model.into() },
        input: usage.get("inputTokens").and_then(Value::as_i64).unwrap_or(0).max(0),
        output: output_total.saturating_sub(reasoning),
        cached: usage.get("cacheReadTokens").and_then(Value::as_i64).unwrap_or(0).max(0),
        reasoning,
    };
    if sample.total() > 0 {
        map.insert((turn, step), sample);
    }
}

fn push_event(
    header: &Option<(String, String)>,
    fallback_id: &str,
    fallback_project: &str,
    events: &mut Vec<SessionEvent>,
    hostname: &str,
    time_ms: i64,
    is_user: bool,
) {
    let (session_id, project) = match header {
        Some((id, project)) => (id.clone(), project.clone()),
        None => (fallback_id.to_string(), fallback_project.to_string()),
    };
    events.push(SessionEvent {
        session_id,
        source: "dsh".into(),
        project,
        hostname: hostname.into(),
        timestamp_ms: time_ms,
        is_user,
    });
}

/// provider 规范化：deepseek 系 → "deepseek"；空 → "dsh"；其余小写原样。
fn normalize_provider(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() { return "dsh".into(); }
    if trimmed.to_ascii_lowercase().contains("deepseek") { return "deepseek".into(); }
    trimmed.to_ascii_lowercase()
}

fn project_name(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\'])
        .split(['/', '\\'])
        .last()
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .into()
}

fn add_bucket(buckets: &mut BTreeMap<String, UsageBucket>, sample: StepSample, project: &str, hostname: &str) {
    let start = sample.time_ms.div_euclid(HALF_HOUR_MS) * HALF_HOUR_MS;
    let Some(bucket_start) = DateTime::<Utc>::from_timestamp_millis(start)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true)) else { return };
    let key = [&sample.provider, &sample.model, project, hostname, &bucket_start].join("\0");
    let bucket = buckets.entry(key).or_insert_with(|| UsageBucket {
        source: "dsh".into(),
        provider: sample.provider.clone(),
        model: sample.model.clone(),
        project: project.into(),
        hostname: hostname.into(),
        bucket_start,
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        reasoning_output_tokens: 0,
        total_tokens: 0,
    });
    bucket.input_tokens += sample.input;
    bucket.output_tokens += sample.output;
    bucket.cached_input_tokens += sample.cached;
    bucket.reasoning_output_tokens += sample.reasoning;
    bucket.total_tokens = bucket.input_tokens + bucket.output_tokens + bucket.cached_input_tokens + bucket.reasoning_output_tokens;
}

/// 解码日志：.zstd 流式逐帧解码（坏帧/崩溃残留保留已解码前缀并标记 torn）；未压缩 .jsonl 直读。
fn decode_log(path: &Path) -> io::Result<(String, bool)> {
    let is_zstd = path.file_name().and_then(|v| v.to_str()).is_some_and(|n| n.ends_with(".zstd"));
    if !is_zstd {
        return Ok((fs::read_to_string(path)?, false));
    }
    let file = fs::File::open(path)?;
    let mut reader = zstd::stream::read::Decoder::new(file).map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut torn = false;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&buffer[..n]),
            Err(_) => { torn = true; break; }
        }
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), torn))
}

/// 递归发现会话日志；`.jsonl` 与 `.zstd` 并存时仅取 .zstd（避免切换压缩配置后重复计数）。
fn find_session_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    if !root.exists() { return Ok(Vec::new()); }
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                directories.push(path);
            } else {
                let name = path.file_name().and_then(|value| value.to_str()).unwrap_or_default();
                if name == ZSTD_SUFFIX {
                    files.push(path);
                } else if name == PLAIN_SUFFIX && !path.with_file_name(ZSTD_SUFFIX).exists() {
                    files.push(path);
                }
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

    const HEADER: &str = r#"{"type":"session","version":0,"id":"session-abc123","createdAt":1786633055544,"cwd":"D:\\DeepseekHarness_WorkSpace","delegationDepth":0,"agentPreset":"standard"}"#;

    fn zstd_fixture(lines: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for line in lines {
            bytes.extend_from_slice(&zstd::bulk::compress(format!("{line}\n").as_bytes(), 3).unwrap());
        }
        bytes
    }

    fn write_zstd_session(root: &Path, project_dir: &str, session_dir: &str, lines: &[&str]) -> PathBuf {
        let dir = root.join(project_dir).join(session_dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(ZSTD_SUFFIX);
        fs::write(&path, zstd_fixture(lines)).unwrap();
        path
    }

    fn usage_chunk(seq: i64, time: i64, turn: i64, step: i64, usage: &str) -> String {
        format!(r#"{{"type":"assistant/chunk","seq":{seq},"time":{time},"data":{{"turn":{turn},"step":{step},"chunk":{{"type":"usage","usage":{usage}}}}}}}"#)
    }

    #[test]
    fn maps_usage_fields_without_double_counting() {
        let root = tempdir().unwrap();
        write_zstd_session(root.path(), "--D-DeepseekHarness_WorkSpace--", "session-abc123", &[
            HEADER,
            r#"{"type":"request/header","seq":1,"time":1786633059000,"data":{"header":{"config":{"provider":"deepseek-official","model":"deepseek-v4-pro"}}}}"#,
            r#"{"type":"user/message","seq":2,"time":1786633059500,"data":{"role":"user","content":[]},"surfaceOp":"append"}"#,
            &usage_chunk(3, 1786633080000, 1, 1, r#"{"inputTokens":20607,"outputTokens":1405,"cacheReadTokens":7808,"reasoningTokens":1281}"#),
            r#"{"type":"step/end","seq":4,"time":1786633081670,"data":{"turn":1,"step":1}}"#,
        ]);
        let output = DshParser::new(root.path()).parse("host", true).unwrap();
        assert_eq!(output.files_scanned, 1);
        assert_eq!(output.buckets.len(), 1);
        let bucket = &output.buckets[0];
        assert_eq!(bucket.source, "dsh");
        assert_eq!(bucket.provider, "deepseek");
        assert_eq!(bucket.model, "deepseek-v4-pro");
        assert_eq!(bucket.project, "DeepseekHarness_WorkSpace");
        assert_eq!(bucket.input_tokens, 20607);
        assert_eq!(bucket.cached_input_tokens, 7808);
        assert_eq!(bucket.output_tokens, 124); // 1405 - 1281（output 已含 reasoning）
        assert_eq!(bucket.reasoning_output_tokens, 1281);
        assert_eq!(bucket.total_tokens, 20607 + 7808 + 124 + 1281);
        assert_eq!(bucket.bucket_start, "2026-08-13T14:30:00.000Z");
        assert_eq!(output.sessions.len(), 1);
        assert_eq!(output.sessions[0].user_message_count, 1);
    }

    #[test]
    fn same_step_samples_are_last_wins() {
        let root = tempdir().unwrap();
        write_zstd_session(root.path(), "--proj--", "session-abc123", &[
            HEADER,
            r#"{"type":"assistant/chunk","seq":1,"time":1786633080000,"data":{"turn":1,"step":1,"chunk":{"type":"usage","usage":{"inputTokens":100,"outputTokens":10,"cacheReadTokens":5,"reasoningTokens":2}}}}"#,
            r#"{"type":"assistant/message","seq":2,"time":1786633081000,"data":{"turn":1,"step":1,"usage":{"inputTokens":900,"outputTokens":90,"cacheReadTokens":50,"reasoningTokens":20}}}"#,
        ]);
        let output = DshParser::new(root.path()).parse("host", true).unwrap();
        assert_eq!(output.buckets.len(), 1);
        let bucket = &output.buckets[0];
        assert_eq!(bucket.input_tokens, 900);
        assert_eq!(bucket.cached_input_tokens, 50);
        assert_eq!(bucket.output_tokens, 70); // 90 - 20
        assert_eq!(bucket.reasoning_output_tokens, 20);
        assert_eq!(bucket.total_tokens, 900 + 50 + 70 + 20);
    }

    #[test]
    fn model_and_provider_follow_latest_request_header() {
        let root = tempdir().unwrap();
        write_zstd_session(root.path(), "--proj--", "session-abc123", &[
            HEADER,
            r#"{"type":"request/header","seq":1,"time":1786633059000,"data":{"header":{"config":{"provider":"deepseek-official","model":"deepseek-v4-pro"}}}}"#,
            &usage_chunk(2, 1786633080000, 1, 1, r#"{"inputTokens":10,"outputTokens":5}"#),
            r#"{"type":"request/header","seq":3,"time":1786633090000,"data":{"header":{"config":{"provider":"deepseek-official","model":"deepseek-v4-flash"}}}}"#,
            &usage_chunk(4, 1786633100000, 2, 1, r#"{"inputTokens":20,"outputTokens":5}"#),
        ]);
        let output = DshParser::new(root.path()).parse("host", true).unwrap();
        let mut models: Vec<_> = output.buckets.iter().map(|bucket| bucket.model.as_str()).collect();
        models.sort();
        assert_eq!(models, vec!["deepseek-v4-flash", "deepseek-v4-pro"]);
        assert!(output.buckets.iter().all(|bucket| bucket.provider == "deepseek"));
    }

    #[test]
    fn torn_tail_frame_keeps_valid_prefix_and_counts_malformed() {
        let root = tempdir().unwrap();
        let dir = root.path().join("--proj--").join("session-abc123");
        fs::create_dir_all(&dir).unwrap();
        let mut bytes = zstd_fixture(&[HEADER, &usage_chunk(1, 1786633080000, 1, 1, r#"{"inputTokens":100,"outputTokens":10}"#)]);
        bytes.extend_from_slice(b"\x28\xb5\x2f\xfd garbage torn frame");
        fs::write(dir.join(ZSTD_SUFFIX), bytes).unwrap();
        let output = DshParser::new(root.path()).parse("host", true).unwrap();
        assert_eq!(output.buckets.len(), 1);
        assert_eq!(output.buckets[0].input_tokens, 100);
        assert_eq!(output.malformed_lines, 1);
    }

    #[test]
    fn plain_jsonl_log_is_supported() {
        let root = tempdir().unwrap();
        let dir = root.path().join("--proj--").join("session-abc123");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(PLAIN_SUFFIX),
            format!("{HEADER}\n{}\n", usage_chunk(1, 1786633080000, 1, 1, r#"{"inputTokens":50,"outputTokens":5}"#)),
        ).unwrap();
        let output = DshParser::new(root.path()).parse("host", true).unwrap();
        assert_eq!(output.buckets.len(), 1);
        assert_eq!(output.buckets[0].input_tokens, 50);
    }

    #[test]
    fn empty_root_yields_default_output() {
        let root = tempdir().unwrap();
        let output = DshParser::new(root.path()).parse("host", true).unwrap();
        assert_eq!(output.files_scanned, 0);
        assert!(output.buckets.is_empty());
        assert!(output.sessions.is_empty());
    }

    #[test]
    fn packed_chunk_rows_collapse_into_one_event() {
        let root = tempdir().unwrap();
        write_zstd_session(root.path(), "--proj--", "session-abc123", &[
            HEADER,
            r#"{"type":"user/message","seq":1,"time":1786633059500,"data":{"role":"user","content":[]}}"#,
            r#"{"type":"reasoning-chunks","seq0":2,"time0":1786633060000,"data":{"turn":1,"step":1,"index":0,"dt":[1,2,3],"texts":["a","b","c"]}}"#,
            r#"{"type":"step/end","seq":3,"time":1786633080000,"data":{"turn":1,"step":1}}"#,
        ]);
        let output = DshParser::new(root.path()).parse("host", true).unwrap();
        assert!(output.buckets.is_empty());
        assert_eq!(output.sessions.len(), 1);
        let session = &output.sessions[0];
        assert_eq!(session.user_message_count, 1);
        assert_eq!(session.message_count, 3); // user + 折叠打包行 + step/end
        assert!(session.active_seconds >= 0);
    }

    #[test]
    fn include_project_false_hides_project() {
        let root = tempdir().unwrap();
        write_zstd_session(root.path(), "--proj--", "session-abc123", &[
            HEADER,
            &usage_chunk(1, 1786633080000, 1, 1, r#"{"inputTokens":10,"outputTokens":2}"#),
        ]);
        let output = DshParser::new(root.path()).parse("host", false).unwrap();
        assert_eq!(output.buckets[0].project, "unknown");
        assert_eq!(output.sessions[0].project, "unknown");
    }

    #[test]
    fn merges_same_bucket_across_session_files() {
        // 回归（2026-08-14 花费对不上事故）：多个会话文件落在同一
        // (provider,model,project,hostname,30min 桶) 时，解析器必须合并求和。
        // 否则 replace_source 按 identity upsert 后写覆盖先写，只保留最后一份。
        let root = tempdir().unwrap();
        write_zstd_session(root.path(), "--proj--", "session-aaa", &[
            HEADER,
            &usage_chunk(1, 1786633080000, 1, 1, r#"{"inputTokens":100,"outputTokens":10,"cacheReadTokens":1000}"#),
        ]);
        write_zstd_session(root.path(), "--proj--", "session-bbb", &[
            r#"{"type":"session","version":0,"id":"session-bbb","createdAt":1786633055544,"cwd":"D:\\DeepseekHarness_WorkSpace","delegationDepth":0}"#,
            &usage_chunk(1, 1786633080000, 1, 1, r#"{"inputTokens":200,"outputTokens":20,"cacheReadTokens":2000}"#),
        ]);
        let output = DshParser::new(root.path()).parse("host", true).unwrap();
        assert_eq!(output.buckets.len(), 1, "跨文件同桶必须合并为一条");
        let bucket = &output.buckets[0];
        assert_eq!(bucket.input_tokens, 300);
        assert_eq!(bucket.output_tokens, 30);
        assert_eq!(bucket.cached_input_tokens, 3000);
        assert_eq!(bucket.total_tokens, 300 + 30 + 3000);
        assert_eq!(output.sessions.len(), 2);
    }

    /// 诊断用（默认跳过）：对 `DSH_SESSIONS_DIR` 指定的真实会话目录跑一次解析并打印汇总，
    /// 供与 DSH 官方投影（Node 参考脚本）在冻结副本上交叉验证。
    /// 运行：`cargo test -p metera-core dsh_diagnostic -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dsh_diagnostic_parse_real_dir() {
        let Some(root) = std::env::var_os("DSH_SESSIONS_DIR") else { return };
        let output = DshParser::new(PathBuf::from(root)).parse("host", true).unwrap();
        let mut totals = (0i64, 0i64, 0i64, 0i64);
        for bucket in &output.buckets {
            totals.0 += bucket.input_tokens;
            totals.1 += bucket.output_tokens;
            totals.2 += bucket.cached_input_tokens;
            totals.3 += bucket.reasoning_output_tokens;
        }
        println!(
            "dsh diagnostic: files={} buckets={} sessions={} malformed={} usage_records={}",
            output.files_scanned, output.buckets.len(), output.sessions.len(), output.malformed_lines, output.usage_records
        );
        println!(
            "dsh totals: input={} output={} cached={} reasoning={} sum={}",
            totals.0, totals.1, totals.2, totals.3, totals.0 + totals.1 + totals.2 + totals.3
        );
    }
}
