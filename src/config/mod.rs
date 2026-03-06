mod types;

pub use types::{
    AgentSourceConfig, AppConfig, RouterConfig, RouterLoggingConfig, RoutingRule, UsageSourceKind,
    validate_agent_usage_source,
};

use crate::error::{AppError, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_FULL_CONFIG: &str = r#"# VibeMate configuration

[system]
# Optional upstream HTTP/SOCKS proxy used for outbound provider requests
# proxy = "http://127.0.0.1:7890"

[router]
host = "127.0.0.1"
port = 12345
default_provider = "openai-official"
rules = [
  # Route all GPT-4o requests through openai-official
  #{ pattern = "gpt-4o*", provider = "openai-official" },
  # Example: route Claude models to anthropic
  #{ pattern = "claude-*", provider = "anthropic" },
  # Example: rewrite models while routing
  #{ pattern = "o1-mini", provider = "openrouter", model = "openai/o1-mini" }
]

[router.logging]
# Persist router access logs in JSON Lines format.
enabled = false
file_path = "~/.vibemate/logs/router-access.log"
max_file_size_mb = 20
max_files = 3

[agents]
# Show extra quotas (for example Codex additional_rate_limits) in `vibemate usage` and dashboard output
show_extra_quota = false
# Usage polling interval for dashboard in seconds (default 300s = 5min)
usage_refresh_interval_secs = 300

[agents.codex]
# auto | oauth | web | cli | local
usage_source = "auto"
# cli_path = "/opt/homebrew/bin/codex"
# session_dir = "~/.codex/sessions"

[agents.claude]
# auto | oauth | web | cli | local
usage_source = "auto"
# cli_path = "/opt/homebrew/bin/claude"
# session_dir = "~/.claude/projects"
# cookie_browser = "chrome"

[agents.cursor]
# auto | oauth | web
usage_source = "auto"
# cookie_browser = "chrome"

[agents.gemini]
# auto | oauth | local
usage_source = "auto"
# session_dir = "~/.gemini"

# Provider definitions map provider names to base URLs + auth headers.
# `api_key` will auto-generate `Authorization = "Bearer <api_key>"`.
[providers.openai-official]
base_url = "https://api.openai.com/v1"
api_key = "sk-your-openai-api-key"

#[providers.openrouter]
#base_url = "https://openrouter.ai/api/v1"
#api_key = "sk-or-v1-your-openrouter-key"
#headers = {
#  "HTTP-Referer" = "https://example.com",
#  "X-Title" = "VibeMate"
#}

#[providers.anthropic]
#base_url = "https://api.anthropic.com/v1"
#headers = {
#  "x-api-key" = "sk-ant-your-anthropic-key",
#  "anthropic-version" = "2023-06-01"
#}

"#;

pub fn ensure_config_initialized(path: &Path) -> Result<PathBuf> {
    ensure_vibemate_dir()?;

    let resolved_path = expand_tilde(path);
    if let Some(parent) = resolved_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if !resolved_path.exists() {
        fs::write(&resolved_path, DEFAULT_FULL_CONFIG)?;
    }

    Ok(resolved_path)
}

pub fn load_config(path: &Path) -> Result<AppConfig> {
    let resolved_path = ensure_config_initialized(path)?;
    let raw = fs::read_to_string(&resolved_path)?;
    let config = toml::from_str::<AppConfig>(&raw)?;
    config.validate()?;
    Ok(config)
}

fn ensure_vibemate_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| AppError::Config("Unable to find home directory".to_string()))?;
    let dir = home.join(".vibemate");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn expand_tilde(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }

    if let Some(suffix) = raw.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(suffix);
    }

    path.to_path_buf()
}
