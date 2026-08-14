use super::{extract_sessions, provider_key, ParserOutput, SessionEvent};
use crate::usage::UsageBucket;
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{types::Value as SqlValue, Connection, OpenFlags};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const HALF_HOUR_MS: i64 = 30 * 60 * 1000;
pub struct ZcodeParser { path: PathBuf, config: PathBuf }
impl ZcodeParser {
    pub fn new(home: impl AsRef<Path>) -> Self { Self { path: home.as_ref().join(".zcode/cli/db/db.sqlite"), config: home.as_ref().join(".zcode/v2/config.json") } }
    pub fn parse(&self, hostname: &str, include_project: bool) -> io::Result<ParserOutput> {
        if !self.path.exists() { return Ok(ParserOutput::default()); }
        parse_database(&self.path, hostname, include_project, &load_provider_config(&self.config)).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }
}

fn parse_database(path: &Path, hostname: &str, include_project: bool, providers: &BTreeMap<String, String>) -> rusqlite::Result<ParserOutput> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare("SELECT m.session_id, m.time_created,
        json_extract(m.data, '$.role'), json_extract(m.data, '$.modelID'), json_extract(m.data, '$.providerID'), json_extract(m.data, '$.tokens'),
        json_extract(m.data, '$.path.root'), json_extract(m.data, '$.path.cwd'), s.directory
        FROM message m LEFT JOIN session s ON s.id = m.session_id")?;
    let mut rows = statement.query([])?;
    let mut buckets = BTreeMap::<String, UsageBucket>::new(); let mut events = Vec::new(); let mut records = 0;
    while let Some(row) = rows.next()? {
        let session_id: Option<String> = row.get(0)?; let created: SqlValue = row.get(1)?;
        let Some(timestamp) = timestamp_ms(&created) else { continue };
        let role: Option<String> = row.get(2)?; let model: Option<String> = row.get(3)?; let provider_id: Option<String> = row.get(4)?; let tokens_raw: Option<String> = row.get(5)?;
        let root: Option<String> = row.get(6)?; let cwd: Option<String> = row.get(7)?; let dir: Option<String> = row.get(8)?;
        let project = if include_project { project_name(root.as_deref().or(cwd.as_deref()).or(dir.as_deref())) } else { "unknown".into() };
        events.push(SessionEvent { session_id: session_id.unwrap_or_else(|| "unknown".into()), source: "zcode".into(), project: project.clone(), hostname: hostname.into(), timestamp_ms: timestamp, is_user: role.as_deref() == Some("user") });
        if role.as_deref() != Some("assistant") { continue; }
        let Some(tokens) = tokens_raw.and_then(|v| serde_json::from_str::<Value>(&v).ok()) else { continue };
        let cached = tokens.pointer("/cache/read").and_then(Value::as_i64).unwrap_or(0);
        let reasoning = tokens.get("reasoning").and_then(Value::as_i64).unwrap_or(0);
        let input = tokens.get("input").and_then(Value::as_i64).unwrap_or(0).saturating_sub(cached);
        let output = tokens.get("output").and_then(Value::as_i64).unwrap_or(0).saturating_sub(reasoning);
        if input == 0 && output == 0 && cached == 0 && reasoning == 0 { continue; }
        let provider = provider_id.as_deref().and_then(|id| providers.get(id)).cloned()
            .unwrap_or_else(|| format!("zcode-provider:{}", provider_id.as_deref().unwrap_or("unknown").to_ascii_lowercase()));
        add_bucket(&mut buckets, timestamp, model.as_deref().unwrap_or("unknown"), &provider, &project, hostname, input, output, cached, reasoning); records += 1;
    }
    Ok(ParserOutput { buckets: buckets.into_values().collect(), sessions: extract_sessions(events), files_scanned: 1, usage_records: records, malformed_lines: 0 })
}

fn add_bucket(map: &mut BTreeMap<String, UsageBucket>, timestamp: i64, model: &str, provider: &str, project: &str, hostname: &str, input: i64, output: i64, cached: i64, reasoning: i64) {
    let start = timestamp.div_euclid(HALF_HOUR_MS) * HALF_HOUR_MS;
    let Some(bucket_start) = DateTime::<Utc>::from_timestamp_millis(start).map(|v| v.to_rfc3339_opts(SecondsFormat::Millis, true)) else { return };
    let key = [provider, model, project, hostname, &bucket_start].join("\0");
    let bucket = map.entry(key).or_insert_with(|| UsageBucket { source: "zcode".into(), provider: provider.into(), model: model.into(), project: project.into(), hostname: hostname.into(), bucket_start, input_tokens: 0, output_tokens: 0, cached_input_tokens: 0, reasoning_output_tokens: 0, total_tokens: 0 });
    bucket.input_tokens += input; bucket.output_tokens += output; bucket.cached_input_tokens += cached; bucket.reasoning_output_tokens += reasoning;
    bucket.total_tokens = bucket.input_tokens + bucket.output_tokens + bucket.cached_input_tokens + bucket.reasoning_output_tokens;
}
fn load_provider_config(path: &Path) -> BTreeMap<String, String> {
    let Some(value) = fs::read_to_string(path).ok().and_then(|raw| serde_json::from_str::<Value>(&raw).ok()) else { return BTreeMap::new() };
    value.get("provider").and_then(Value::as_object).into_iter().flatten().map(|(id, config)| {
        let endpoint = config.pointer("/options/baseURL").and_then(Value::as_str);
        (id.clone(), provider_key(endpoint, &format!("zcode-provider:{}", id.to_ascii_lowercase())))
    }).collect()
}
fn timestamp_ms(value: &SqlValue) -> Option<i64> {
    match value {
        SqlValue::Integer(v) => Some(if v.abs() < 1_000_000_000_000 { v * 1000 } else { *v }),
        SqlValue::Real(v) => Some(if v.abs() < 1_000_000_000_000.0 { (v * 1000.0) as i64 } else { *v as i64 }),
        SqlValue::Text(v) => v.parse::<i64>().ok().map(|n| if n.abs() < 1_000_000_000_000 { n * 1000 } else { n }).or_else(|| DateTime::parse_from_rfc3339(v).ok().map(|d| d.timestamp_millis())),
        _ => None,
    }
}
fn project_name(value: Option<&str>) -> String { value.and_then(|v| v.trim_end_matches(['/', '\\']).split(['/', '\\']).last()).filter(|v| !v.is_empty()).unwrap_or("unknown").into() }
