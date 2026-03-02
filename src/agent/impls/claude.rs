use std::collections::HashSet;
use std::io;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::auth::pkce::{generate_challenge, generate_state, generate_verifier};
use crate::agent::auth::token::{AgentToken, auth_file_path, load_token, save_token};
use crate::agent::{
    Agent, AgentAuthCapability, AgentDescriptor, AgentIdentity, AgentUsageCapability, UsageInfo,
    UsageWindow,
};
use crate::error::{AppError, Result};

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTH_URL: &str = "https://claude.ai/oauth/authorize";
pub const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
pub const REDIRECT_URI: &str = "https://console.anthropic.com/oauth/code/callback";
pub const SCOPE: &str = "org:create_api_key user:profile user:inference";
pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const ANTHROPIC_BETA: &str = "oauth-2025-04-20";
const TOKEN_FILE_NAME: &str = "claude_auth.json";
pub const DESCRIPTOR: AgentDescriptor = AgentDescriptor {
    id: "claude-code",
    display_name: "Claude Code",
    token_file_name: TOKEN_FILE_NAME,
};

pub struct ClaudeAgent;

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Debug, Serialize)]
struct AuthCodeExchange<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    redirect_uri: &'a str,
    code: &'a str,
    code_verifier: &'a str,
    state: &'a str,
}

#[derive(Debug, Serialize)]
struct RefreshExchange<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    refresh_token: &'a str,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsageResponse {
    five_hour: Option<UsageBucket>,
    seven_day: Option<UsageBucket>,
    seven_day_opus: Option<UsageBucket>,
    #[serde(default)]
    extra_usage: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct UsageBucket {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

fn usage_window(name: &str, bucket: Option<UsageBucket>) -> Option<UsageWindow> {
    let bucket = bucket?;
    let utilization_pct = bucket.utilization.filter(|value| value.is_finite());
    let resets_at = bucket.resets_at.filter(|value| !value.trim().is_empty());
    if utilization_pct.is_none() && resets_at.is_none() {
        return None;
    }

    Some(UsageWindow {
        name: name.to_string(),
        utilization_pct: utilization_pct.unwrap_or(0.0),
        resets_at,
        is_extra: false,
        source_limit_name: None,
    })
}

pub async fn login(client: &reqwest::Client) -> Result<()> {
    let verifier = generate_verifier();
    let challenge = generate_challenge(&verifier);
    let state = generate_state();

    let auth_url = format!(
        "{AUTH_URL}?code=true&client_id={CLIENT_ID}&response_type=code&redirect_uri={REDIRECT_URI}&scope={SCOPE}&code_challenge={challenge}&code_challenge_method=S256&state={state}"
    );

    tracing::info!("Open this URL in your browser if it does not open automatically: {auth_url}");
    let _ = open::that(&auth_url);

    println!("Paste the code shown in browser (format: code#state):");
    let pasted = tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map(|_| input.trim().to_string())
    })
    .await
    .map_err(|e| AppError::OAuth(format!("Failed to read pasted code: {e}")))?
    .map_err(AppError::Io)?;

    let (code, returned_state) = pasted
        .split_once('#')
        .ok_or_else(|| AppError::OAuth("Expected pasted value in format code#state".to_string()))?;

    if returned_state != state {
        return Err(AppError::OAuth(
            "State mismatch in Claude OAuth response".to_string(),
        ));
    }

    let response = client
        .post(TOKEN_URL)
        .json(&AuthCodeExchange {
            grant_type: "authorization_code",
            client_id: CLIENT_ID,
            redirect_uri: REDIRECT_URI,
            code,
            code_verifier: &verifier,
            state: &state,
        })
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Claude token exchange failed with status {}",
            response.status()
        )));
    }

    let payload: TokenResponse = response.json().await?;
    let token = AgentToken {
        access_token: payload.access_token,
        refresh_token: payload.refresh_token,
        expires_at: Utc::now() + Duration::seconds(payload.expires_in.unwrap_or(3600)),
        last_refresh: Some(Utc::now()),
    };

    let path = auth_file_path(TOKEN_FILE_NAME)?;
    save_token(&path, &token)?;
    Ok(())
}

pub async fn refresh_if_needed(token: &mut AgentToken, client: &reqwest::Client) -> Result<()> {
    let now = Utc::now();
    if token.expires_at - now > Duration::minutes(5) {
        return Ok(());
    }

    let refresh_token = token
        .refresh_token
        .as_deref()
        .ok_or_else(|| AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        })?;

    let response = client
        .post(TOKEN_URL)
        .json(&RefreshExchange {
            grant_type: "refresh_token",
            client_id: CLIENT_ID,
            refresh_token,
        })
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        });
    }

    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Claude token refresh failed with status {}",
            response.status()
        )));
    }

    let payload: TokenResponse = response.json().await?;
    token.access_token = payload.access_token;
    token.refresh_token = payload.refresh_token.or(token.refresh_token.take());
    token.expires_at = now + Duration::seconds(payload.expires_in.unwrap_or(3600));
    token.last_refresh = Some(now);

    let path = auth_file_path(TOKEN_FILE_NAME)?;
    save_token(&path, token)?;
    Ok(())
}

