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

pub fn normalize_quota_display_name(quota_name: &str) -> String {
    if quota_name == "code-review-seven-day" || quota_name == "code-review" {
        return "Code Review".to_string();
    }
    if quota_name == "five-hour" {
        return "5h limit".to_string();
    }
    if quota_name == "seven-day" {
        return "7d limit".to_string();
    }
    if quota_name == "seven-day-opus" {
        return "opus (7d)".to_string();
    }

    if let Some(prefix) = quota_name.strip_suffix("-five-hour") {
        return format!("{prefix} (5h)");
    }
    if let Some(prefix) = quota_name.strip_suffix("-seven-day-opus") {
        return format!("{prefix} opus (7d)");
    }
    if let Some(prefix) = quota_name.strip_suffix("-seven-day") {
        return format!("{prefix} (7d)");
    }

    quota_name.to_string()
}
