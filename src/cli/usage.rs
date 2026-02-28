use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::agent::auth::token::{auth_file_path, save_token};
use crate::agent::{AgentUsageCapability, UsageInfo, UsageWindow, global_agent_registry};
use crate::config::AppConfig;
use crate::error::{AppError, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct UsageOptions {
    pub json: bool,
    pub raw: bool,
}

#[derive(Debug, Serialize)]
struct UsageJsonOutput {
    usage: Vec<UsageJsonAgent>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct UsageJsonAgent {
    agent_name: String,
    display_name: String,
    plan: Option<String>,
    quotas: Vec<UsageJsonQuota>,
    extra_quotas: Vec<UsageJsonQuota>,
}

#[derive(Debug, Serialize)]
struct UsageJsonQuota {
    quota_name: String,
    name: String,
    used_percent: f64,
    resets_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct UsageRawOutput {
    // Raw upstream usage payload keyed by agent name; no schema normalization.
    raw_usage: BTreeMap<String, Value>,
    warnings: Vec<String>,
}

pub async fn run(config: &AppConfig, options: UsageOptions) -> Result<()> {
    let registry = global_agent_registry();
    let mut usage_results = Vec::new();
    let mut raw_results = BTreeMap::new();
    let mut errors = Vec::new();
    let mut found_any_token = false;
    let client = config.server.build_http_client()?;

    for agent_impl in registry.iter() {
        let agent_id = agent_impl.descriptor().id;
        let Some(auth) = agent_impl.auth_capability() else {
            errors.push(format!("{agent_id} capability missing: auth"));
            continue;
        };
        let Some(usage_capability) = agent_impl.usage_capability() else {
            errors.push(format!("{agent_id} capability missing: usage"));
            continue;
        };

        match auth.load_saved_token().await {
            Ok(Some(mut token)) => {
                found_any_token = true;
                if let Err(err) = auth.refresh_if_needed(&mut token, &client).await {
                    errors.push(format!("{agent_id} refresh error: {err}"));
                } else {
                    match auth_file_path(agent_impl.descriptor().token_file_name) {
                        Ok(path) => {
                            if let Err(err) = save_token(&path, &token) {
                                errors.push(format!("{agent_id} token save error: {err}"));
                            }
                        }
                        Err(err) => errors.push(format!("{agent_id} token path error: {err}")),
                    }

                    if options.raw {
                        match usage_capability.get_usage_raw(&token, &client).await {
                            Ok(value) => {
                                raw_results.insert(agent_id.to_string(), value);
                            }
                            Err(err) => errors.push(format!("{agent_id} usage error: {err}")),
                        }
                    } else {
                        match usage_capability.get_usage(&token, &client).await {
                            Ok(info) => usage_results.push(info),
                            Err(err) => errors.push(format!("{agent_id} usage error: {err}")),
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(err) => errors.push(format!("{agent_id} token load error: {err}")),
        }
    }

    let has_data = if options.raw {
        !raw_results.is_empty()
    } else {
        !usage_results.is_empty()
    };

    if options.raw {
        let output = UsageRawOutput {
            raw_usage: raw_results,
            warnings: errors.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if options.json {
        let usage_json: Vec<UsageJsonAgent> =
            usage_results.iter().map(to_usage_json_agent).collect();
        let output = UsageJsonOutput {
            usage: usage_json,
            warnings: errors.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if !usage_results.is_empty() {
            print_usage_table(&usage_results, config.show_extra_quota());
        }
        if !errors.is_empty() {
            eprintln!("\nUsage warnings");
            eprintln!("==============");
            for error in &errors {
                eprintln!("- {error}");
            }
        }
    }

    if !found_any_token {
        if options.raw {
            return Ok(());
        }
        if options.json {
            return Ok(());
        }
        let supported_logins = registry
            .supported_ids()
            .into_iter()
            .map(|name| format!("`vibemate login {name}`"))
            .collect::<Vec<_>>()
            .join(" or ");
        println!("No login tokens found. Run {supported_logins}.");
        return Ok(());
    }

    if !has_data {
        if options.raw || options.json {
            return Ok(());
        }
        return Err(AppError::OAuth(
            "No usage data could be fetched.".to_string(),
        ));
    }

    Ok(())
}

fn print_usage_table(items: &[UsageInfo], show_extra_quota: bool) {
    println!("\nUsage Summary");
    println!("=============");

    for item in items {
        let plan = item.plan.clone().unwrap_or_else(|| "unknown".to_string());
        let display_name = if item.display_name.trim().is_empty() {
            item.agent_name.as_str()
        } else {
            item.display_name.as_str()
        };
        println!("\n{} (plan: {})", display_name, plan);
        let windows: Vec<_> = item
            .windows
            .iter()
            .filter(|window| {
                should_display_window(window) && (show_extra_quota || !window.is_extra)
            })
            .collect();
        if windows.is_empty() {
            println!("  - no window data reported by provider");
            continue;
        }
        for window in windows {
            let reset = window
                .resets_at
                .clone()
                .unwrap_or_else(|| "n/a".to_string());
            let display_name = derive_display_name(&item.agent_name, window);
            println!(
                "  - {:14} {:>6.2}%   resets_at={} ",
                display_name, window.utilization_pct, reset
            );
        }
    }
}

fn to_usage_json_agent(info: &UsageInfo) -> UsageJsonAgent {
    let mut quotas = Vec::new();
    let mut extra_quotas = Vec::new();
    for window in info
        .windows
        .iter()
        .filter(|window| should_display_window(window))
    {
        let quota = UsageJsonQuota {
            quota_name: derive_quota_name(&info.agent_name, window),
            name: derive_display_name(&info.agent_name, window),
            used_percent: window.utilization_pct,
            resets_at: window.resets_at.clone(),
        };
        if window.is_extra {
            extra_quotas.push(quota);
        } else {
            quotas.push(quota);
        }
    }

    UsageJsonAgent {
        agent_name: info.agent_name.clone(),
        display_name: info.display_name.clone(),
        plan: info.plan.clone(),
        quotas,
        extra_quotas,
    }
}

fn derive_quota_name(agent_name: &str, window: &UsageWindow) -> String {
    if let Some(agent) = lookup_usage_capability(agent_name) {
        return agent.quota_name(window);
    }

    window.name.to_string()
}

pub fn derive_display_name(agent_name: &str, window: &UsageWindow) -> String {
    if let Some(agent) = lookup_usage_capability(agent_name) {
        return agent.display_quota_name(window);
    }

    normalize_quota_display_name(&window.name)
}

fn lookup_usage_capability(agent_name: &str) -> Option<&'static dyn AgentUsageCapability> {
    global_agent_registry()
        .get(agent_name)
        .and_then(|agent| agent.usage_capability())
}

fn should_display_quota(window: &UsageWindow) -> bool {
    if !window.utilization_pct.is_finite() {
        return false;
    }
    match &window.resets_at {
        Some(value) => !value.trim().is_empty(),
        None => false,
    }
}

fn should_display_extra_quota(window: &UsageWindow) -> bool {
    window.utilization_pct.is_finite()
}

pub fn should_display_window(window: &UsageWindow) -> bool {
    if window.is_extra {
        return should_display_extra_quota(window);
    }
    should_display_quota(window)
}

fn normalize_quota_display_name(quota_name: &str) -> String {
    crate::agent::normalize_quota_display_name(quota_name)
}

#[cfg(test)]
mod tests {
    use crate::agent::{UsageInfo, UsageWindow};

    use super::{
        derive_display_name, derive_quota_name, normalize_quota_display_name, to_usage_json_agent,
    };

    #[test]
    fn normalizes_common_quota_names() {
        assert_eq!(normalize_quota_display_name("five-hour"), "5h limit");
        assert_eq!(normalize_quota_display_name("seven-day"), "7d limit");
        assert_eq!(normalize_quota_display_name("seven-day-opus"), "opus (7d)");
    }

    #[test]
    fn keeps_model_specific_quota_context() {
        assert_eq!(
            normalize_quota_display_name("gpt-5-3-codex-spark-five-hour"),
            "gpt-5-3-codex-spark (5h)"
        );
        assert_eq!(
            normalize_quota_display_name("code-review-seven-day"),
            "Code Review"
        );
        assert_eq!(
            normalize_quota_display_name("gpt-5-3-codex-spark-seven-day"),
            "gpt-5-3-codex-spark (7d)"
        );
        assert_eq!(
            normalize_quota_display_name("gpt-5-3-codex-spark-seven-day-opus"),
            "gpt-5-3-codex-spark opus (7d)"
        );
    }

    #[test]
    fn codex_additional_rate_limits_use_stable_quota_name() {
        let codex_extra_week = UsageWindow {
            name: "gpt-5-3-codex-spark-seven-day".to_string(),
            is_extra: true,
            source_limit_name: Some("GPT-5.3-Codex-Spark".to_string()),
            ..Default::default()
        };
        let codex_extra_session = UsageWindow {
            name: "gpt-5-3-codex-spark-five-hour".to_string(),
            is_extra: true,
            source_limit_name: Some("GPT-5.3-Codex-Spark".to_string()),
            ..Default::default()
        };
        let codex_base = UsageWindow {
            name: "five-hour".to_string(),
            is_extra: false,
            ..Default::default()
        };

        assert_eq!(
            derive_quota_name("codex", &codex_extra_week),
            "GPT-5.3-Codex-Spark"
        );
        assert_eq!(
            derive_quota_name("codex", &codex_extra_session),
            "GPT-5.3-Codex-Spark"
        );
        assert_eq!(derive_quota_name("codex", &codex_base), "five-hour");
        assert_eq!(
            derive_display_name("codex", &codex_extra_session),
            "GPT-5.3-Codex-Spark(5h)"
        );
        assert_eq!(
            derive_display_name("codex", &codex_extra_week),
            "GPT-5.3-Codex-Spark(7d)"
        );
    }

    #[test]
    fn only_codex_extra_uses_stable_quota_name() {
        let claude_extra = UsageWindow {
            name: "sonnet-4".to_string(),
            is_extra: true,
            ..Default::default()
        };
        assert_eq!(derive_quota_name("claude-code", &claude_extra), "sonnet-4");
    }

    #[test]
    fn usage_json_splits_extra_quotas_for_claude() {
        let info = UsageInfo {
            agent_name: "claude-code".to_string(),
            display_name: "Claude Code".to_string(),
            plan: None,
            windows: vec![
                UsageWindow {
                    name: "five-hour".to_string(),
                    utilization_pct: 6.0,
                    resets_at: Some("2026-02-27T20:00:00Z".to_string()),
                    is_extra: false,
                    source_limit_name: None,
                },
                UsageWindow {
                    name: "extra-usage".to_string(),
                    utilization_pct: 0.0,
                    resets_at: Some("2026-03-01T00:00:00Z".to_string()),
                    is_extra: true,
                    source_limit_name: None,
                },
            ],
            extra_usage: None,
        };

        let json_agent = to_usage_json_agent(&info);
        assert_eq!(json_agent.quotas.len(), 1);
        assert_eq!(json_agent.extra_quotas.len(), 1);
        assert_eq!(json_agent.extra_quotas[0].quota_name, "extra-usage");
        assert_eq!(json_agent.extra_quotas[0].name, "extra-usage");
    }

    #[test]
    fn usage_json_keeps_extra_quota_without_reset_at() {
        let info = UsageInfo {
            agent_name: "claude-code".to_string(),
            display_name: "Claude Code".to_string(),
            plan: None,
            windows: vec![
                UsageWindow {
                    name: "five-hour".to_string(),
                    utilization_pct: 6.0,
                    resets_at: None,
                    is_extra: false,
                    source_limit_name: None,
                },
                UsageWindow {
                    name: "extra-usage".to_string(),
                    utilization_pct: 0.0,
                    resets_at: None,
                    is_extra: true,
                    source_limit_name: None,
                },
            ],
            extra_usage: None,
        };

        let json_agent = to_usage_json_agent(&info);
        assert!(json_agent.quotas.is_empty());
        assert_eq!(json_agent.extra_quotas.len(), 1);
        assert_eq!(json_agent.extra_quotas[0].quota_name, "extra-usage");
        assert!(json_agent.extra_quotas[0].resets_at.is_none());
    }
}