pub async fn get_usage(token: &AgentToken, client: &reqwest::Client) -> Result<UsageInfo> {
    let value = get_usage_raw(token, client).await?;
    parse_usage(value)
}

fn parse_usage(value: Value) -> Result<UsageInfo> {
    let usage: ClaudeUsageResponse = serde_json::from_value(value)?;
    let extra_usage = usage.extra_usage.clone();
    let mut windows = Vec::new();
    for maybe_window in [
        usage_window("five-hour", usage.five_hour),
        usage_window("seven-day", usage.seven_day),
        usage_window("seven-day-opus", usage.seven_day_opus),
    ] {
        if let Some(window) = maybe_window {
            windows.push(window);
        }
    }

    if let Some(extra) = usage.extra_usage {
        merge_extra_usage_windows(&mut windows, &extra);
    }

    Ok(UsageInfo {
        agent_name: DESCRIPTOR.id.to_string(),
        display_name: DESCRIPTOR.display_name.to_string(),
        plan: None,
        windows,
        extra_usage,
    })
}

fn merge_extra_usage_windows(windows: &mut Vec<UsageWindow>, extra: &Value) {
    let mut seen: HashSet<String> = windows.iter().map(|w| w.name.clone()).collect();

    if let Some(window) = parse_extra_window("extra-usage", extra) {
        if seen.insert(window.name.clone()) {
            windows.push(window);
        }
    }

    if let Some(items) = extra.as_array() {
        for (index, item) in items.iter().enumerate() {
            let base_name = item
                .get("name")
                .or_else(|| item.get("quota_name"))
                .or_else(|| item.get("window"))
                .or_else(|| item.get("type"))
                .and_then(Value::as_str)
                .map(normalize_name)
                .unwrap_or_else(|| format!("extra-{}", index + 1));
            collect_extra_windows(item, &base_name, windows, &mut seen);
        }
        return;
    }

    if let Some(map) = extra.as_object() {
        for (key, item) in map {
            let base_name = normalize_name(key);
            collect_extra_windows(item, &base_name, windows, &mut seen);
        }
    }
}

fn collect_extra_windows(
    value: &Value,
    base_name: &str,
    windows: &mut Vec<UsageWindow>,
    seen: &mut HashSet<String>,
) {
    if let Some(window) = parse_extra_window(base_name, value) {
        if seen.insert(window.name.clone()) {
            windows.push(window);
        }
    }

    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let normalized_key = normalize_name(key);
                let child_name = if is_passthrough_extra_key(&normalized_key) {
                    base_name.to_string()
                } else if normalized_key.is_empty() {
                    base_name.to_string()
                } else {
                    format!("{base_name}-{normalized_key}")
                };
                collect_extra_windows(child, &child_name, windows, seen);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let child_base = item
                    .get("name")
                    .or_else(|| item.get("quota_name"))
                    .or_else(|| item.get("window"))
                    .or_else(|| item.get("type"))
                    .and_then(Value::as_str)
                    .map(normalize_name)
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("{base_name}-{}", index + 1));
                collect_extra_windows(item, &child_base, windows, seen);
            }
        }
        _ => {}
    }
}

fn is_passthrough_extra_key(key: &str) -> bool {
    matches!(
        key,
        "usage"
            | "window"
            | "bucket"
            | "current"
            | "current-window"
            | "rate-limit"
            | "primary-window"
            | "secondary-window"
    )
}

fn parse_extra_window(name: &str, value: &Value) -> Option<UsageWindow> {
    let utilization_pct = parse_utilization_pct(
        value,
        &[
            "utilization",
            "used_percent",
            "utilization_pct",
            "usage_percent",
            "percent_used",
        ],
    )
    .filter(|value| value.is_finite());
    let resets_at = parse_string(
        value,
        &[
            "resets_at",
            "reset_at",
            "next_reset_at",
            "resetAt",
            "resetsAt",
            "reset_after_seconds",
            "reset_after",
            "seconds_until_reset",
            "period_end",
            "cycle_end",
            "billing_cycle_end",
        ],
    );
    let resets_at = resets_at.filter(|value| !value.trim().is_empty());
    if utilization_pct.is_none() && resets_at.is_none() {
        return None;
    }

    Some(UsageWindow {
        name: name.to_string(),
        utilization_pct: utilization_pct.unwrap_or(0.0),
        resets_at,
        is_extra: true,
        source_limit_name: None,
    })
}

