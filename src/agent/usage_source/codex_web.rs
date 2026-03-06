use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::agent::auth::token::AgentToken;
use crate::agent::impls::codex;
use crate::agent::usage_source::UsageSource;
use crate::config::UsageSourceKind;
use crate::error::{AppError, Result};

pub struct CodexWebSource;

#[derive(Debug, Deserialize)]
struct CodexAuthFile {
    tokens: Option<CodexAuthTokens>,
}

#[derive(Debug, Deserialize)]
struct CodexAuthTokens {
    access_token: Option<String>,
}

#[async_trait]
impl UsageSource for CodexWebSource {
    fn kind(&self) -> UsageSourceKind {
        UsageSourceKind::Web
    }

    async fn is_available(&self) -> bool {
        load_access_token().is_ok()
    }

    async fn fetch_usage(
        &self,
        _token: Option<&AgentToken>,
        client: &reqwest::Client,
    ) -> Result<crate::agent::UsageInfo> {
        let access_token = load_access_token()?;
        let response = client
            .get(codex::USAGE_URL)
            .bearer_auth(access_token)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(AppError::OAuth(format!(
                "Codex web usage request failed with status {}",
                response.status()
            )));
        }

        let value: Value = response.json().await?;
        let mut usage = codex::parse_usage_value(value);
        usage.source = Some("web".to_string());
        Ok(usage)
    }
}

fn load_access_token() -> Result<String> {
    let home = dirs::home_dir()
        .ok_or_else(|| AppError::OAuth("Unable to resolve home directory".to_string()))?;
    let auth_path = home.join(".codex/auth.json");
    let raw = std::fs::read_to_string(&auth_path)
        .map_err(|err| AppError::OAuth(format!("Failed to read {}: {err}", auth_path.display())))?;
    let auth: CodexAuthFile = serde_json::from_str(&raw).map_err(|err| {
        AppError::OAuth(format!("Failed to parse {}: {err}", auth_path.display()))
    })?;
    auth.tokens
        .and_then(|tokens| tokens.access_token)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::OAuth(format!("No access_token in {}", auth_path.display())))
}
