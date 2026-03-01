use std::io;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use reqwest::StatusCode;
use reqwest::header::COOKIE;
use serde_json::Value;

use crate::agent::auth::token::{AgentToken, auth_file_path, load_token, save_token};
use crate::agent::{
    Agent, AgentAuthCapability, AgentDescriptor, AgentIdentity, AgentUsageCapability, UsageInfo,
    UsageWindow,
};
use crate::error::{AppError, Result};

const AUTH_ME_URL: &str = "https://cursor.com/api/auth/me";
pub const USAGE_SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
const TOKEN_FILE_NAME: &str = "cursor_auth.json";
pub const DESCRIPTOR: AgentDescriptor = AgentDescriptor {
    id: "cursor",
    display_name: "Cursor",
    token_file_name: TOKEN_FILE_NAME,
};

pub struct CursorAgent;

pub async fn login(client: &reqwest::Client) -> Result<()> {
    println!("Cursor login steps:");
    println!("1. Open https://cursor.com in your browser and sign in.");
    println!("2. Open DevTools -> Application -> Cookies.");
    println!("3. Copy the value of WorkosCursorSessionToken.");
    println!("Paste WorkosCursorSessionToken and press Enter:");

    let pasted = tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map(|_| input.trim().to_string())
    })
    .await
    .map_err(|e| AppError::OAuth(format!("Failed to read pasted cookie: {e}")))?
    .map_err(AppError::Io)?;

    let cookie = normalize_cookie_value(&pasted);
    if cookie.is_empty() {
        return Err(AppError::OAuth(
            "WorkosCursorSessionToken cannot be empty.".to_string(),
        ));
    }

    validate_cookie(&cookie, client).await?;

    let now = Utc::now();
    let token = AgentToken {
        access_token: cookie,
        refresh_token: None,
        expires_at: now + Duration::days(365),
        last_refresh: Some(now),
    };

    let path = auth_file_path(TOKEN_FILE_NAME)?;
    save_token(&path, &token)?;
    Ok(())
}

pub async fn refresh_if_needed(_token: &mut AgentToken, _client: &reqwest::Client) -> Result<()> {
    Ok(())
}

pub async fn get_usage(token: &AgentToken, client: &reqwest::Client) -> Result<UsageInfo> {
    let value = get_usage_raw(token, client).await?;
    Ok(parse_usage(value))
}

pub async fn get_usage_raw(token: &AgentToken, client: &reqwest::Client) -> Result<Value> {
    let response = client
        .get(USAGE_SUMMARY_URL)
        .header(COOKIE, cursor_cookie_header(&token.access_token))
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err(AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        });
    }

    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Cursor usage request failed with status {}",
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

async fn validate_cookie(cookie: &str, client: &reqwest::Client) -> Result<()> {
    let response = client
        .get(AUTH_ME_URL)
        .header(COOKIE, cursor_cookie_header(cookie))
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err(AppError::OAuth(
            "Cursor cookie is invalid or expired. Please copy a fresh WorkosCursorSessionToken."
                .to_string(),
        ));
    }

    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Cursor cookie validation failed with status {}",
            response.status()
        )));
    }

    Ok(())
}

