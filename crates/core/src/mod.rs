pub mod codex;
pub mod kimi_code;
pub mod workbuddy;

use crate::usage::UsageBucket;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParserOutput {
    pub buckets: Vec<UsageBucket>,
    pub files_scanned: usize,
    pub usage_records: usize,
    pub malformed_lines: usize,
}
