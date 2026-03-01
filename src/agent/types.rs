use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize)]
pub struct UsageInfo {
    pub agent_name: String,
    pub display_name: String,
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
    pub extra_usage: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UsageWindow {
    pub name: String,
    pub utilization_pct: f64,
    pub resets_at: Option<String>,
    #[serde(skip_serializing, default)]
    pub is_extra: bool,
    #[serde(skip_serializing, default)]
    pub source_limit_name: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct AgentDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub token_file_name: &'static str,
}
