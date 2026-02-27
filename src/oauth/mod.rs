pub mod callback;
pub mod claude;
pub mod codex;
pub mod pkce;
pub mod token;

use async_trait::async_trait;

use crate::error::Result;

#[derive(Debug, Clone, Default)]
pub struct UsageInfo {
    pub agent_name: String,
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
}

#[derive(Debug, Clone, Default)]
pub struct UsageWindow {
    pub name: String,
    pub utilization_pct: f64,
    pub resets_at: Option<String>,
}

#[async_trait]
pub trait OAuthAgent: Send + Sync {
    fn name(&self) -> &str;
    async fn login(&self) -> Result<()>;
    fn is_logged_in(&self) -> bool;
    async fn get_usage(&self) -> Result<UsageInfo>;
    async fn refresh_if_needed(&mut self) -> Result<()>;
}