fn parse_usage(value: Value) -> UsageInfo {
    let plan = value
        .get("membershipType")
        .or_else(|| value.get("plan"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let resets_at = parse_billing_reset(&value);
    let mut windows = Vec::new();
    let mut path = Vec::new();
    collect_windows(&value, &mut path, resets_at.as_ref(), &mut windows);

    UsageInfo {
        agent_name: DESCRIPTOR.id.to_string(),
        display_name: DESCRIPTOR.display_name.to_string(),
        plan,
        windows,
        extra_usage: None,
    }
}

fn collect_windows(
    value: &Value,
    path: &mut Vec<String>,
    resets_at: Option<&String>,
    windows: &mut Vec<UsageWindow>,
) {
    match value {
        Value::Object(map) => {
            if let Some(window) = parse_window(path, map, resets_at) {
                windows.push(window);
            }

            for (key, nested) in map {
                path.push(key.to_string());
                collect_windows(nested, path, resets_at, windows);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (idx, nested) in items.iter().enumerate() {
                path.push(format!("item-{}", idx + 1));
                collect_windows(nested, path, resets_at, windows);
                path.pop();
            }
        }
        _ => {}
    }
}

fn parse_window(
    path: &[String],
    entry: &serde_json::Map<String, Value>,
    resets_at: Option<&String>,
) -> Option<UsageWindow> {
    if path.is_empty() {
        return None;
    }

    if entry.get("enabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }

    let utilization_pct = parse_num_requests_window(entry)
        .or_else(|| parse_used_limit_window(entry))
        .or_else(|| parse_percent_window(entry))?;
    Some(UsageWindow {
        name: normalize_name(&path.join("-")),
        utilization_pct,
        resets_at: resets_at.cloned(),
        is_extra: false,
        source_limit_name: None,
    })
}

fn parse_num_requests_window(entry: &serde_json::Map<String, Value>) -> Option<f64> {
    let num_requests = entry.get("numRequests").and_then(parse_f64)?;
    let max_request_usage = entry.get("maxRequestUsage").and_then(parse_f64)?;
    if max_request_usage <= 0.0 {
        return None;
    }
    Some((num_requests / max_request_usage) * 100.0)
}

fn parse_used_limit_window(entry: &serde_json::Map<String, Value>) -> Option<f64> {
    let limit = entry.get("limit").and_then(parse_f64)?;
    if limit <= 0.0 {
        return None;
    }

    if let Some(used) = entry.get("used").and_then(parse_f64) {
        return Some((used / limit) * 100.0);
    }

    if let Some(remaining) = entry.get("remaining").and_then(parse_f64) {
        return Some(((limit - remaining) / limit) * 100.0);
    }

    None
}

fn parse_percent_window(entry: &serde_json::Map<String, Value>) -> Option<f64> {
    for key in [
        "totalPercentUsed",
        "percentUsed",
        "apiPercentUsed",
        "autoPercentUsed",
    ] {
        let Some(value) = entry.get(key).and_then(parse_f64) else {
            continue;
        };
        return Some(if value <= 1.0 { value * 100.0 } else { value });
    }
    None
}

fn parse_billing_reset(value: &Value) -> Option<String> {
    for key in ["billingCycleEnd", "startOfMonth", "billing_cycle_end"] {
        let Some(raw) = value.get(key) else {
            continue;
        };
        if let Some(parsed) = parse_timestamp_or_string(raw) {
            return Some(parsed);
        }
    }
    None
}

fn parse_f64(value: &Value) -> Option<f64> {
    if let Some(n) = value.as_f64() {
        return Some(n);
    }
    value.as_str().and_then(|v| v.parse::<f64>().ok())
}

fn timestamp_to_string(raw: i64) -> Option<String> {
    let seconds = if raw.saturating_abs() > 10_000_000_000 {
        raw.checked_div(1000)?
    } else {
        raw
    };
    chrono::DateTime::<Utc>::from_timestamp(seconds, 0).map(|dt| dt.to_rfc3339())
}

fn parse_timestamp_or_string(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let timestamp = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))?;
    timestamp_to_string(timestamp)
}

fn normalize_cookie_value(value: &str) -> String {
    let trimmed = value.trim();
    if let Some((key, token)) = trimmed.split_once('=')
        && key.trim() == "WorkosCursorSessionToken"
    {
        return token.trim().to_string();
    }
    trimmed.to_string()
}

fn cursor_cookie_header(value: &str) -> String {
    format!("WorkosCursorSessionToken={}", normalize_cookie_value(value))
}

fn normalize_name(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut last_dash = false;
    let mut prev_lower_or_digit = false;
    for ch in input.chars() {
        if !ch.is_ascii_alphanumeric() {
            if !last_dash {
                output.push('-');
                last_dash = true;
            }
            prev_lower_or_digit = false;
            continue;
        }

        if ch.is_ascii_uppercase() && prev_lower_or_digit && !last_dash {
            output.push('-');
        }

        output.push(ch.to_ascii_lowercase());
        last_dash = false;
        prev_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
    }
    output.trim_matches('-').to_string()
}

fn humanize_quota_name(quota_name: &str) -> String {
    match quota_name {
        "individual-usage-on-demand" => return "Individual On-Demand".to_string(),
        "individual-usage-plan" => return "Individual Plan".to_string(),
        "team-usage-on-demand" => return "Team On-Demand".to_string(),
        "team-usage-plan" => return "Team Plan".to_string(),
        _ => {}
    }

    let mut words = Vec::new();
    let parts: Vec<&str> = quota_name
        .split('-')
        .filter(|part| !part.is_empty())
        .collect();
    let mut idx = 0usize;
    while idx < parts.len() {
        if parts[idx] == "on" && idx + 1 < parts.len() && parts[idx + 1] == "demand" {
            words.push("On-Demand".to_string());
            idx += 2;
            continue;
        }

        let part = parts[idx];
        if part == "api" {
            words.push("API".to_string());
        } else {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                words.push(format!(
                    "{}{}",
                    first.to_ascii_uppercase(),
                    chars.as_str().to_ascii_lowercase()
                ));
            }
        }
        idx += 1;
    }

    if words.is_empty() {
        return quota_name.to_string();
    }
    words.join(" ")
}

impl AgentIdentity for CursorAgent {
    fn descriptor(&self) -> &'static AgentDescriptor {
        &DESCRIPTOR
    }
}

impl Agent for CursorAgent {
    fn auth_capability(&self) -> Option<&dyn AgentAuthCapability> {
        Some(self)
    }

    fn usage_capability(&self) -> Option<&dyn AgentUsageCapability> {
        Some(self)
    }
}

#[async_trait]
impl AgentAuthCapability for CursorAgent {
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
impl AgentUsageCapability for CursorAgent {
    async fn get_usage(&self, token: &AgentToken, client: &reqwest::Client) -> Result<UsageInfo> {
        get_usage(token, client).await
    }

