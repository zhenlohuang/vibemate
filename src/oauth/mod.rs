pub mod callback;
pub mod claude;
pub mod codex;
pub mod pkce;
pub mod token;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::sync::OnceLock;

use crate::error::Result;
use crate::oauth::token::TokenData;

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

#[async_trait]
pub trait OAuthAgent: Send + Sync {
    fn descriptor(&self) -> &'static AgentDescriptor;
    async fn login(&self) -> Result<()>;
    async fn load_saved_token(&self) -> Result<Option<TokenData>>;
    async fn refresh_if_needed(&self, token: &mut TokenData) -> Result<()>;
    async fn get_usage(&self, token: &TokenData) -> Result<UsageInfo>;
    async fn get_usage_raw(&self, token: &TokenData) -> Result<Value>;

    fn quota_name(&self, window: &UsageWindow) -> String {
        window.name.clone()
    }

    fn display_quota_name(&self, window: &UsageWindow) -> String {
        normalize_quota_display_name(&window.name)
    }
}

pub struct AgentRegistry {
    agents: Vec<Box<dyn OAuthAgent>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: vec![Box::new(codex::CodexAgent), Box::new(claude::ClaudeAgent)],
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn OAuthAgent> + '_ {
        self.agents.iter().map(Box::as_ref)
    }

    pub fn get(&self, id: &str) -> Option<&dyn OAuthAgent> {
        self.iter().find(|agent| agent.descriptor().id == id)
    }

    pub fn supported_ids(&self) -> Vec<&'static str> {
        self.iter().map(|agent| agent.descriptor().id).collect()
    }
}

static GLOBAL_AGENT_REGISTRY: OnceLock<AgentRegistry> = OnceLock::new();

pub fn global_agent_registry() -> &'static AgentRegistry {
    GLOBAL_AGENT_REGISTRY.get_or_init(AgentRegistry::new)
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
