use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use crate::error::{AppError, Result};

const PROXY_ENV_KEYS: [&str; 6] = [
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
    "http_proxy",
    "HTTP_PROXY",
];

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub system: SystemConfig,
    pub router: RouterConfig,
    pub agents: AgentsConfig,
    pub providers: HashMap<String, ProviderConfig>,
}

impl AppConfig {
    pub fn show_extra_quota(&self) -> bool {
        self.agents.show_extra_quota
    }

    pub fn usage_refresh_interval(&self) -> Duration {
        let secs = self.agents.usage_refresh_interval_secs.max(1);
        Duration::from_secs(secs)
    }

    pub fn agent_source_config(&self, agent_id: &str) -> &AgentSourceConfig {
        self.agents.agent_source_config(agent_id)
    }

    pub fn validate(&self) -> Result<()> {
        self.agents.validate()
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SystemConfig {
    pub proxy: Option<String>,
}

impl SystemConfig {
    pub fn build_http_client(&self) -> crate::error::Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder();
        if let Some((_, key)) = first_proxy_env_value(read_proxy_env_var) {
            tracing::debug!(
                "Using proxy from environment variable `{key}`; ignoring `[system].proxy`"
            );
        } else if let Some(proxy_url) = self.config_proxy_url() {
            let proxy_config = reqwest::Proxy::all(proxy_url).map_err(|e| {
                crate::error::AppError::Config(format!(
                    "Invalid network proxy URL '{proxy_url}': {e}"
                ))
            })?;
            builder = builder.proxy(proxy_config);
        }
        builder.build().map_err(|e| {
            crate::error::AppError::Config(format!("Failed to build HTTP client: {e}"))
        })
    }

    fn config_proxy_url(&self) -> Option<&str> {
        self.proxy.as_deref().and_then(normalize_proxy_value)
    }
}

fn first_proxy_env_value<F>(mut lookup: F) -> Option<(String, &'static str)>
where
    F: FnMut(&str) -> Option<String>,
{
    for key in PROXY_ENV_KEYS {
        if let Some(value) = lookup(key).as_deref().and_then(normalize_proxy_value) {
            return Some((value.to_string(), key));
        }
    }
    None
}

fn normalize_proxy_value(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn read_proxy_env_var(key: &str) -> Option<String> {
    std::env::var_os(key).map(|value| value.to_string_lossy().to_string())
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct AgentsConfig {
    pub show_extra_quota: bool,
    pub usage_refresh_interval_secs: u64,
    pub codex: AgentSourceConfig,
    pub claude: AgentSourceConfig,
    pub cursor: AgentSourceConfig,
    pub gemini: AgentSourceConfig,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            show_extra_quota: false,
            usage_refresh_interval_secs: 300,
            codex: AgentSourceConfig::default(),
            claude: AgentSourceConfig::default(),
            cursor: AgentSourceConfig::default(),
            gemini: AgentSourceConfig::default(),
        }
    }
}

impl AgentsConfig {
    pub fn agent_source_config(&self, agent_id: &str) -> &AgentSourceConfig {
        match agent_id {
            "codex" => &self.codex,
            "claude" => &self.claude,
            "cursor" => &self.cursor,
            "gemini" => &self.gemini,
            _ => &DEFAULT_AGENT_SOURCE_CONFIG,
        }
    }

    pub fn validate(&self) -> Result<()> {
        validate_agent_usage_source("codex", self.codex.usage_source)?;
        validate_agent_usage_source("claude", self.claude.usage_source)?;
        validate_agent_usage_source("cursor", self.cursor.usage_source)?;
        validate_agent_usage_source("gemini", self.gemini.usage_source)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UsageSourceKind {
    #[default]
    Auto,
    Oauth,
    Cli,
    Web,
    Local,
}

impl UsageSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Oauth => "oauth",
            Self::Cli => "cli",
            Self::Web => "web",
            Self::Local => "local",
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct AgentSourceConfig {
    pub usage_source: UsageSourceKind,
    pub cli_path: Option<String>,
    pub session_dir: Option<String>,
    pub cookie_browser: Option<String>,
}

impl Default for AgentSourceConfig {
    fn default() -> Self {
        Self {
            usage_source: UsageSourceKind::Auto,
            cli_path: None,
            session_dir: None,
            cookie_browser: Some("chrome".to_string()),
        }
    }
}

static DEFAULT_AGENT_SOURCE_CONFIG: LazyLock<AgentSourceConfig> =
    LazyLock::new(AgentSourceConfig::default);

const ALL_USAGE_SOURCES: &[UsageSourceKind] = &[
    UsageSourceKind::Auto,
    UsageSourceKind::Oauth,
    UsageSourceKind::Cli,
    UsageSourceKind::Web,
    UsageSourceKind::Local,
];
const CURSOR_USAGE_SOURCES: &[UsageSourceKind] = &[
    UsageSourceKind::Auto,
    UsageSourceKind::Oauth,
    UsageSourceKind::Web,
];
const GEMINI_USAGE_SOURCES: &[UsageSourceKind] = &[
    UsageSourceKind::Auto,
    UsageSourceKind::Oauth,
    UsageSourceKind::Local,
];

pub fn validate_agent_usage_source(agent_id: &str, usage_source: UsageSourceKind) -> Result<()> {
    let supported = match agent_id {
        "cursor" => CURSOR_USAGE_SOURCES,
        "gemini" => GEMINI_USAGE_SOURCES,
        "codex" | "claude" => ALL_USAGE_SOURCES,
        _ => ALL_USAGE_SOURCES,
    };

    if supported.contains(&usage_source) {
        return Ok(());
    }

    let supported_values = supported
        .iter()
        .map(|kind| format!("\"{}\"", kind.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    Err(AppError::Config(format!(
        "Agent `{agent_id}` does not support usage_source = \"{}\". Supported values: {supported_values}",
        usage_source.as_str()
    )))
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct RouterConfig {
    pub host: String,
    pub port: u16,
    pub default_provider: String,
    pub rules: Vec<RoutingRule>,
    pub logging: RouterLoggingConfig,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 12_345,
            default_provider: "openai-official".to_string(),
            rules: Vec::new(),
            logging: RouterLoggingConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct RouterLoggingConfig {
    pub enabled: bool,
    pub file_path: String,
    pub max_file_size_mb: u64,
    pub max_files: u32,
}

impl Default for RouterLoggingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            file_path: "~/.vibemate/logs/router-access.log".to_string(),
            max_file_size_mb: 20,
            max_files: 3,
        }
    }
}

impl RouterLoggingConfig {
    pub fn max_file_size_bytes(&self) -> u64 {
        self.max_file_size_mb.max(1) * 1024 * 1024
    }

    pub fn max_files_or_default(&self) -> u32 {
        self.max_files.max(1)
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RoutingRule {
    pub pattern: String,
    pub provider: String,
    pub model: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        AppConfig, SystemConfig, UsageSourceKind, first_proxy_env_value, normalize_proxy_value,
        validate_agent_usage_source,
    };

    #[test]
    fn system_config_default_proxy_is_none() {
        let config = SystemConfig::default();
        assert_eq!(config.proxy, None);
    }

    #[test]
    fn proxy_env_uses_expected_precedence() {
        let env_map = HashMap::from([
            (
                "http_proxy".to_string(),
                "http://127.0.0.1:7000".to_string(),
            ),
            (
                "all_proxy".to_string(),
                "socks5h://127.0.0.1:8000".to_string(),
            ),
            (
                "https_proxy".to_string(),
                "http://127.0.0.1:9000".to_string(),
            ),
        ]);

        let result = first_proxy_env_value(|key| env_map.get(key).cloned());
        assert_eq!(
            result,
            Some(("http://127.0.0.1:9000".to_string(), "https_proxy"))
        );
    }

    #[test]
    fn proxy_env_ignores_empty_values() {
        let env_map = HashMap::from([
            ("https_proxy".to_string(), "   ".to_string()),
            ("all_proxy".to_string(), "".to_string()),
            (
                "http_proxy".to_string(),
                "http://127.0.0.1:7890".to_string(),
            ),
        ]);

        let result = first_proxy_env_value(|key| env_map.get(key).cloned());
        assert_eq!(
            result,
            Some(("http://127.0.0.1:7890".to_string(), "http_proxy"))
        );
    }

    #[test]
    fn config_proxy_supports_socks5_url() {
        let config = SystemConfig {
            proxy: Some("socks5h://127.0.0.1:7890".to_string()),
        };
        assert_eq!(config.config_proxy_url(), Some("socks5h://127.0.0.1:7890"));
    }

    #[test]
    fn normalize_proxy_value_trims_whitespace() {
        assert_eq!(
            normalize_proxy_value("  http://127.0.0.1:7890  "),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(normalize_proxy_value(" \t "), None);
    }

    #[test]
    fn app_config_accepts_router_without_logging_section() {
        let value = r#"
            [router]
            host = "127.0.0.1"
            port = 12345
            default_provider = "openai-official"
            rules = []

            [providers.openai-official]
            base_url = "https://api.openai.com/v1"
        "#;

        let parsed: AppConfig = toml::from_str(value).expect("config should parse");
        assert!(!parsed.router.logging.enabled);
        assert_eq!(
            parsed.router.logging.file_path,
            "~/.vibemate/logs/router-access.log"
        );
        assert_eq!(parsed.router.logging.max_file_size_mb, 20);
        assert_eq!(parsed.router.logging.max_files, 3);
    }

    #[test]
    fn app_config_supports_custom_router_logging_section() {
        let value = r#"
            [router]
            host = "127.0.0.1"
            port = 12345
            default_provider = "openai-official"
            rules = []

            [router.logging]
            enabled = true
            file_path = "/tmp/router.log"
            max_file_size_mb = 7
            max_files = 5

            [providers.openai-official]
            base_url = "https://api.openai.com/v1"
        "#;

        let parsed: AppConfig = toml::from_str(value).expect("config should parse");
        assert!(parsed.router.logging.enabled);
        assert_eq!(parsed.router.logging.file_path, "/tmp/router.log");
        assert_eq!(parsed.router.logging.max_file_size_mb, 7);
        assert_eq!(parsed.router.logging.max_files, 5);
    }

    #[test]
    fn app_config_supports_agent_usage_source_overrides() {
        let value = r#"
            [agents]
            show_extra_quota = true
            usage_refresh_interval_secs = 60

            [agents.codex]
            usage_source = "web"
            cli_path = "/opt/homebrew/bin/codex"
            session_dir = "~/.codex/sessions"

            [agents.claude]
            usage_source = "local"
            cookie_browser = "firefox"

            [agents.cursor]
            usage_source = "web"
            cookie_browser = "firefox"

            [agents.gemini]
            usage_source = "local"
            session_dir = "~/.gemini"

            [router]
            host = "127.0.0.1"
            port = 12345
            default_provider = "openai-official"
            rules = []

            [providers.openai-official]
            base_url = "https://api.openai.com/v1"
        "#;

        let parsed: AppConfig = toml::from_str(value).expect("config should parse");
        assert!(parsed.agents.show_extra_quota);
        assert_eq!(parsed.agents.codex.usage_source, UsageSourceKind::Web);
        assert_eq!(
            parsed.agents.codex.cli_path.as_deref(),
            Some("/opt/homebrew/bin/codex")
        );
        assert_eq!(parsed.agents.claude.usage_source, UsageSourceKind::Local);
        assert_eq!(
            parsed.agents.claude.cookie_browser.as_deref(),
            Some("firefox")
        );
        assert_eq!(parsed.agents.cursor.usage_source, UsageSourceKind::Web);
        assert_eq!(parsed.agents.gemini.usage_source, UsageSourceKind::Local);
        assert_eq!(
            parsed.agents.gemini.session_dir.as_deref(),
            Some("~/.gemini")
        );
    }

    #[test]
    fn agent_source_config_defaults_cookie_browser_to_chrome() {
        let config = AppConfig::default();

        assert_eq!(
            config.agents.codex.cookie_browser.as_deref(),
            Some("chrome")
        );
        assert_eq!(
            config.agents.claude.cookie_browser.as_deref(),
            Some("chrome")
        );
        assert_eq!(
            config.agents.cursor.cookie_browser.as_deref(),
            Some("chrome")
        );
        assert_eq!(
            config.agents.gemini.cookie_browser.as_deref(),
            Some("chrome")
        );
        assert_eq!(
            config
                .agent_source_config("unknown")
                .cookie_browser
                .as_deref(),
            Some("chrome")
        );
    }

    #[test]
    fn cursor_rejects_unsupported_usage_sources() {
        let err = validate_agent_usage_source("cursor", UsageSourceKind::Local).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Configuration error: Agent `cursor` does not support usage_source = \"local\". Supported values: \"auto\", \"oauth\", \"web\""
        );
    }

    #[test]
    fn gemini_rejects_unsupported_usage_sources() {
        let err = validate_agent_usage_source("gemini", UsageSourceKind::Web).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Configuration error: Agent `gemini` does not support usage_source = \"web\". Supported values: \"auto\", \"oauth\", \"local\""
        );
    }
}
