use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const PROXY_ENV_KEYS: [&str; 6] = [
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
    "http_proxy",
    "HTTP_PROXY",
];

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub agents: AgentsConfig,
    pub providers: HashMap<String, ProviderConfig>,
    pub routing: RoutingConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            agents: AgentsConfig::default(),
            providers: HashMap::new(),
            routing: RoutingConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn show_extra_quota(&self) -> bool {
        self.agents.show_extra_quota
    }

    pub fn usage_refresh_interval(&self) -> Duration {
        let secs = self.agents.usage_refresh_interval_secs.max(1);
        Duration::from_secs(secs)
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub proxy: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 12_345,
            proxy: Some("http://127.0.0.1:7890".to_string()),
        }
    }
}

impl ServerConfig {
    pub fn build_http_client(&self) -> crate::error::Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder();
        if let Some((_, key)) = first_proxy_env_value(read_proxy_env_var) {
            tracing::debug!(
                "Using proxy from environment variable `{key}`; ignoring `[server].proxy`"
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
#[serde(default)]
pub struct AgentsConfig {
    pub show_extra_quota: bool,
    pub usage_refresh_interval_secs: u64,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            show_extra_quota: false,
            usage_refresh_interval_secs: 300,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProviderConfig {
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct RoutingConfig {
    pub default_provider: String,
    pub rules: Vec<RoutingRule>,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            default_provider: "openai-official".to_string(),
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RoutingRule {
    pub pattern: String,
    pub provider: String,
    pub model: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{ServerConfig, first_proxy_env_value, normalize_proxy_value};

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
        let config = ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 12_345,
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
}
