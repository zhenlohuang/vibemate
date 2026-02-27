use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

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
            proxy: Some("socks5://127.0.0.1:7890".to_string()),
        }
    }
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
