use std::collections::HashMap;

use crate::config::AppConfig;

#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, Provider>,
}

impl ProviderRegistry {
    pub fn from_config(config: &AppConfig) -> Self {
        let providers = config
            .providers
            .iter()
            .map(|(name, cfg)| {
                (
                    name.clone(),
                    Provider {
                        name: name.clone(),
                        base_url: cfg.base_url.clone(),
                        headers: cfg.headers.clone(),
                    },
                )
            })
            .collect();
        Self { providers }
    }

    pub fn get(&self, name: &str) -> Option<&Provider> {
        self.providers.get(name)
    }
}
