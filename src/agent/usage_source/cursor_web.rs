use async_trait::async_trait;

use crate::agent::auth::token::AgentToken;
use crate::agent::impls::cursor;
use crate::agent::usage_source::UsageSource;
use crate::agent::usage_source::cookie::extract_cookie;
use crate::config::UsageSourceKind;
use crate::error::Result;

pub struct CursorWebSource {
    pub cookie_browser: Option<String>,
}

#[async_trait]
impl UsageSource for CursorWebSource {
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
        let cookie = extract_cookie(
            self.cookie_browser.as_deref(),
            "cursor.com",
            "WorkosCursorSessionToken",
        )
        .await?;
        let token = AgentToken {
            access_token: cookie,
            refresh_token: None,
            expires_at: chrono::Utc::now() + chrono::Duration::days(30),
            last_refresh: Some(chrono::Utc::now()),
        };
        let mut usage = cursor::get_usage(&token, client).await?;
        usage.source = Some("web".to_string());
        Ok(usage)
    }
}
