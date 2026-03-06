use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::Value;

use crate::agent::{UsageInfo, UsageWindow};
use crate::error::{AppError, Result};

pub fn rolling_window_start(days: i64) -> DateTime<Utc> {
    Utc::now() - Duration::days(days.max(1))
}

pub fn parse_rfc3339_timestamp(value: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn parse_naive_date(value: &str) -> Option<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()?;
    let naive = date.and_hms_opt(0, 0, 0)?;
    Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

pub fn summarize_local_plan(label: &str, total_tokens: u64, cost_usd: Option<f64>) -> String {
    let tokens = format_token_count(total_tokens);
    match cost_usd {
        Some(cost) if cost > 0.0 => format!("{label}: {tokens} tokens, ${cost:.2}"),
        _ => format!("{label}: {tokens} tokens"),
    }
}

pub fn format_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

pub fn local_usage_info(
    agent_name: &str,
    display_name: &str,
    plan: String,
    mut windows: Vec<UsageWindow>,
    extra_usage: Option<Value>,
) -> UsageInfo {
    if windows.is_empty() {
        windows.push(UsageWindow {
            name: "local-30-day".to_string(),
            utilization_pct: 100.0,
            resets_at: None,
            is_extra: false,
            source_limit_name: None,
        });
    }

    UsageInfo {
        agent_name: agent_name.to_string(),
        display_name: display_name.to_string(),
        plan: Some(plan),
        windows,
        extra_usage,
        source: Some("local".to_string()),
    }
}

pub fn normalize_path(path: Option<&str>, default_path: PathBuf) -> PathBuf {
    path.filter(|value| !value.trim().is_empty())
        .map(|value| shellexpand(value.trim()))
        .unwrap_or(default_path)
}

pub fn expand_home(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    shellexpand(&raw)
}

fn shellexpand(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }

    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }

    PathBuf::from(path)
}

pub fn local_scan_error(message: impl Into<String>) -> AppError {
    AppError::LocalScan(message.into())
}

pub fn ensure_exists(path: &Path, what: &str) -> Result<()> {
    if path.exists() {
        Ok(())
    } else {
        Err(local_scan_error(format!(
            "{what} not found at {}",
            path.display()
        )))
    }
}
