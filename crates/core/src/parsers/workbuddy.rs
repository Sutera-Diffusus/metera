use super::{extract_sessions, provider_key, ParserOutput, SessionEvent};
use crate::usage::UsageBucket;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const HALF_HOUR_MS: i64 = 30 * 60 * 1000;

pub struct WorkBuddyParser {
    projects_dir: PathBuf,
}

impl WorkBuddyParser {
    pub fn new(projects_dir: impl Into<PathBuf>) -> Self {
        Self {
            projects_dir: projects_dir.into(),
        }
    }

    pub fn parse(&self, hostname: &str, include_project: bool) -> io::Result<ParserOutput> {
        let files = find_jsonl_files(&self.projects_dir)?;
        let providers = load_model_providers(self.projects_dir.parent().unwrap_or(&self.projects_dir));
        let mut output = ParserOutput {
            files_scanned: files.len(),
            ..ParserOutput::default()
        };
        let mut buckets = BTreeMap::<BucketKey, UsageBucket>::new();
        let mut session_events = Vec::new();

        for path in files {
            let snapshot_size = fs::metadata(&path)?.len();
            let mut raw = String::new();
            File::open(&path)?
                .take(snapshot_size)
                .read_to_string(&mut raw)?;

            for line in raw.lines().filter(|line| !line.trim().is_empty()) {
                let record = match serde_json::from_str::<Value>(line) {
                    Ok(record) => record,
                    Err(_) => {
                        output.malformed_lines += 1;
                        continue;
                    }
                };
                let Some(timestamp_ms) = extract_timestamp_ms(&record) else {
                    continue;
                };
                if record.get("type").and_then(Value::as_str) == Some("message") {
                    if let (Some(session_id), Some(role)) = (
                        record.get("sessionId").and_then(Value::as_str),
                        record.get("role").and_then(Value::as_str),
                    ) {
                        let project = if include_project {
                            first_string(&record, &["/cwd"])
                                .map(project_name)
                                .filter(|name| !name.is_empty())
                                .unwrap_or_else(|| "unknown".into())
                        } else {
                            "unknown".into()
                        };
                        session_events.push(SessionEvent {
                            session_id: session_id.to_string(),
                            source: "workbuddy".into(),
                            project,
                            hostname: hostname.into(),
                            timestamp_ms,
                            is_user: role == "user",
                        });
                    }
                }
                let Some(tokens) = extract_tokens(&record) else {
                    continue;
                };

                let bucket_start_ms = timestamp_ms.div_euclid(HALF_HOUR_MS) * HALF_HOUR_MS;
                let Some(bucket_start) = DateTime::<Utc>::from_timestamp_millis(bucket_start_ms)
                    .map(|date| date.to_rfc3339_opts(SecondsFormat::Millis, true))
                else {
                    continue;
                };
                let model = first_string(
                    &record,
                    &[
                        "/providerData/requestModelName",
                        "/providerData/model",
                        "/providerData/requestModelId",
                        "/message/model",
                    ],
                )
                .unwrap_or("unknown")
                .to_string();
                let provider_model = first_string(&record, &[
                    "/providerData/requestModelId",
                    "/providerData/requestModelName",
                    "/providerData/model",
                    "/message/model",
                ]).unwrap_or("unknown").to_ascii_lowercase();
                let provider = providers.get(&provider_model)
                    .or_else(|| provider_model.rsplit(':').next().and_then(|model| providers.get(model)))
                    .cloned()
                    .unwrap_or_else(|| format!("workbuddy-provider:{provider_model}"));
                let project = if include_project {
                    first_string(&record, &["/cwd"])
                        .map(project_name)
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| "unknown".into())
                } else {
                    "unknown".into()
                };
                let key = BucketKey {
                    bucket_start: bucket_start.clone(),
                    provider: provider.clone(),
                    model: model.clone(),
                    project: project.clone(),
                    hostname: hostname.to_string(),
                };
                let bucket = buckets.entry(key).or_insert_with(|| UsageBucket {
                    source: "workbuddy".into(),
                    provider,
                    model,
                    project,
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
                    + bucket.cached_input_tokens
                    + bucket.output_tokens
                    + bucket.reasoning_output_tokens;
                output.usage_records += 1;
            }
        }

        output.buckets = buckets.into_values().collect();
        output.sessions = extract_sessions(session_events);
        Ok(output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BucketKey {
    bucket_start: String,
    provider: String,
    model: String,
    project: String,
    hostname: String,
}

fn load_model_providers(root: &Path) -> BTreeMap<String, String> {
    let Some(models) = fs::read_to_string(root.join("models.json")).ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value.as_array().cloned()) else { return BTreeMap::new() };
    let mut providers = BTreeMap::new();
    for model in models {
        let fallback_name = model.get("vendor").and_then(Value::as_str).unwrap_or("unknown");
        let provider = provider_key(model.get("url").and_then(Value::as_str), &format!("workbuddy-provider:{}", fallback_name.to_ascii_lowercase()));
        for field in ["id", "name"] {
            if let Some(value) = model.get(field).and_then(Value::as_str) {
                providers.insert(value.to_ascii_lowercase(), provider.clone());
            }
        }
    }
    providers
}

#[derive(Debug, Clone, Copy)]
struct Tokens {
    input: i64,
    output: i64,
    cached: i64,
    reasoning: i64,
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

fn extract_timestamp_ms(record: &Value) -> Option<i64> {
    let value = record.get("timestamp")?;
    if let Some(number) = value.as_i64() {
        return Some(if number.abs() < 1_000_000_000_000 {
            number * 1000
        } else {
            number
        });
    }
    let parsed = DateTime::parse_from_rfc3339(value.as_str()?).ok()?;
    Some(parsed.timestamp_millis())
}

fn extract_tokens(record: &Value) -> Option<Tokens> {
    let usage = record
        .pointer("/providerData/rawUsage")
        .filter(|value| value.is_object())
        .map(|value| (value, UsageShape::Raw))
        .or_else(|| {
            record
                .pointer("/providerData/usage")
                .filter(|value| value.is_object())
                .map(|value| (value, UsageShape::Camel))
        })
        .or_else(|| {
            record
                .pointer("/message/usage")
                .filter(|value| value.is_object())
                .map(|value| (value, UsageShape::Snake))
        })?;

    let tokens = match usage.1 {
        UsageShape::Raw => {
            let input_total = token(usage.0, &["prompt_tokens", "input_tokens"]);
            let cached = token(
                usage.0,
                &[
                    "prompt_tokens_details.cached_tokens",
                    "prompt_cache_hit_tokens",
                    "cache_read_input_tokens",
                ],
            );
            let input = optional_token(usage.0, &["prompt_cache_miss_tokens"])
                .unwrap_or_else(|| input_total.saturating_sub(cached));
            let output_total = token(usage.0, &["completion_tokens", "output_tokens"]);
            let reasoning = token(
                usage.0,
                &[
                    "completion_tokens_details.reasoning_tokens",
                    "reasoning_output_tokens",
                ],
            );
            Tokens {
                input,
                output: output_total.saturating_sub(reasoning),
                cached,
                reasoning,
            }
        }
        UsageShape::Camel => {
            let input_total = token(usage.0, &["inputTokens"]);
            let cached = token(
                usage.0,
                &["inputTokensDetails.cachedTokens", "cachedInputTokens"],
            );
            let output_total = token(usage.0, &["outputTokens"]);
            let reasoning = token(
                usage.0,
                &[
                    "outputTokensDetails.reasoningTokens",
                    "reasoningOutputTokens",
                ],
            );
            Tokens {
                input: input_total.saturating_sub(cached),
                output: output_total.saturating_sub(reasoning),
                cached,
                reasoning,
            }
        }
        UsageShape::Snake => {
            let input_total = token(usage.0, &["input_tokens"]);
            let cached = token(usage.0, &["cache_read_input_tokens", "cached_input_tokens"]);
            let output_total = token(usage.0, &["output_tokens"]);
            let reasoning = token(usage.0, &["reasoning_output_tokens"]);
            Tokens {
                input: input_total.saturating_sub(cached),
                output: output_total.saturating_sub(reasoning),
                cached,
                reasoning,
            }
        }
    };

    (tokens.input > 0 || tokens.output > 0 || tokens.cached > 0 || tokens.reasoning > 0)
        .then_some(tokens)
}

enum UsageShape {
    Raw,
    Camel,
    Snake,
}

fn optional_token(value: &Value, paths: &[&str]) -> Option<i64> {
    paths.iter().find_map(|path| {
        let mut current = value;
        for segment in path.split('.') {
            current = current.get(segment)?;
        }
        current
            .as_i64()
            .or_else(|| {
                current
                    .as_u64()
                    .and_then(|number| i64::try_from(number).ok())
            })
            .map(|number| number.max(0))
    })
}

fn token(value: &Value, paths: &[&str]) -> i64 {
    optional_token(value, paths).unwrap_or(0)
}

fn first_string<'a>(value: &'a Value, pointers: &[&str]) -> Option<&'a str> {
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
}

fn project_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_cached_usage_and_session_messages() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("projects").join("demo");
        fs::create_dir_all(&project).unwrap();
        fs::write(directory.path().join("models.json"), r#"[{"id":"deepseek-v4-flash","name":"DeepSeek-V4 Flash","vendor":"DeepSeek","url":"https://api.deepseek.com/anthropic"}]"#).unwrap();
        fs::write(project.join("session.jsonl"), concat!(
            r#"{"timestamp":1785500000000,"type":"message","role":"user","sessionId":"one","cwd":"D:\\Work\\demo"}"#, "\n",
            r#"{"timestamp":1785500001000,"type":"message","role":"assistant","sessionId":"one","cwd":"D:\\Work\\demo"}"#, "\n",
            r#"{"timestamp":1785500002000,"type":"function_call","sessionId":"one","cwd":"D:\\Work\\demo","providerData":{"requestModelName":"DeepSeek-V4 Flash","rawUsage":{"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":80},"completion_tokens_details":{"reasoning_tokens":5}}}}"#, "\n",
        )).unwrap();

        let output = WorkBuddyParser::new(directory.path().join("projects"))
            .parse("host", true)
            .unwrap();
        assert_eq!(output.buckets.len(), 1);
        assert_eq!(output.buckets[0].input_tokens, 20);
        assert_eq!(output.buckets[0].cached_input_tokens, 80);
        assert_eq!(output.buckets[0].output_tokens, 15);
        assert_eq!(output.buckets[0].reasoning_output_tokens, 5);
        assert_eq!(output.buckets[0].total_tokens, 120);
        assert_eq!(output.buckets[0].provider, "api.deepseek.com");
        assert_eq!(output.sessions.len(), 1);
        assert_eq!(output.sessions[0].user_message_count, 1);
        assert_eq!(output.sessions[0].message_count, 2);
    }
}