fn parse_number_raw(value: &Value, keys: &[&str]) -> Option<f64> {
    for key in keys {
        if let Some(v) = value.get(*key) {
            if let Some(n) = v.as_f64() {
                return Some(n);
            }
            if let Some(s) = v.as_str() {
                if let Ok(n) = s.parse::<f64>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn parse_percent_field(value: &Value, keys: &[&str]) -> Option<f64> {
    parse_number_raw(value, keys).map(|v| if v <= 1.0 { v * 100.0 } else { v })
}

fn parse_utilization_pct(value: &Value, keys: &[&str]) -> Option<f64> {
    if let Some(v) = parse_percent_field(value, keys) {
        return Some(v);
    }

    if let Some(v) = parse_number_raw(
        value,
        &[
            "utilization_ratio",
            "usage_ratio",
            "percent_ratio",
            "ratio",
            "used_ratio",
        ],
    ) {
        return Some(v * 100.0);
    }

    let used = parse_number_raw(
        value,
        &[
            "used",
            "consumed",
            "total_used",
            "current_usage",
            "usage",
            "spent",
            "used_credits",
        ],
    );
    let limit = parse_number_raw(
        value,
        &[
            "limit",
            "max",
            "total",
            "allowance",
            "quota",
            "monthly_limit",
        ],
    );
    if let (Some(used), Some(limit)) = (used, limit) {
        if limit > 0.0 {
            return Some((used / limit) * 100.0);
        }
    }

    let remaining = parse_number_raw(value, &["remaining", "left", "available"]);
    if let (Some(remaining), Some(limit)) = (remaining, limit) {
        if limit > 0.0 {
            return Some(((limit - remaining) / limit) * 100.0);
        }
    }

    None
}

fn parse_string(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(field) = value.get(*key) {
            if let Some(seconds) = field.as_i64() {
                if key.ends_with("after_seconds")
                    || key.ends_with("after")
                    || key.ends_with("until_reset")
                {
                    let ts = Utc::now().timestamp().saturating_add(seconds);
                    if let Some(dt) = chrono::DateTime::<Utc>::from_timestamp(ts, 0) {
                        return Some(dt.to_rfc3339());
                    }
                }
            }
            if let Some(seconds) = field.as_u64() {
                if key.ends_with("after_seconds")
                    || key.ends_with("after")
                    || key.ends_with("until_reset")
                {
                    if let Ok(sec_i64) = i64::try_from(seconds) {
                        let ts = Utc::now().timestamp().saturating_add(sec_i64);
                        if let Some(dt) = chrono::DateTime::<Utc>::from_timestamp(ts, 0) {
                            return Some(dt.to_rfc3339());
                        }
                    }
                }
            }
            if let Some(s) = field.as_str() {
                return Some(s.to_string());
            }
            if let Some(ts) = field.as_i64() {
                if let Some(dt) = chrono::DateTime::<Utc>::from_timestamp(ts, 0) {
                    return Some(dt.to_rfc3339());
                }
            }
            if let Some(ts) = field.as_u64() {
                if let Ok(ts_i64) = i64::try_from(ts) {
                    if let Some(dt) = chrono::DateTime::<Utc>::from_timestamp(ts_i64, 0) {
                        return Some(dt.to_rfc3339());
                    }
                }
            }
        }
    }
    None
}

fn normalize_name(input: &str) -> String {
    input.trim().to_ascii_lowercase().replace([' ', '_'], "-")
}

pub async fn get_usage_raw(token: &AgentToken, client: &reqwest::Client) -> Result<Value> {
    let response = client
        .get(USAGE_URL)
        .bearer_auth(&token.access_token)
        .header("anthropic-beta", ANTHROPIC_BETA)
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        });
    }

    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Claude usage request failed with status {}",
            response.status()
        )));
    }

    let value: Value = response.json().await?;
    Ok(value)
}

pub async fn load_saved_token() -> Result<Option<AgentToken>> {
    let path = auth_file_path(TOKEN_FILE_NAME)?;
    load_token(&path)
}

impl AgentIdentity for ClaudeAgent {
    fn descriptor(&self) -> &'static AgentDescriptor {
        &DESCRIPTOR
    }
}

impl Agent for ClaudeAgent {
    fn auth_capability(&self) -> Option<&dyn AgentAuthCapability> {
        Some(self)
    }

    fn usage_capability(&self) -> Option<&dyn AgentUsageCapability> {
        Some(self)
    }
}

#[async_trait]
impl AgentAuthCapability for ClaudeAgent {
    async fn login(&self, client: &reqwest::Client) -> Result<()> {
        login(client).await
    }

