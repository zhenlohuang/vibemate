use async_trait::async_trait;

use crate::agent::auth::token::AgentToken;
use crate::agent::impls::gemini;
use crate::agent::usage_source::UsageSource;
use crate::config::{AgentSourceConfig, UsageSourceKind};
use crate::error::{AppError, Result};

pub struct GeminiLocalSource {
    session_dir: Option<String>,
}

impl GeminiLocalSource {
    pub fn new(config: &AgentSourceConfig) -> Self {
        Self {
            session_dir: config.session_dir.clone(),
        }
    }

    fn creds_path(&self) -> Result<std::path::PathBuf> {
        let default = gemini::local_oauth_creds_path()?;
        let Some(session_dir) = self.session_dir.as_deref() else {
            return Ok(default);
        };

        let base = if session_dir == "~" || session_dir.starts_with("~/") {
            crate::config::expand_tilde(std::path::Path::new(session_dir))
        } else {
            std::path::PathBuf::from(session_dir)
        };

        if base.extension().is_some() {
            Ok(base)
        } else {
            Ok(base.join("oauth_creds.json"))
        }
    }
}

#[async_trait]
impl UsageSource for GeminiLocalSource {
    fn kind(&self) -> UsageSourceKind {
        UsageSourceKind::Local
    }

    async fn is_available(&self) -> bool {
        self.creds_path().is_ok_and(|path| path.exists())
    }

    async fn fetch_usage(
        &self,
        _token: Option<&AgentToken>,
        client: &reqwest::Client,
    ) -> Result<crate::agent::UsageInfo> {
        let path = self.creds_path()?;
        let raw = std::fs::read_to_string(&path).map_err(|err| {
            AppError::LocalScan(format!("Failed to read {}: {err}", path.display()))
        })?;
        let value: serde_json::Value = serde_json::from_str(&raw).map_err(|err| {
            AppError::LocalScan(format!("Failed to parse {}: {err}", path.display()))
        })?;
        let mut token = gemini::import_local_oauth_creds(&value)?;
        gemini::refresh_if_needed(&mut token, client).await?;
        let mut usage = gemini::get_usage(&token, client).await?;
        usage.source = Some("local".to_string());
        Ok(usage)
    }
}
