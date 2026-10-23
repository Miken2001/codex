//! Server-owned background work; viewing these records never invokes a model.
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTaskInfo {
    pub id: String,
    pub process_id: String,
    pub label: String,
    pub kind: String,
    pub status: String,
    pub command: String,
    #[ts(type = "number")]
    pub started_at: i64,
    #[ts(type = "number | null")]
    pub completed_at: Option<i64>,
    pub exit_code: Option<i32>,
    /// Bounded output tail. The normal command transcript remains available separately.
    pub output: String,
}
