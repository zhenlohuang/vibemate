use async_trait::async_trait;
use chrono::{Duration, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::io::ErrorKind;
use url::Url;

use crate::agent::auth::pkce::{generate_challenge, generate_state, generate_verifier};
use crate::agent::auth::token::{AgentToken, auth_file_path, load_token, save_token};
use crate::agent::{
    Agent, AgentAuthCapability, AgentDescriptor, AgentIdentity, AgentUsageCapability, UsageInfo,
    UsageWindow, normalize_quota_display_name,
};
use crate::error::{AppError, Result};

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const CALLBACK_PORT: u16 = 1455;
pub const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const TOKEN_FILE_NAME: &str = "codex_auth.json";
pub const DESCRIPTOR: AgentDescriptor = AgentDescriptor {
    id: "codex",
    display_name: "Codex",
    token_file_name: TOKEN_FILE_NAME,
};

pub struct CodexAgent;

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
}

#[derive(Debug, Serialize)]
struct RefreshExchange<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    refresh_token: &'a str,
}

pub async fn login() -> Result<()> {
    let verifier = generate_verifier();
    let challenge = generate_challenge(&verifier);
    let expected_state = generate_state();

    let mut auth_url =
        Url::parse(AUTH_URL).map_err(|e| AppError::OAuth(format!("Invalid AUTH_URL: {e}")))?;
    auth_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &expected_state);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", CALLBACK_PORT))
        .await
        .map_err(|e| {
            if e.kind() == ErrorKind::AddrInUse {
                AppError::OAuth(format!(
                    "Failed to bind callback server on 127.0.0.1:{CALLBACK_PORT}: address already in use. Stop the process currently using this port and retry."
                ))
            } else {
                AppError::OAuth(format!("Failed to bind callback server: {e}"))
            }
        })?;

    let callback = tokio::spawn(crate::agent::auth::callback::start_callback_server(
        listener,
    ));

    let auth_url_string = auth_url.to_string();
    tracing::info!(
        "Open this URL in your browser if it does not open automatically: {auth_url_string}"
    );
    let _ = open::that(&auth_url_string);

    let callback_payload = callback
        .await
        .map_err(|e| AppError::OAuth(format!("Callback task failed: {e}")))??;

    if let Some(error) = callback_payload.error {
        let description = callback_payload
            .error_description
            .unwrap_or_else(|| "No error description".to_string());
        return Err(AppError::OAuth(format!(
            "Codex OAuth callback error: {error} ({description})"
        )));
    }

    let returned_state = callback_payload.state.unwrap_or_default();
    if returned_state != expected_state {
        return Err(AppError::OAuth(
            "State mismatch in Codex OAuth response".to_string(),
        ));
    }

    let code = callback_payload.code.ok_or_else(|| {
        AppError::OAuth("Codex OAuth callback did not include a code parameter".to_string())
    })?;

    let client = reqwest::Client::new();
    let token_res = client
        .post(TOKEN_URL)
        .json(&AuthCodeExchange {
            grant_type: "authorization_code",
            client_id: CLIENT_ID,
            redirect_uri: REDIRECT_URI,
            code: &code,
            code_verifier: &verifier,
        })
        .send()
        .await?;

    if !token_res.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Codex token exchange failed with status {}",
            token_res.status()
        )));
    }

    let token_payload: TokenResponse = token_res.json().await?;
    let token = AgentToken {
        access_token: token_payload.access_token,
        refresh_token: token_payload.refresh_token,
        expires_at: Utc::now() + Duration::seconds(token_payload.expires_in.unwrap_or(3600)),
        last_refresh: Some(Utc::now()),
    };

    let path = auth_file_path(TOKEN_FILE_NAME)?;
    save_token(&path, &token)?;
    Ok(())
}

pub async fn refresh_if_needed(token: &mut AgentToken) -> Result<()> {
    let now = Utc::now();
    let expiring_soon = token.expires_at - now <= Duration::minutes(5);
    let refresh_stale = token
        .last_refresh
        .map(|last_refresh| now.signed_duration_since(last_refresh) >= Duration::days(8))
        .unwrap_or(false);
    if !expiring_soon && !refresh_stale {
        return Ok(());
    }

    let refresh_token = token
        .refresh_token
        .as_deref()
        .ok_or_else(|| AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        })?;

    let client = reqwest::Client::new();
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
            "Codex token refresh failed with status {}",
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

pub async fn get_usage(token: &AgentToken) -> Result<UsageInfo> {
    let value = get_usage_raw(token).await?;
    Ok(parse_usage(value))
}

