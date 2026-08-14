use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBucket {
    pub source: String,
    pub provider: String,
    pub model: String,
    pub project: String,
    pub hostname: String,
    pub bucket_start: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_input_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSession {
    pub source: String,
    pub project: String,
    pub hostname: String,
    pub session_hash: String,
    pub first_message_at: String,
    pub last_message_at: String,
    pub duration_seconds: i64,
    pub active_seconds: i64,
    pub message_count: i64,
    pub user_message_count: i64,
    pub user_prompt_hours: Vec<i64>,
}

impl UsageBucket {
    pub fn hide_project(mut self) -> Self {
        self.project = "unknown".into();
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_input_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpsertStats {
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
}

pub struct UsageRepository {
    connection: Connection,
}

impl UsageRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS usage_buckets (
                identity TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL,
                source TEXT NOT NULL,
                provider TEXT NOT NULL DEFAULT 'unknown',
                model TEXT NOT NULL,
                project TEXT NOT NULL,
                hostname TEXT NOT NULL,
                bucket_start TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cached_input_tokens INTEGER NOT NULL,
                reasoning_output_tokens INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS usage_buckets_time_source
                ON usage_buckets(bucket_start, source);
            CREATE TABLE IF NOT EXISTS usage_sessions (
                identity TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL,
                source TEXT NOT NULL,
                project TEXT NOT NULL,
                hostname TEXT NOT NULL,
                session_hash TEXT NOT NULL,
                first_message_at TEXT NOT NULL,
                last_message_at TEXT NOT NULL,
                duration_seconds INTEGER NOT NULL,
                active_seconds INTEGER NOT NULL,
                message_count INTEGER NOT NULL,
                user_message_count INTEGER NOT NULL,
                user_prompt_hours TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS usage_sessions_time_source
                ON usage_sessions(first_message_at, source);
            CREATE INDEX IF NOT EXISTS usage_sessions_last_message
                ON usage_sessions(last_message_at);
            CREATE TABLE IF NOT EXISTS metera_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        if !has_column(&connection, "usage_buckets", "provider")? {
            connection.execute("ALTER TABLE usage_buckets ADD COLUMN provider TEXT NOT NULL DEFAULT 'unknown'", [])?;
        }
        let schema_version = connection.query_row("SELECT value FROM metera_metadata WHERE key = 'derived_schema'", [], |row| row.get::<_, String>(0)).optional()?;
        if schema_version.as_deref() != Some("3") {
            // 升级/降级到非当前 schema:先备份现有数据到备份表,再清空,避免未来 schema
            // 变更时无备份丢失全部历史。
            let backup_suffix = format!("_v{}_bak", schema_version.as_deref().unwrap_or("0"));
            for table in ["usage_buckets", "usage_sessions"] {
                let has_old = connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                        [&format!("{table}{backup_suffix}")],
                        |row| row.get::<_, i64>(0),
                    )?
                    > 0;
                if !has_old {
                    connection.execute_batch(&format!(
                        "CREATE TABLE {table}{backup_suffix} AS SELECT * FROM {table};"
                    ))?;
                }
            }
            connection.execute_batch("DELETE FROM usage_buckets; DELETE FROM usage_sessions;
                INSERT INTO metera_metadata(key,value) VALUES('derived_schema','3')
                ON CONFLICT(key) DO UPDATE SET value=excluded.value;")?;
        }
        Ok(Self { connection })
    }

    pub fn upsert_buckets(&mut self, buckets: &[UsageBucket]) -> Result<UpsertStats> {
        let transaction = self.connection.transaction()?;
        let mut stats = UpsertStats::default();

        for bucket in buckets {
            let identity = bucket_identity(bucket);
            let content_hash = bucket_hash(bucket);
            let existing = transaction
                .query_row(
                    "SELECT content_hash, input_tokens + output_tokens + cached_input_tokens + reasoning_output_tokens
                     FROM usage_buckets WHERE identity = ?1",
                    [&identity],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?;

            let incoming_total = bucket.input_tokens + bucket.output_tokens
                + bucket.cached_input_tokens + bucket.reasoning_output_tokens;
            match existing.as_ref() {
                Some((hash, _)) if hash == &content_hash => {
                    stats.unchanged += 1;
                    continue;
                }
                Some((_, stored_total)) if *stored_total > incoming_total => {
                    stats.unchanged += 1;
                    continue;
                }
                Some(_) => stats.updated += 1,
                None => stats.inserted += 1,
            }

            transaction.execute(
                "INSERT INTO usage_buckets (
                    identity, content_hash, source, provider, model, project, hostname,
                    bucket_start, input_tokens, output_tokens, cached_input_tokens,
                    reasoning_output_tokens, total_tokens
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                ON CONFLICT(identity) DO UPDATE SET
                    content_hash = excluded.content_hash,
                    input_tokens = excluded.input_tokens,
                    output_tokens = excluded.output_tokens,
                    cached_input_tokens = excluded.cached_input_tokens,
                    reasoning_output_tokens = excluded.reasoning_output_tokens,
                    total_tokens = excluded.total_tokens",
                params![
                    identity,
                    content_hash,
                    bucket.source,
                    bucket.provider,
                    bucket.model,
                    bucket.project,
                    bucket.hostname,
                    bucket.bucket_start,
                    bucket.input_tokens,
                    bucket.output_tokens,
                    bucket.cached_input_tokens,
                    bucket.reasoning_output_tokens,
                    bucket.total_tokens,
                ],
            )?;
        }

        transaction.commit()?;
        Ok(stats)
    }

    pub fn remove_sources(&mut self, sources: &[&str]) -> Result<usize> {
        let mut removed = 0;
        for source in sources {
            removed += self.connection.execute("DELETE FROM usage_buckets WHERE lower(source) = lower(?1)", [source])?;
            removed += self.connection.execute("DELETE FROM usage_sessions WHERE lower(source) = lower(?1)", [source])?;
        }
        Ok(removed)
    }

    /// 返回某个数据源当前已入库的 bucket + session 记录数。
    /// 用于空扫描保护：解析结果为空但库中已有历史时,不应清空历史。
    pub fn source_record_count(&self, source: &str) -> Result<usize> {
        let buckets: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM usage_buckets WHERE lower(source) = lower(?1)",
            [source],
            |row| row.get(0),
        )?;
        let sessions: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM usage_sessions WHERE lower(source) = lower(?1)",
            [source],
            |row| row.get(0),
        )?;
        Ok((buckets + sessions) as usize)
    }

    pub fn upsert_sessions(&mut self, sessions: &[UsageSession]) -> Result<UpsertStats> {
        let transaction = self.connection.transaction()?;
        let mut stats = UpsertStats::default();
        for session in sessions {
            let identity = [session.source.as_str(), session.hostname.as_str(), session.session_hash.as_str()].join("\0");
            let payload = serde_json::to_string(session).unwrap_or_default();
            let content_hash = &hex::encode(Sha256::digest(payload.as_bytes()))[..16];
            let existing = transaction.query_row(
                "SELECT content_hash, message_count FROM usage_sessions WHERE identity = ?1",
                [&identity],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            ).optional()?;
            match existing.as_ref() {
                Some((hash, _)) if hash == content_hash => { stats.unchanged += 1; continue; }
                Some((_, count)) if *count > session.message_count => { stats.unchanged += 1; continue; }
                Some(_) => stats.updated += 1,
                None => stats.inserted += 1,
            }
            transaction.execute(
                "INSERT INTO usage_sessions (identity, content_hash, source, project, hostname,
                    session_hash, first_message_at, last_message_at, duration_seconds, active_seconds,
                    message_count, user_message_count, user_prompt_hours)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                 ON CONFLICT(identity) DO UPDATE SET content_hash=excluded.content_hash,
                    project=excluded.project, first_message_at=excluded.first_message_at,
                    last_message_at=excluded.last_message_at, duration_seconds=excluded.duration_seconds,
                    active_seconds=excluded.active_seconds, message_count=excluded.message_count,
                    user_message_count=excluded.user_message_count, user_prompt_hours=excluded.user_prompt_hours",
                params![identity, content_hash, session.source, session.project, session.hostname,
                    session.session_hash, session.first_message_at, session.last_message_at,
                    session.duration_seconds, session.active_seconds, session.message_count,
                    session.user_message_count, serde_json::to_string(&session.user_prompt_hours).unwrap_or_else(|_| "[]".into())],
            )?;
        }
        transaction.commit()?;
        Ok(stats)
    }

    /// 原子地重建单个数据源：删除旧记录后写入全部 buckets 与 sessions。
    /// 删除与写入在同一外层事务内；嵌套的 upsert_* 会自动退化为 SAVEPOINT，
    /// 任一步失败时整体回滚，避免 remove_sources 与 upsert 之间崩溃造成数据丢失。
    pub fn replace_source(
        &mut self,
        source: &str,
        buckets: &[UsageBucket],
        sessions: &[UsageSession],
    ) -> Result<(usize, UpsertStats, UpsertStats)> {
        // 单个 rusqlite 事务对象承载全部写入:Drop 未提交自动回滚,
        // 任一步失败整体撤销,避免中途崩溃丢失整个数据源。
        let tx = self.connection.transaction()?;
        let mut removed = 0;
        removed += tx.execute("DELETE FROM usage_buckets WHERE lower(source) = lower(?1)", [source])?;
        removed += tx.execute("DELETE FROM usage_sessions WHERE lower(source) = lower(?1)", [source])?;

        let mut bucket_stats = UpsertStats::default();
        for bucket in buckets {
            let identity = bucket_identity(bucket);
            let content_hash = bucket_hash(bucket);
            let existing = tx
                .query_row(
                    "SELECT content_hash, input_tokens + output_tokens + cached_input_tokens + reasoning_output_tokens
                     FROM usage_buckets WHERE identity = ?1",
                    [&identity],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?;
            let incoming_total = bucket.input_tokens + bucket.output_tokens
                + bucket.cached_input_tokens + bucket.reasoning_output_tokens;
            match existing.as_ref() {
                Some((hash, _)) if hash == &content_hash => { bucket_stats.unchanged += 1; continue; }
                Some((_, stored_total)) if *stored_total > incoming_total => { bucket_stats.unchanged += 1; continue; }
                Some(_) => bucket_stats.updated += 1,
                None => bucket_stats.inserted += 1,
            }
            tx.execute(
                "INSERT INTO usage_buckets (
                    identity, content_hash, source, provider, model, project, hostname,
                    bucket_start, input_tokens, output_tokens, cached_input_tokens,
                    reasoning_output_tokens, total_tokens
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                ON CONFLICT(identity) DO UPDATE SET
                    content_hash = excluded.content_hash,
                    input_tokens = excluded.input_tokens,
                    output_tokens = excluded.output_tokens,
                    cached_input_tokens = excluded.cached_input_tokens,
                    reasoning_output_tokens = excluded.reasoning_output_tokens,
                    total_tokens = excluded.total_tokens",
                params![
                    identity, content_hash,
                    bucket.source, bucket.provider, bucket.model, bucket.project, bucket.hostname, bucket.bucket_start,
                    bucket.input_tokens, bucket.output_tokens, bucket.cached_input_tokens, bucket.reasoning_output_tokens, bucket.total_tokens,
                ],
            )?;
        }

        let mut session_stats = UpsertStats::default();
        for session in sessions {
            let identity = [session.source.as_str(), session.hostname.as_str(), session.session_hash.as_str()].join("\0");
            let payload = serde_json::to_string(session).unwrap_or_default();
            let content_hash = &hex::encode(Sha256::digest(payload.as_bytes()))[..16];
            let existing = tx
                .query_row(
                    "SELECT content_hash, message_count FROM usage_sessions WHERE identity = ?1",
                    [&identity],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?;
            match existing.as_ref() {
                Some((hash, _)) if hash == content_hash => { session_stats.unchanged += 1; continue; }
                Some((_, count)) if *count > session.message_count => { session_stats.unchanged += 1; continue; }
                Some(_) => session_stats.updated += 1,
                None => session_stats.inserted += 1,
            }
            tx.execute(
                "INSERT INTO usage_sessions (identity, content_hash, source, project, hostname,
                    session_hash, first_message_at, last_message_at, duration_seconds, active_seconds,
                    message_count, user_message_count, user_prompt_hours)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                 ON CONFLICT(identity) DO UPDATE SET content_hash=excluded.content_hash,
                    project=excluded.project, first_message_at=excluded.first_message_at,
                    last_message_at=excluded.last_message_at, duration_seconds=excluded.duration_seconds,
                    active_seconds=excluded.active_seconds, message_count=excluded.message_count,
                    user_message_count=excluded.user_message_count, user_prompt_hours=excluded.user_prompt_hours",
                params![identity, content_hash, session.source, session.project, session.hostname,
                    session.session_hash, session.first_message_at, session.last_message_at,
                    session.duration_seconds, session.active_seconds, session.message_count,
                    session.user_message_count, serde_json::to_string(&session.user_prompt_hours).unwrap_or_else(|_| "[]".into())],
            )?;
        }

        tx.commit()?;
        Ok((removed, bucket_stats, session_stats))
    }

    pub fn totals_between(
        &self,
        start: &str,
        end: &str,
        source: Option<&str>,
    ) -> Result<UsageTotals> {
        let columns = "COALESCE(SUM(input_tokens), 0),
                       COALESCE(SUM(output_tokens), 0),
                       COALESCE(SUM(cached_input_tokens), 0),
                       COALESCE(SUM(reasoning_output_tokens), 0),
                       COALESCE(SUM(total_tokens), 0)";
        let read = |row: &rusqlite::Row<'_>| {
            Ok(UsageTotals {
                input_tokens: row.get(0)?,
                output_tokens: row.get(1)?,
                cached_input_tokens: row.get(2)?,
                reasoning_output_tokens: row.get(3)?,
                total_tokens: row.get(4)?,
            })
        };

        match source {
            Some(source) => self.connection.query_row(
                &format!(
                    "SELECT {columns} FROM usage_buckets
                     WHERE bucket_start >= ?1 AND bucket_start < ?2 AND source = ?3"
                ),
                params![start, end, source],
                read,
            ),
            None => self.connection.query_row(
                &format!(
                    "SELECT {columns} FROM usage_buckets
                     WHERE bucket_start >= ?1 AND bucket_start < ?2"
                ),
                params![start, end],
                read,
            ),
        }
    }

    pub fn buckets_between(&self, start: &str, end: &str) -> Result<Vec<UsageBucket>> {
        let mut statement = self.connection.prepare(
            "SELECT source, provider, model, project, hostname, bucket_start,
                    input_tokens, output_tokens, cached_input_tokens,
                    reasoning_output_tokens, total_tokens
             FROM usage_buckets
             WHERE bucket_start >= ?1 AND bucket_start < ?2
             ORDER BY bucket_start, source, model, project, hostname",
        )?;
        let rows = statement.query_map(params![start, end], |row| {
            Ok(UsageBucket {
                source: row.get(0)?,
                provider: row.get(1)?,
                model: row.get(2)?,
                project: row.get(3)?,
                hostname: row.get(4)?,
                bucket_start: row.get(5)?,
                input_tokens: row.get(6)?,
                output_tokens: row.get(7)?,
                cached_input_tokens: row.get(8)?,
                reasoning_output_tokens: row.get(9)?,
                total_tokens: row.get(10)?,
            })
        })?;
        rows.collect()
    }

    pub fn sessions_between(&self, start: &str, end: &str) -> Result<Vec<UsageSession>> {
        let mut statement = self.connection.prepare(
            "SELECT source, project, hostname, session_hash, first_message_at, last_message_at,
                    duration_seconds, active_seconds, message_count, user_message_count, user_prompt_hours
             FROM usage_sessions WHERE first_message_at >= ?1 AND first_message_at < ?2
             ORDER BY first_message_at, source, session_hash")?;
        let rows = statement.query_map(params![start, end], |row| {
            let hours: String = row.get(10)?;
            Ok(UsageSession {
                source: row.get(0)?, project: row.get(1)?, hostname: row.get(2)?,
                session_hash: row.get(3)?, first_message_at: row.get(4)?, last_message_at: row.get(5)?,
                duration_seconds: row.get(6)?, active_seconds: row.get(7)?, message_count: row.get(8)?,
                user_message_count: row.get(9)?, user_prompt_hours: serde_json::from_str(&hours).unwrap_or_else(|_| vec![0; 24]),
            })
        })?;
        rows.collect()
    }

    /// 返回与窗口 [start, end) 时间重叠的会话（first_message_at < end AND last_message_at >= start）。
    /// 供每日报告等按日界统计的场景补全跨日界会话；活跃时长由调用方做窗口裁剪。
    pub fn sessions_overlapping(&self, start: &str, end: &str) -> Result<Vec<UsageSession>> {
        let mut statement = self.connection.prepare(
            "SELECT source, project, hostname, session_hash, first_message_at, last_message_at,
                    duration_seconds, active_seconds, message_count, user_message_count, user_prompt_hours
             FROM usage_sessions WHERE first_message_at < ?2 AND last_message_at >= ?1
             ORDER BY first_message_at, source, session_hash")?;
        let rows = statement.query_map(params![start, end], |row| {
            let hours: String = row.get(10)?;
            Ok(UsageSession {
                source: row.get(0)?, project: row.get(1)?, hostname: row.get(2)?,
                session_hash: row.get(3)?, first_message_at: row.get(4)?, last_message_at: row.get(5)?,
                duration_seconds: row.get(6)?, active_seconds: row.get(7)?, message_count: row.get(8)?,
                user_message_count: row.get(9)?, user_prompt_hours: serde_json::from_str(&hours).unwrap_or_else(|_| vec![0; 24]),
            })
        })?;
        rows.collect()
    }
}

fn bucket_identity(bucket: &UsageBucket) -> String {
    [
        bucket.source.as_str(),
        bucket.provider.as_str(),
        bucket.model.as_str(),
        bucket.project.as_str(),
        bucket.hostname.as_str(),
        bucket.bucket_start.as_str(),
    ]
    .join("\u{0}")
}

fn bucket_hash(bucket: &UsageBucket) -> String {
    let value = [
        bucket.input_tokens,
        bucket.output_tokens,
        bucket.cached_input_tokens,
        bucket.reasoning_output_tokens,
        bucket.total_tokens,
    ]
    .map(|token| token.to_string())
    .join("\u{0}");
    hex::encode(Sha256::digest(value.as_bytes()))[..16].to_string()
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    let found = names.flatten().any(|name| name == column);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(source: &str, project: &str, start: &str, input: i64, output: i64) -> UsageBucket {
        UsageBucket {
            source: source.into(),
            provider: "provider.test".into(),
            model: "model-a".into(),
            project: project.into(),
            hostname: "host-a".into(),
            bucket_start: start.into(),
            input_tokens: input,
            output_tokens: output,
            cached_input_tokens: 5,
            reasoning_output_tokens: 3,
            total_tokens: input + output + 3,
        }
    }

    fn session(source: &str, project: &str, hash: &str, first: &str, last: &str) -> UsageSession {
        UsageSession {
            source: source.into(),
            project: project.into(),
            hostname: "host-a".into(),
            session_hash: hash.into(),
            first_message_at: first.into(),
            last_message_at: last.into(),
            duration_seconds: 2400,
            active_seconds: 1200,
            message_count: 10,
            user_message_count: 3,
            user_prompt_hours: vec![0; 24],
        }
    }

    #[test]
    fn same_identity_updates_instead_of_duplicating() {
        let mut repository = UsageRepository::open_in_memory().unwrap();
        let first = bucket(
            "codex",
            "secret-project",
            "2026-07-31T10:00:00.000Z",
            100,
            20,
        );
        let mut changed = first.clone();
        changed.output_tokens = 40;
        changed.total_tokens = 143;

        assert_eq!(
            repository
                .upsert_buckets(&[first.clone()])
                .unwrap()
                .inserted,
            1
        );
        assert_eq!(repository.upsert_buckets(&[first]).unwrap().unchanged, 1);
        assert_eq!(repository.upsert_buckets(&[changed]).unwrap().updated, 1);

        let totals = repository
            .totals_between("2026-07-31T00:00:00.000Z", "2026-08-01T00:00:00.000Z", None)
            .unwrap();
        assert_eq!(totals.total_tokens, 143);
    }

    #[test]
    fn replace_source_is_idempotent_across_scans() {
        // 模拟完整扫描循环:同一数据源重复 replace_source,记录数必须保持不变
        // （事件型解析器幂等 + 快照型解析器 bucket_start 稳定,共同保证不重复累计）。
        let mut repository = UsageRepository::open_in_memory().unwrap();
        let buckets = vec![
            bucket("reasonix", "proj-a", "2026-08-06T04:00:00.000Z", 6_000_000, 60_000),
            bucket("reasonix", "proj-a", "2026-08-06T04:30:00.000Z", 250_000_000, 250_000),
        ];
        let sessions = vec![session("reasonix", "proj-a", "hash-one", "2026-08-06T04:00:00.000Z", "2026-08-06T04:40:00.000Z")];

        let (removed, _, _) = repository.replace_source("reasonix", &buckets, &sessions).unwrap();
        assert_eq!(removed, 0);
        let count_after_first = repository.source_record_count("reasonix").unwrap();
        assert_eq!(count_after_first, buckets.len() + sessions.len());

        // 第二次 replace 相同数据:先删后插,净记录数不变。
        let (removed, _, _) = repository.replace_source("reasonix", &buckets, &sessions).unwrap();
        assert_eq!(removed, count_after_first);
        let count_after_second = repository.source_record_count("reasonix").unwrap();
        assert_eq!(count_after_second, count_after_first, "重复 replace 不得产生重复记录");

        // 快照内容更新（同一 bucket_start,累计值增长）:记录数仍不变,总量更新。
        let mut grown = buckets.clone();
        grown[1].total_tokens = 260_000_000 + 250_000 + 3;
        let (_, _, _) = repository.replace_source("reasonix", &grown, &sessions).unwrap();
        assert_eq!(repository.source_record_count("reasonix").unwrap(), count_after_first);
        let totals = repository
            .totals_between("2026-08-06T00:00:00.000Z", "2026-08-07T00:00:00.000Z", None)
            .unwrap();
        assert_eq!(totals.total_tokens, 6_000_000 + 60_000 + 3 + 260_000_000 + 250_000 + 3);
    }

    #[test]
    fn totals_filter_by_time_and_source() {
        let mut repository = UsageRepository::open_in_memory().unwrap();
        repository
            .upsert_buckets(&[
                bucket("codex", "one", "2026-07-31T10:00:00.000Z", 100, 20),
                bucket("workbuddy", "two", "2026-07-31T10:30:00.000Z", 50, 10),
                bucket("codex", "one", "2026-08-01T10:00:00.000Z", 900, 90),
            ])
            .unwrap();

        let all = repository
            .totals_between("2026-07-31T00:00:00.000Z", "2026-08-01T00:00:00.000Z", None)
            .unwrap();
        let workbuddy = repository
            .totals_between(
                "2026-07-31T00:00:00.000Z",
                "2026-08-01T00:00:00.000Z",
                Some("workbuddy"),
            )
            .unwrap();

        assert_eq!(all.total_tokens, 186);
        assert_eq!(workbuddy.total_tokens, 63);
    }

    #[test]
    fn project_names_are_hidden_before_storage() {
        let hidden = bucket(
            "workbuddy",
            "private-client",
            "2026-07-31T10:00:00.000Z",
            1,
            1,
        )
        .hide_project();
        assert_eq!(hidden.project, "unknown");
    }

    #[test]
    fn range_query_returns_complete_buckets_in_time_order() {
        let mut repository = UsageRepository::open_in_memory().unwrap();
        repository
            .upsert_buckets(&[
                bucket("workbuddy", "two", "2026-07-31T10:30:00.000Z", 50, 10),
                bucket("codex", "one", "2026-07-31T10:00:00.000Z", 100, 20),
                bucket("codex", "one", "2026-08-01T10:00:00.000Z", 900, 90),
            ])
            .unwrap();

        let buckets = repository
            .buckets_between("2026-07-31T00:00:00.000Z", "2026-08-01T00:00:00.000Z")
            .unwrap();
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].source, "codex");
        assert_eq!(buckets[1].source, "workbuddy");
    }
}
