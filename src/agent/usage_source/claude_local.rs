use async_trait::async_trait;
use serde_json::{Value, json};

use crate::agent::auth::token::AgentToken;
use crate::agent::usage_source::UsageSource;
use crate::agent::usage_source::local_cost::{
    expand_home, local_scan_error, local_usage_info, normalize_path, parse_naive_date,
    rolling_window_start, summarize_local_plan,
};
use crate::config::{AgentSourceConfig, UsageSourceKind};
use crate::error::Result;

pub struct ClaudeLocalSource {
    session_dir: Option<String>,
}

impl ClaudeLocalSource {
    pub fn new(config: &AgentSourceConfig) -> Self {
        Self {
            session_dir: config.session_dir.clone(),
        }
    }
}

#[async_trait]
impl UsageSource for ClaudeLocalSource {
    fn kind(&self) -> UsageSourceKind {
        UsageSourceKind::Local
    }

    async fn is_available(&self) -> bool {
        stats_cache_path().exists() || default_projects_dir().exists()
    }

    async fn fetch_usage(
        &self,
        _token: Option<&AgentToken>,
        _client: &reqwest::Client,
    ) -> Result<crate::agent::UsageInfo> {
        match load_from_stats_cache() {
            Ok(info) => Ok(info),
            Err(_) => load_from_project_jsonl(self.session_dir.as_deref()),
        }
    }
}

fn load_from_stats_cache() -> Result<crate::agent::UsageInfo> {
    let path = stats_cache_path();
    let raw = std::fs::read_to_string(&path)
        .map_err(|err| local_scan_error(format!("Failed to read {}: {err}", path.display())))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| local_scan_error(format!("Failed to parse {}: {err}", path.display())))?;

    let start = rolling_window_start(30);
    let mut tokens_by_model = std::collections::BTreeMap::<String, u64>::new();
    let mut total_tokens = 0u64;

    if let Some(entries) = value.get("dailyModelTokens").and_then(Value::as_array) {
        for entry in entries {
            let Some(date) = entry.get("date").and_then(Value::as_str) else {
                continue;
            };
            let Some(day) = parse_naive_date(date) else {
                continue;
            };
            if day < start {
                continue;
            }
            if let Some(map) = entry.get("tokensByModel").and_then(Value::as_object) {
                for (model, token_value) in map {
                    let tokens = token_value.as_u64().unwrap_or_default();
                    total_tokens = total_tokens.saturating_add(tokens);
                    *tokens_by_model
                        .entry(normalize_model_name(model))
                        .or_default() += tokens;
                }
            }
        }
    }

    let total_cost = value
        .get("modelUsage")
        .and_then(Value::as_object)
        .map(|models| {
            models
                .values()
                .filter_map(|item| item.get("costUSD").and_then(Value::as_f64))
                .sum::<f64>()
        })
        .unwrap_or_default();

    let windows = build_model_share_windows(&tokens_by_model, total_tokens);
    let plan = summarize_local_plan("Local 30d", total_tokens, Some(total_cost));
    Ok(local_usage_info(
        "claude",
        "Claude",
        plan,
        windows,
        Some(json!({
            "total_tokens": total_tokens,
            "cost_usd": total_cost,
            "window_days": 30,
            "source_file": path,
        })),
    ))
}

fn load_from_project_jsonl(session_dir: Option<&str>) -> Result<crate::agent::UsageInfo> {
    let root = normalize_path(session_dir, default_projects_dir());
    let start = rolling_window_start(30);
    let mut tokens_by_model = std::collections::BTreeMap::<String, u64>::new();
    let mut total_tokens = 0u64;

    for file in collect_jsonl_files(&root)? {
        let raw = match std::fs::read_to_string(&file) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        for line in raw.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(timestamp) = value
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(crate::agent::usage_source::local_cost::parse_rfc3339_timestamp)
            else {
                continue;
            };
            if timestamp < start {
                continue;
            }
            let usage = value
                .get("message")
                .and_then(|message| message.get("usage"))
                .or_else(|| value.get("usage"));
            let Some(usage) = usage else {
                continue;
            };
            let model = value
                .get("message")
                .and_then(|message| message.get("model"))
                .and_then(Value::as_str)
                .unwrap_or("claude");
            let tokens = usage
                .get("input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or_default()
                + usage
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
                + usage
                    .get("cache_creation_input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
                + usage
                    .get("cache_read_input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
            total_tokens = total_tokens.saturating_add(tokens);
            *tokens_by_model
                .entry(normalize_model_name(model))
                .or_default() += tokens;
        }
    }

    let windows = build_model_share_windows(&tokens_by_model, total_tokens);
    let plan = summarize_local_plan("Local 30d", total_tokens, None);
    Ok(local_usage_info(
        "claude",
        "Claude",
        plan,
        windows,
        Some(json!({
            "total_tokens": total_tokens,
            "window_days": 30,
            "source_dir": root,
        })),
    ))
}

fn collect_jsonl_files(root: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Err(local_scan_error(format!(
            "Claude session directory not found at {}",
            root.display()
        )));
    }

    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|err| local_scan_error(format!("Failed to read {}: {err}", dir.display())))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
    Ok(files)
}

fn build_model_share_windows(
    tokens_by_model: &std::collections::BTreeMap<String, u64>,
    total_tokens: u64,
) -> Vec<crate::agent::UsageWindow> {
    if total_tokens == 0 {
        return vec![crate::agent::UsageWindow {
            name: "local-30-day".to_string(),
            utilization_pct: 0.0,
            resets_at: None,
            is_extra: false,
            source_limit_name: None,
        }];
    }

    tokens_by_model
        .iter()
        .map(|(model, tokens)| crate::agent::UsageWindow {
            name: model.clone(),
            utilization_pct: (*tokens as f64 / total_tokens as f64) * 100.0,
            resets_at: None,
            is_extra: false,
            source_limit_name: None,
        })
        .collect()
}

fn normalize_model_name(model: &str) -> String {
    model
        .trim()
        .trim_start_matches("global.anthropic.")
        .replace([' ', '_', ':'], "-")
        .to_ascii_lowercase()
}

fn stats_cache_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude/stats-cache.json")
}

fn default_projects_dir() -> std::path::PathBuf {
    expand_home(std::path::Path::new("~/.claude/projects"))
}
