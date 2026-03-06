use async_trait::async_trait;
use serde_json::json;

use crate::agent::auth::token::AgentToken;
use crate::agent::usage_source::UsageSource;
use crate::agent::usage_source::local_cost::{
    ensure_exists, local_scan_error, local_usage_info, normalize_path, summarize_local_plan,
};
use crate::config::{AgentSourceConfig, UsageSourceKind};
use crate::error::Result;

pub struct CodexLocalSource {
    session_dir: Option<String>,
}

impl CodexLocalSource {
    pub fn new(config: &AgentSourceConfig) -> Self {
        Self {
            session_dir: config.session_dir.clone(),
        }
    }
}

#[async_trait]
impl UsageSource for CodexLocalSource {
    fn kind(&self) -> UsageSourceKind {
        UsageSourceKind::Local
    }

    async fn is_available(&self) -> bool {
        default_state_db().exists()
    }

    async fn fetch_usage(
        &self,
        _token: Option<&AgentToken>,
        _client: &reqwest::Client,
    ) -> Result<crate::agent::UsageInfo> {
        let db_path = default_state_db();
        ensure_exists(&db_path, "Codex state DB")?;
        let start_ts = (chrono::Utc::now() - chrono::Duration::days(30)).timestamp();
        let sql = format!(
            "select coalesce(sum(tokens_used), 0) from threads where updated_at >= {start_ts};"
        );
        let total_tokens = query_single_integer(&db_path, &sql)? as u64;

        let session_root = normalize_path(self.session_dir.as_deref(), default_sessions_dir());
        let plan = summarize_local_plan("Local 30d", total_tokens, None);
        Ok(local_usage_info(
            "codex",
            "Codex",
            plan,
            vec![crate::agent::UsageWindow {
                name: "local-30-day".to_string(),
                utilization_pct: if total_tokens == 0 { 0.0 } else { 100.0 },
                resets_at: None,
                is_extra: false,
                source_limit_name: None,
            }],
            Some(json!({
                "session_root": session_root,
                "total_tokens": total_tokens,
                "window_days": 30,
            })),
        ))
    }
}

fn default_state_db() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".codex/state_5.sqlite")
}

fn default_sessions_dir() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_default().join(".codex/sessions")
}

fn query_single_integer(db_path: &std::path::Path, sql: &str) -> Result<i64> {
    let output = std::process::Command::new("sqlite3")
        .arg(db_path)
        .arg(sql)
        .output()
        .map_err(|err| local_scan_error(format!("Failed to run sqlite3: {err}")))?;
    if !output.status.success() {
        return Err(local_scan_error(format!(
            "sqlite3 failed for {}: {}",
            db_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .parse::<i64>()
        .map_err(|err| local_scan_error(format!("Failed to parse sqlite result: {err}")))
}
