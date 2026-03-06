use async_trait::async_trait;

use crate::agent::UsageInfo;
use crate::agent::auth::token::AgentToken;
use crate::config::UsageSourceKind;
use crate::error::{AppError, Result};

pub mod claude_cli;
pub mod claude_local;
pub mod claude_web;
pub mod cli_runner;
pub mod codex_cli;
pub mod codex_local;
pub mod codex_web;
pub mod cookie;
pub mod cursor_web;
pub mod gemini_local;
pub mod local_cost;

#[async_trait]
pub trait UsageSource: Send + Sync {
    fn kind(&self) -> UsageSourceKind;

    async fn is_available(&self) -> bool;

    async fn fetch_usage(
        &self,
        token: Option<&AgentToken>,
        client: &reqwest::Client,
    ) -> Result<UsageInfo>;
}

#[derive(Default)]
pub struct UsageFallbackChain {
    sources: Vec<Box<dyn UsageSource>>,
}

impl UsageFallbackChain {
    pub fn new(sources: Vec<Box<dyn UsageSource>>) -> Self {
        Self { sources }
    }

    pub async fn fetch_usage(
        &self,
        token: Option<&AgentToken>,
        client: &reqwest::Client,
    ) -> Result<UsageInfo> {
        if self.sources.is_empty() {
            return Err(AppError::NoUsageSources(
                "No usage sources configured".to_string(),
            ));
        }

        let mut attempted = 0usize;
        let mut errors = Vec::new();

        for source in &self.sources {
            if !source.is_available().await {
                continue;
            }
            attempted += 1;

            match source.fetch_usage(token, client).await {
                Ok(mut usage) => {
                    if usage.source.is_none() {
                        usage.source = Some(source.kind().as_str().to_string());
                    }
                    return Ok(usage);
                }
                Err(err) => errors.push(format!("{}: {err}", source.kind().as_str())),
            }
        }

        if attempted == 0 {
            return Err(AppError::NoUsageSources(
                "No available usage sources".to_string(),
            ));
        }

        Err(AppError::OAuth(format!(
            "Usage fallback failed: {}",
            errors.join(" | ")
        )))
    }
}
