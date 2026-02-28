use crate::config::{RouterConfig, RoutingRule};

#[derive(Debug, Clone)]
pub struct ResolvedRoute {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct ModelRouter {
    default_provider: String,
    rules: Vec<RoutingRule>,
}

impl ModelRouter {
    pub fn from_config(config: &RouterConfig) -> Self {
        Self {
            default_provider: config.default_provider.clone(),
            rules: config.rules.clone(),
        }
    }

    pub fn resolve(&self, model: &str) -> ResolvedRoute {
        for rule in &self.rules {
            if glob_match(&rule.pattern, model) {
                return ResolvedRoute {
                    provider: rule.provider.clone(),
                    model: rule.model.clone().unwrap_or_else(|| model.to_string()),
                };
            }
        }

        ResolvedRoute {
            provider: self.default_provider.clone(),
            model: model.to_string(),
        }
    }
}

fn glob_match(pattern: &str, input: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if !pattern.contains('*') {
        return pattern == input;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    let anchored_start = !pattern.starts_with('*');
    let anchored_end = !pattern.ends_with('*');

    let mut cursor = 0usize;

    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if index == 0 && anchored_start {
            if !input[cursor..].starts_with(part) {
                return false;
            }
            cursor += part.len();
            continue;
        }

        if let Some(found) = input[cursor..].find(part) {
            cursor += found + part.len();
        } else {
            return false;
        }
    }

    if anchored_end {
        if let Some(last) = parts.iter().rev().find(|part| !part.is_empty()) {
            return input.ends_with(last);
        }
    }

    true
}