    async fn get_usage_raw(&self, token: &AgentToken, client: &reqwest::Client) -> Result<Value> {
        get_usage_raw(token, client).await
    }

    fn process_quota_name(&self, quota_name: &str) -> String {
        humanize_quota_name(quota_name)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{humanize_quota_name, parse_usage};

    #[test]
    fn parse_usage_supports_num_requests_pattern() {
        let value = json!({
            "membershipType": "pro",
            "startOfMonth": "2026-03-01T00:00:00Z",
            "gpt4": { "numRequests": 25, "maxRequestUsage": 100 }
        });

        let usage = parse_usage(value);
        assert_eq!(usage.plan.as_deref(), Some("pro"));
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].name, "gpt4");
        assert!((usage.windows[0].utilization_pct - 25.0).abs() < 0.0001);
        assert_eq!(
            usage.windows[0].resets_at.as_deref(),
            Some("2026-03-01T00:00:00Z")
        );
    }

    #[test]
    fn parse_usage_handles_empty_response() {
        let usage = parse_usage(json!({}));
        assert!(usage.plan.is_none());
        assert!(usage.windows.is_empty());
    }

    #[test]
    fn parse_usage_skips_zero_limit_entries() {
        let value = json!({
            "startOfMonth": "2026-03-01T00:00:00Z",
            "gpt4": { "numRequests": 7, "maxRequestUsage": 0 }
        });

        let usage = parse_usage(value);
        assert!(usage.windows.is_empty());
    }

    #[test]
    fn parse_usage_skips_non_object_fields() {
        let value = json!({
            "plan": "hobby",
            "startOfMonth": "2026-03-01T00:00:00Z",
            "totalRequests": 123,
            "requestSummary": "ignored",
            "windows": ["not-an-object"]
        });

        let usage = parse_usage(value);
        assert_eq!(usage.plan.as_deref(), Some("hobby"));
        assert!(usage.windows.is_empty());
    }

    #[test]
    fn parse_usage_supports_cursor_usage_summary_shape() {
        let value = json!({
            "membershipType": "enterprise",
            "billingCycleEnd": "2026-03-24T08:19:20.000Z",
            "individualUsage": {
                "onDemand": {
                    "enabled": false,
                    "limit": 0,
                    "remaining": 0,
                    "used": 0
                },
                "plan": {
                    "enabled": true,
                    "limit": 2000,
                    "remaining": 2000,
                    "used": 0
                }
            },
            "teamUsage": {
                "onDemand": {
                    "enabled": true,
                    "limit": 78000,
                    "remaining": 70439,
                    "used": 7561
                }
            }
        });

        let usage = parse_usage(value);
        assert_eq!(usage.plan.as_deref(), Some("enterprise"));
        assert_eq!(usage.windows.len(), 2);

        let plan_window = usage
            .windows
            .iter()
            .find(|w| w.name == "individual-usage-plan")
            .expect("individual usage plan window should exist");
        assert!(plan_window.utilization_pct.abs() < 0.0001);
        assert_eq!(
            plan_window.resets_at.as_deref(),
            Some("2026-03-24T08:19:20.000Z")
        );

        let team_window = usage
            .windows
            .iter()
            .find(|w| w.name == "team-usage-on-demand")
            .expect("team usage on-demand window should exist");
        assert!((team_window.utilization_pct - 9.6935897436).abs() < 0.0001);
        assert_eq!(
            team_window.resets_at.as_deref(),
            Some("2026-03-24T08:19:20.000Z")
        );
    }

    #[test]
    fn parse_usage_includes_individual_plan_and_ondemand() {
        let value = json!({
            "membershipType": "pro",
            "billingCycleEnd": "2026-03-24T08:19:20.000Z",
            "individualUsage": {
                "onDemand": {
                    "enabled": true,
                    "limit": 100,
                    "used": 10
                },
                "plan": {
                    "enabled": true,
                    "totalPercentUsed": 0.25
                }
            }
        });

        let usage = parse_usage(value);
        assert_eq!(usage.plan.as_deref(), Some("pro"));
        assert_eq!(usage.windows.len(), 2);
        assert!(
            usage
                .windows
                .iter()
                .any(|w| w.name == "individual-usage-on-demand"
                    && (w.utilization_pct - 10.0).abs() < 0.0001)
        );
        assert!(usage.windows.iter().any(
            |w| w.name == "individual-usage-plan" && (w.utilization_pct - 25.0).abs() < 0.0001
        ));
    }

    #[test]
    fn humanize_quota_name_formats_cursor_windows() {
        assert_eq!(
            humanize_quota_name("individual-usage-on-demand"),
            "Individual On-Demand"
        );
        assert_eq!(
            humanize_quota_name("individual-usage-plan"),
            "Individual Plan"
        );
        assert_eq!(
            humanize_quota_name("team-usage-on-demand"),
            "Team On-Demand"
        );
        assert_eq!(humanize_quota_name("team-usage-plan"), "Team Plan");
        assert_eq!(humanize_quota_name("api-usage"), "API Usage");
    }
}