pub async fn get_usage_raw(token: &AgentToken) -> Result<Value> {
    let client = reqwest::Client::new();
    let response = client
        .get(USAGE_URL)
        .bearer_auth(&token.access_token)
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        });
    }

    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Codex usage request failed with status {}",
            response.status()
        )));
    }

    let value: Value = response.json().await?;
    if std::env::var_os("VIBEMATE_DEBUG_USAGE_JSON").is_some() {
        eprintln!("codex usage raw json: {value}");
    }
    Ok(value)
}

pub async fn load_saved_token() -> Result<Option<AgentToken>> {
    let path = auth_file_path(TOKEN_FILE_NAME)?;
    load_token(&path)
}

fn parse_usage(value: Value) -> UsageInfo {
    let plan = value
        .get("plan")
        .and_then(Value::as_str)
        .or_else(|| value.get("plan_type").and_then(Value::as_str))
        .or_else(|| value.get("subscription_plan").and_then(Value::as_str))
        .map(ToString::to_string);

    let mut windows = Vec::new();
    let mut seen = HashSet::<String>::new();
    let mut push_window = |window: UsageWindow| {
        if seen.insert(window.name.clone()) {
            windows.push(window);
        }
    };

    if let Some(rate_limit) = value.get("rate_limit") {
        for window in parse_rate_limit_windows(None, None, rate_limit, false) {
            push_window(window);
        }
    }

    if let Some(rate_limit) = value.get("code_review_rate_limit") {
        for window in parse_rate_limit_windows(Some("code-review"), None, rate_limit, false) {
            push_window(window);
        }
    }

    if let Some(items) = value
        .get("additional_rate_limits")
        .and_then(Value::as_array)
    {
        for item in items {
            let prefix = item.get("limit_name").and_then(Value::as_str);
            if let Some(rate_limit) = item.get("rate_limit") {
                for window in parse_rate_limit_windows(prefix, prefix, rate_limit, true) {
                    push_window(window);
                }
            }
        }
    }

    if let Some(items) = value.get("windows").and_then(Value::as_array) {
        for item in items {
            let name = item
                .get("name")
                .or_else(|| item.get("window"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if let Some(window) = parse_window(name, item) {
                push_window(window);
            }
        }
    }

    if let Some(items) = value.get("windows").and_then(Value::as_object) {
        for (name, item) in items {
            if let Some(window) = parse_window(name, item) {
                push_window(window);
            }
        }
    }

    for key in ["five_hour", "seven_day", "seven_day_opus"] {
        if let Some(window) = value.get(key).and_then(|v| parse_window(key, v)) {
            push_window(window);
        }
    }

    for group_key in ["usage", "rate_limits", "limits", "buckets"] {
        if let Some(group) = value.get(group_key).and_then(Value::as_object) {
            for (name, item) in group {
                if let Some(window) = parse_window(name, item) {
                    push_window(window);
                }
            }
        }
        if let Some(group) = value.get(group_key).and_then(Value::as_array) {
            for item in group {
                let name = item
                    .get("name")
                    .or_else(|| item.get("window"))
                    .or_else(|| item.get("type"))
                    .and_then(Value::as_str)
                    .unwrap_or(group_key);
                if let Some(window) = parse_window(name, item) {
                    push_window(window);
                }
            }
        }
    }

    UsageInfo {
        agent_name: DESCRIPTOR.id.to_string(),
        display_name: DESCRIPTOR.display_name.to_string(),
        plan,
        windows,
        extra_usage: None,
    }
}

impl AgentIdentity for CodexAgent {
    fn descriptor(&self) -> &'static AgentDescriptor {
        &DESCRIPTOR
    }
}

impl Agent for CodexAgent {
    fn auth_capability(&self) -> Option<&dyn AgentAuthCapability> {
        Some(self)
    }

    fn usage_capability(&self) -> Option<&dyn AgentUsageCapability> {
        Some(self)
    }
}

#[async_trait]
impl AgentAuthCapability for CodexAgent {
    async fn login(&self) -> Result<()> {
        login().await
    }

    async fn load_saved_token(&self) -> Result<Option<AgentToken>> {
        load_saved_token().await
    }

    async fn refresh_if_needed(&self, token: &mut AgentToken) -> Result<()> {
        refresh_if_needed(token).await
    }
}

#[async_trait]
impl AgentUsageCapability for CodexAgent {
    async fn get_usage(&self, token: &AgentToken) -> Result<UsageInfo> {
        get_usage(token).await
    }

    async fn get_usage_raw(&self, token: &AgentToken) -> Result<Value> {
        get_usage_raw(token).await
    }

    fn quota_name(&self, window: &UsageWindow) -> String {
        if window.is_extra {
            return window
                .source_limit_name
                .clone()
                .unwrap_or_else(|| "additional_rate_limits".to_string());
        }
        window.name.clone()
    }

    fn display_quota_name(&self, window: &UsageWindow) -> String {
        if window.is_extra {
            let limit_name = window
                .source_limit_name
                .clone()
                .unwrap_or_else(|| "additional_rate_limits".to_string());
            if window.name.ends_with("-five-hour") {
                return format!("{limit_name}(5h)");
            }
            if window.name.ends_with("-seven-day") {
                return format!("{limit_name}(7d)");
            }
            if window.name.ends_with("-seven-day-opus") {
                return format!("{limit_name}(opus 7d)");
            }
            return limit_name;
        }
        normalize_quota_display_name(&window.name)
    }
}

fn parse_window(name: &str, value: &Value) -> Option<UsageWindow> {
    parse_window_with_extra(name, value, false, None)
}

fn parse_window_with_extra(
    name: &str,
    value: &Value,
    is_extra: bool,
    source_limit_name: Option<&str>,
) -> Option<UsageWindow> {
    let utilization_pct = parse_utilization_pct(value);
    let resets_at = parse_string_fields(
        value,
        &[
            "resets_at",
            "reset_at",
            "next_reset_at",
            "resetsAt",
            "resetAt",
        ],
    );

    if utilization_pct.is_some() && resets_at.is_some() {
        return Some(UsageWindow {
            name: name.replace('_', "-"),
            utilization_pct: utilization_pct.unwrap_or(0.0),
            resets_at,
            is_extra,
            source_limit_name: source_limit_name.map(ToString::to_string),
        });
    }

    for nested_key in [
        "usage",
        "window",
        "bucket",
        "current",
        "current_window",
        "limit",
        "rate_limit",
        "primary_window",
        "secondary_window",
    ] {
        if let Some(nested) = value.get(nested_key) {
            if let Some(window) = parse_window_with_extra(name, nested, is_extra, source_limit_name)
            {
                return Some(window);
            }
        }
    }

    None
}

fn parse_utilization_pct(value: &Value) -> Option<f64> {
    if let Some(v) = parse_number_fields(
        value,
        &[
            "utilization_pct",
            "utilization",
            "usage_pct",
            "usage_percent",
            "percent_used",
            "used_percent",
            "percent",
        ],
    ) {
        return Some(if v <= 1.0 { v * 100.0 } else { v });
    }

    if let Some(v) = parse_number_fields(
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

    let used = parse_number_fields(
        value,
        &[
            "used",
            "consumed",
            "total_used",
            "current_usage",
            "usage",
            "spent",
        ],
    );
    let limit = parse_number_fields(value, &["limit", "max", "total", "allowance", "quota"]);
    if let (Some(used), Some(limit)) = (used, limit) {
        if limit > 0.0 {
            return Some((used / limit) * 100.0);
        }
    }

    let remaining = parse_number_fields(value, &["remaining", "left", "available"]);
    if let (Some(remaining), Some(limit)) = (remaining, limit) {
        if limit > 0.0 {
            return Some(((limit - remaining) / limit) * 100.0);
        }
    }

    None
}

fn parse_number_fields(value: &Value, keys: &[&str]) -> Option<f64> {
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

fn parse_string_fields(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(field) = value.get(*key) {
            if let Some(s) = field.as_str() {
                if !s.trim().is_empty() {
                    return Some(s.to_string());
                }
                continue;
            }
            if let Some(ts) = field.as_i64() {
                if let Some(dt) = chrono::DateTime::<Utc>::from_timestamp(ts, 0) {
                    return Some(dt.to_rfc3339());
                }
                return Some(ts.to_string());
            }
            if let Some(ts) = field.as_u64() {
                if let Ok(ts_i64) = i64::try_from(ts) {
                    if let Some(dt) = chrono::DateTime::<Utc>::from_timestamp(ts_i64, 0) {
                        return Some(dt.to_rfc3339());
                    }
                }
                return Some(ts.to_string());
            }
        }
    }
    None
}

fn parse_rate_limit_windows(
    name_prefix: Option<&str>,
    source_limit_name: Option<&str>,
    value: &Value,
    is_extra: bool,
) -> Vec<UsageWindow> {
    let mut out = Vec::new();
    for field in ["primary_window", "secondary_window"] {
        let Some(window_value) = value.get(field) else {
            continue;
        };
        if window_value.is_null() {
            continue;
        }

        let seconds = window_value.get("limit_window_seconds").and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
        });
        let base_name = map_window_seconds(seconds).unwrap_or(field);
        let name = match name_prefix {
            Some(prefix) => format!("{}-{}", normalize_name(prefix), base_name),
            None => base_name.to_string(),
        };

        if let Some(window) =
            parse_window_with_extra(&name, window_value, is_extra, source_limit_name)
        {
            out.push(window);
        }
    }
    out
}