    async fn load_saved_token(&self) -> Result<Option<AgentToken>> {
        load_saved_token().await
    }

    async fn refresh_if_needed(
        &self,
        token: &mut AgentToken,
        client: &reqwest::Client,
    ) -> Result<()> {
        refresh_if_needed(token, client).await
    }
}

#[async_trait]
impl AgentUsageCapability for ClaudeAgent {
    async fn get_usage(&self, token: &AgentToken, client: &reqwest::Client) -> Result<UsageInfo> {
        get_usage(token, client).await
    }

    async fn get_usage_raw(&self, token: &AgentToken, client: &reqwest::Client) -> Result<Value> {
        get_usage_raw(token, client).await
    }

    fn process_quota_name(&self, quota_name: &str) -> String {
        const DISPLAY_NAME_MAP: [(&str, &str); 3] = [
            ("five-hour", "Session"),
            ("seven-day", "Weekly"),
            ("extra-usage", "Extra Usage"),
        ];
        DISPLAY_NAME_MAP
            .iter()
            .find_map(|(name, display_name)| (*name == quota_name).then_some(*display_name))
            .unwrap_or(quota_name)
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_usage;

    #[test]
    fn parse_usage_includes_extra_usage_object() {
        let value = json!({
            "five_hour": { "utilization": 6.0, "resets_at": "2026-02-27T20:00:00Z" },
            "seven_day": { "utilization": 1.0, "resets_at": "2026-03-06T15:00:00Z" },
            "seven_day_opus": null,
            "extra_usage": {
                "sonnet_4": { "used_percent": 23, "reset_at": 1772760122 }
            }
        });

        let usage = parse_usage(value).expect("parse should succeed");
        assert!(
            usage
                .windows
                .iter()
                .any(|w| { w.name == "sonnet-4" && (w.utilization_pct - 23.0).abs() < 0.0001 })
        );
    }

    #[test]
    fn parse_usage_includes_extra_usage_used_limit_and_reset_after() {
        let value = json!({
            "five_hour": { "utilization": 6.0, "resets_at": "2026-02-27T20:00:00Z" },
            "seven_day": { "utilization": 1.0, "resets_at": "2026-03-06T15:00:00Z" },
            "seven_day_opus": null,
            "extra_usage": {
                "sonnet_4": { "used": 30, "limit": 100, "reset_after_seconds": 3600 }
            }
        });

        let usage = parse_usage(value).expect("parse should succeed");
        assert!(usage.windows.iter().any(|w| {
            w.name == "sonnet-4"
                && (w.utilization_pct - 30.0).abs() < 0.0001
                && w.resets_at.is_some()
        }));
    }

    #[test]
    fn parse_usage_includes_monthly_extra_usage_aggregate_object() {
        let value = json!({
            "five_hour": { "utilization": 7.0, "resets_at": "2026-02-27T20:00:00Z" },
            "seven_day": { "utilization": 1.0, "resets_at": "2026-03-06T15:00:00Z" },
            "extra_usage": {
                "is_enabled": true,
                "monthly_limit": 6500,
                "used_credits": 0.0,
                "utilization": null
            }
        });

        let usage = parse_usage(value).expect("parse should succeed");
        let extra = usage
            .windows
            .iter()
            .find(|w| w.name == "extra-usage")
            .expect("extra usage window should exist");
        assert!(extra.is_extra);
        assert!(extra.resets_at.is_none());
        assert!(extra.utilization_pct.abs() < 0.0001);
    }

    #[test]
    fn parse_usage_keeps_base_window_without_reset_at() {
        let value = json!({
            "five_hour": { "utilization": 6.0, "resets_at": null },
            "seven_day": null,
            "seven_day_opus": null
        });

        let usage = parse_usage(value).expect("parse should succeed");
        let five_hour = usage
            .windows
            .iter()
            .find(|w| w.name == "five-hour")
            .expect("five-hour window should exist");
        assert!((five_hour.utilization_pct - 6.0).abs() < 0.0001);
        assert!(five_hour.resets_at.is_none());
    }

    #[test]
    fn parse_usage_keeps_base_window_with_reset_at_without_utilization() {
        let value = json!({
            "five_hour": { "utilization": null, "resets_at": "2026-02-27T20:00:00Z" },
            "seven_day": null,
            "seven_day_opus": null
        });

        let usage = parse_usage(value).expect("parse should succeed");
        let five_hour = usage
            .windows
            .iter()
            .find(|w| w.name == "five-hour")
            .expect("five-hour window should exist");
        assert!(five_hour.utilization_pct.abs() < 0.0001);
        assert_eq!(five_hour.resets_at.as_deref(), Some("2026-02-27T20:00:00Z"));
    }
}
