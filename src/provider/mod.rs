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
                let mut headers = cfg.headers.clone();
                if let Some(api_key) = &cfg.api_key {
                    let has_authorization = headers
                        .keys()
                        .any(|key| key.eq_ignore_ascii_case("authorization"));
                    if !has_authorization {
                        headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
                    }
                }

                (
                    name.clone(),
                    Provider {
                        name: name.clone(),
                        base_url: cfg.base_url.clone(),
                        headers,
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

#[cfg(test)]
mod tests {
    use crate::config::AppConfig;

    use super::ProviderRegistry;

    #[test]
    fn injects_bearer_authorization_from_api_key() {
        let cfg = r#"
[providers.openai]
base_url = "https://api.openai.com/v1"
api_key = "sk-test"
"#;
        let app_config = toml::from_str::<AppConfig>(cfg).expect("config should parse");

        let registry = ProviderRegistry::from_config(&app_config);
        let provider = registry.get("openai").expect("provider should exist");

        assert_eq!(
            provider.headers.get("Authorization"),
            Some(&"Bearer sk-test".to_string())
        );
    }

    #[test]
    fn keeps_existing_authorization_header() {
        let cfg = r#"
[providers.custom]
base_url = "https://example.com/v1"
api_key = "sk-test"
headers = { authorization = "Token preset" }
"#;
        let app_config = toml::from_str::<AppConfig>(cfg).expect("config should parse");

        let registry = ProviderRegistry::from_config(&app_config);
        let provider = registry.get("custom").expect("provider should exist");

        assert_eq!(
            provider.headers.get("authorization"),
            Some(&"Token preset".to_string())
        );
        assert!(!provider.headers.contains_key("Authorization"));
    }
}
