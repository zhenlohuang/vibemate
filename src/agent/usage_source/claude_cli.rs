use std::time::Duration;

use crate::agent::auth::token::AgentToken;
use crate::agent::usage_source::UsageSource;
use crate::agent::usage_source::cli_runner::{resolve_binary, run_command};
use crate::config::{AgentSourceConfig, UsageSourceKind};
use crate::error::{AppError, Result};
use async_trait::async_trait;

pub struct ClaudeCliSource {
    binary: Option<String>,
}

impl ClaudeCliSource {
    pub fn new(config: &AgentSourceConfig) -> Self {
        Self {
            binary: resolve_binary(config.cli_path.as_deref(), "claude"),
        }
    }
}

#[async_trait]
impl UsageSource for ClaudeCliSource {
    fn kind(&self) -> UsageSourceKind {
        UsageSourceKind::Cli
    }

    async fn is_available(&self) -> bool {
        self.binary.is_some()
    }

    async fn fetch_usage(
        &self,
        _token: Option<&AgentToken>,
        _client: &reqwest::Client,
    ) -> Result<crate::agent::UsageInfo> {
        let binary = self.binary.as_deref().ok_or_else(|| {
            AppError::CliSubprocess("Claude CLI binary is not available".to_string())
        })?;
        let _ = run_command(binary, &["--help"], None, Duration::from_secs(5)).await?;
        Err(AppError::CliSubprocess(format!(
            "Claude CLI at `{binary}` is available, but this build does not expose structured usage output"
        )))
    }
}
