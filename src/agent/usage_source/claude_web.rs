use async_trait::async_trait;
use serde_json::Value;

use crate::agent::auth::token::AgentToken;
use crate::agent::impls::claude;
use crate::agent::usage_source::UsageSource;
use crate::agent::usage_source::cookie::extract_cookie;
use crate::config::UsageSourceKind;
use crate::error::{AppError, Result};

pub struct ClaudeWebSource {
    pub cookie_browser: Option<String>,
}

#[async_trait]
impl UsageSource for ClaudeWebSource {
    fn kind(&self) -> UsageSourceKind {
        UsageSourceKind::Web
    }

    async fn is_available(&self) -> bool {
        self.cookie_browser.is_some()
    }

    async fn fetch_usage(
        &self,
        _token: Option<&AgentToken>,
        client: &reqwest::Client,
    ) -> Result<crate::agent::UsageInfo> {
        let cookie =
            extract_cookie(self.cookie_browser.as_deref(), "claude.ai", "sessionKey").await?;
        let organizations: Value = client
            .get("https://claude.ai/api/organizations")
            .header(reqwest::header::COOKIE, format!("sessionKey={cookie}"))
            .send()
            .await?
            .json()
            .await?;

        let org_id = organizations
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::OAuth(
                    "Claude web org list did not include an organization id".to_string(),
                )
            })?;

        let usage_value: Value = client
            .get(format!(
                "https://claude.ai/api/organizations/{org_id}/usage"
            ))
            .header(reqwest::header::COOKIE, format!("sessionKey={cookie}"))
            .send()
            .await?
            .json()
            .await?;

        let mut usage = claude::parse_usage_value(usage_value)?;
        usage.source = Some("web".to_string());
        Ok(usage)
    }
}