fn map_window_seconds(seconds: Option<i64>) -> Option<&'static str> {
    match seconds {
        Some(18000) => Some("five-hour"),
        Some(604800) => Some("seven-day"),
        _ => None,
    }
}

fn normalize_name(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;
    for ch in input.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if !last_dash {
                out.push('-');
                last_dash = true;
            }
        } else {
            out.push(mapped);
            last_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_usage;

    #[test]
    fn parse_usage_supports_windows_object_with_used_limit() {
        let value = json!({
            "plan": "plus",
            "windows": {
                "five_hour": { "used": 30, "limit": 50, "resets_at": "2026-02-27T20:00:00Z" }
            }
        });

        let usage = parse_usage(value);
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].name, "five-hour");
        assert!((usage.windows[0].utilization_pct - 60.0).abs() < 0.0001);
        assert_eq!(
            usage.windows[0].resets_at.as_deref(),
            Some("2026-02-27T20:00:00Z")
        );
    }

    #[test]
    fn parse_usage_supports_ratio_fields() {
        let value = json!({
            "plan_type": "plus",
            "rate_limits": {
                "seven_day": { "usage_ratio": 0.12, "resets_at": "2026-03-06T15:00:00Z" }
            }
        });

        let usage = parse_usage(value);
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].name, "seven-day");
        assert!((usage.windows[0].utilization_pct - 12.0).abs() < 0.0001);
    }

    #[test]
    fn parse_usage_supports_nested_usage_object() {
        let value = json!({
            "subscription_plan": "plus",
            "buckets": {
                "five_hour": {
                    "usage": { "remaining": 20, "limit": 100, "resetAt": "2026-02-28T01:00:00Z" }
                }
            }
        });

        let usage = parse_usage(value);
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].name, "five-hour");
        assert!((usage.windows[0].utilization_pct - 80.0).abs() < 0.0001);
        assert_eq!(
            usage.windows[0].resets_at.as_deref(),
            Some("2026-02-28T01:00:00Z")
        );
    }

    #[test]
    fn parse_usage_supports_wham_rate_limit_shape() {
        let value = json!({
            "plan_type": "plus",
            "rate_limit": {
                "primary_window": {
                    "limit_window_seconds": 18000,
                    "reset_at": 1772226175,
                    "used_percent": 10
                },
                "secondary_window": {
                    "limit_window_seconds": 604800,
                    "reset_at": 1772760122,
                    "used_percent": 5
                }
            },
            "additional_rate_limits": [
                {
                    "limit_name": "GPT-5.3-Codex-Spark",
                    "rate_limit": {
                        "primary_window": {
                            "limit_window_seconds": 18000,
                            "reset_at": 1772228940,
                            "used_percent": 0
                        }
                    }
                }
            ]
        });

        let usage = parse_usage(value);
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        assert!(usage.windows.iter().any(|w| {
            w.name == "five-hour"
                && (w.utilization_pct - 10.0).abs() < 0.0001
                && w.resets_at.as_deref().is_some()
        }));
        assert!(usage.windows.iter().any(|w| {
            w.name == "seven-day"
                && (w.utilization_pct - 5.0).abs() < 0.0001
                && w.resets_at.as_deref().is_some()
        }));
        assert!(
            !usage
                .windows
                .iter()
                .any(|w| w.name == "additional-rate-limits")
        );
        assert!(
            usage
                .windows
                .iter()
                .any(|w| w.name == "gpt-5-3-codex-spark-five-hour" && w.is_extra)
        );
    }

    #[test]
    fn parse_usage_ignores_unrelated_root_arrays() {
        let value = json!({
            "plan_type": "plus",
            "metadata": [
                {
                    "name": "not-a-quota",
                    "used": 1,
                    "limit": 2,
                    "resets_at": "2026-02-27T20:00:00Z"
                }
            ]
        });

        let usage = parse_usage(value);
        assert!(usage.windows.is_empty());
    }
}
