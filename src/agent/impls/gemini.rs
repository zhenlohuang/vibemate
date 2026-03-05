use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::{Value, json};

use crate::agent::auth::token::{AgentToken, auth_file_path, load_token, save_token};
use crate::agent::{
    Agent, AgentAuthCapability, AgentDescriptor, AgentIdentity, AgentUsageCapability, UsageInfo,
    UsageWindow,
};
use crate::error::{AppError, Result};

const TOKEN_FILE_NAME: &str = "gemini_auth.json";
const GEMINI_CREDS_FILE: &str = ".gemini/oauth_creds.json";
const GOOGLE_OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const LOAD_CODE_ASSIST_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
const RETRIEVE_USER_QUOTA_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota";
const CLOUD_PROJECTS_URL: &str = "https://cloudresourcemanager.googleapis.com/v1/projects";
const REFRESH_EARLY_WINDOW_MINUTES: i64 = 5;

pub const DESCRIPTOR: AgentDescriptor = AgentDescriptor {
    id: "gemini",
    display_name: "Gemini",
    token_file_name: TOKEN_FILE_NAME,
};

pub struct GeminiAgent;

#[derive(Debug, Serialize)]
struct RefreshTokenExchange<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
    refresh_token: &'a str,
}

#[derive(Debug, serde::Deserialize)]
struct RefreshTokenResponse {
    access_token: String,
    expires_in: Option<i64>,
    refresh_token: Option<String>,
}

pub async fn login(_client: &reqwest::Client) -> Result<()> {
    let creds_path = gemini_oauth_creds_path()?;
    if !creds_path.exists() {
        return Err(AppError::OAuth(format!(
            "Gemini CLI credentials not found at {}. Run `gemini auth login` and retry.",
            creds_path.display()
        )));
    }

    let raw = fs::read_to_string(&creds_path)?;
    let value: Value = serde_json::from_str(&raw)?;
    let token = import_gemini_creds(&value)?;
    let path = auth_file_path(TOKEN_FILE_NAME)?;
    save_token(&path, &token)?;
    Ok(())
}

pub async fn refresh_if_needed(token: &mut AgentToken, client: &reqwest::Client) -> Result<()> {
    let now = Utc::now();
    if token.expires_at - now > Duration::minutes(REFRESH_EARLY_WINDOW_MINUTES) {
        return Ok(());
    }

    let refresh_token = token
        .refresh_token
        .as_deref()
        .ok_or_else(|| AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        })?;

    let (client_id, client_secret) = load_oauth_client_credentials()?.ok_or_else(|| {
        AppError::OAuth(
            "Unable to extract Gemini OAuth client credentials from Gemini CLI install. Run `gemini auth login` and retry."
                .to_string(),
        )
    })?;

    let response = client
        .post(GOOGLE_OAUTH_TOKEN_URL)
        .form(&RefreshTokenExchange {
            grant_type: "refresh_token",
            client_id: &client_id,
            client_secret: &client_secret,
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
            "Gemini token refresh failed with status {}",
            response.status()
        )));
    }

    let payload: RefreshTokenResponse = response.json().await?;
    token.access_token = payload.access_token;
    token.refresh_token = payload.refresh_token.or(token.refresh_token.take());
    token.expires_at = now + Duration::seconds(payload.expires_in.unwrap_or(3600));
    token.last_refresh = Some(now);

    let path = auth_file_path(TOKEN_FILE_NAME)?;
    save_token(&path, token)?;
    Ok(())
}

pub async fn get_usage(token: &AgentToken, client: &reqwest::Client) -> Result<UsageInfo> {
    let load_code_assist = load_code_assist(token, client).await?;
    let project = resolve_project_id(token, client, &load_code_assist).await?;
    let quota = retrieve_user_quota(token, client, project.as_deref()).await?;
    Ok(parse_usage(&load_code_assist, &quota))
}

pub async fn get_usage_raw(token: &AgentToken, client: &reqwest::Client) -> Result<Value> {
    let load_code_assist = load_code_assist(token, client).await?;
    let project = resolve_project_id(token, client, &load_code_assist).await?;
    let quota = retrieve_user_quota(token, client, project.as_deref()).await?;
    Ok(json!({
        "loadCodeAssist": load_code_assist,
        "resolvedProject": project,
        "retrieveUserQuota": quota,
    }))
}

pub async fn load_saved_token() -> Result<Option<AgentToken>> {
    let path = auth_file_path(TOKEN_FILE_NAME)?;
    load_token(&path)
}

fn gemini_oauth_creds_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| AppError::Config("Unable to find home directory".to_string()))?;
    Ok(home.join(GEMINI_CREDS_FILE))
}

fn import_gemini_creds(value: &Value) -> Result<AgentToken> {
    let access_token =
        find_first_string(value, &["access_token", "accessToken"]).ok_or_else(|| {
            AppError::OAuth("Gemini oauth_creds.json is missing access_token".to_string())
        })?;
    let refresh_token =
        find_first_string(value, &["refresh_token", "refreshToken"]).ok_or_else(|| {
            AppError::OAuth("Gemini oauth_creds.json is missing refresh_token".to_string())
        })?;

    let expires_at_value = find_first_value(
        value,
        &[
            "expiry_date",
            "expiryDate",
            "expires_at",
            "expiresAt",
            "expiry",
        ],
    )
    .ok_or_else(|| AppError::OAuth("Gemini oauth_creds.json is missing expiry_date".to_string()))?;
    let expires_at = parse_expiry_value(expires_at_value).ok_or_else(|| {
        AppError::OAuth("Unable to parse Gemini credential expiry_date".to_string())
    })?;

    Ok(AgentToken {
        access_token,
        refresh_token: Some(refresh_token),
        expires_at,
        last_refresh: Some(Utc::now()),
    })
}

fn parse_expiry_value(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
                return Some(dt.with_timezone(&Utc));
            }
            trimmed.parse::<i64>().ok().and_then(timestamp_to_datetime)
        }
        Value::Number(raw) => {
            if let Some(v) = raw.as_i64() {
                return timestamp_to_datetime(v);
            }
            raw.as_u64()
                .and_then(|v| i64::try_from(v).ok())
                .and_then(timestamp_to_datetime)
        }
        _ => None,
    }
}

fn timestamp_to_datetime(raw: i64) -> Option<DateTime<Utc>> {
    if raw.saturating_abs() > 10_000_000_000 {
        return DateTime::<Utc>::from_timestamp_millis(raw);
    }
    DateTime::<Utc>::from_timestamp(raw, 0)
}

fn find_first_string(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(found) = find_string_by_key(value, key) {
            return Some(found);
        }
    }
    None
}

fn find_string_by_key(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(value_to_string) {
                return Some(found);
            }
            for nested in map.values() {
                if let Some(found) = find_string_by_key(nested, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => {
            for nested in items {
                if let Some(found) = find_string_by_key(nested, key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn find_first_value<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    for key in keys {
        if let Some(found) = find_value_by_key(value, key) {
            return Some(found);
        }
    }
    None
}

fn find_value_by_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            for nested in map.values() {
                if let Some(found) = find_value_by_key(nested, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => {
            for nested in items {
                if let Some(found) = find_value_by_key(nested, key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Number(raw) => Some(raw.to_string()),
        _ => None,
    }
}

fn load_oauth_client_credentials() -> Result<Option<(String, String)>> {
    let Some(gemini_binary) = find_binary_in_path("gemini") else {
        return Ok(None);
    };
    let Some(oauth2_js_path) = find_oauth2_js_path(&gemini_binary) else {
        return Ok(None);
    };

    let contents = fs::read_to_string(oauth2_js_path)?;
    let client_id = extract_js_constant(&contents, "OAUTH_CLIENT_ID");
    let client_secret = extract_js_constant(&contents, "OAUTH_CLIENT_SECRET");
    Ok(match (client_id, client_secret) {
        (Some(id), Some(secret)) => Some((id, secret)),
        _ => None,
    })
}

fn find_binary_in_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        for suffix in [".exe", ".cmd", ".bat"] {
            let candidate = dir.join(format!("{name}{suffix}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn find_oauth2_js_path(gemini_binary: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(parent) = gemini_binary.parent() {
        candidates.push(parent.join(
            "../lib/node_modules/@google/gemini-cli/node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js",
        ));
        candidates
            .push(parent.join(
                "../lib/node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js",
            ));
        candidates.push(
            parent.join("../lib/node_modules/@google/gemini-cli/dist/src/code_assist/oauth2.js"),
        );
        candidates.push(parent.join("../lib/node_modules/@google/gemini-cli/dist/oauth2.js"));
        candidates.push(parent.join("../lib/node_modules/gemini-cli/dist/oauth2.js"));
        candidates.push(parent.join("../lib/node_modules/@google/gemini-cli/build/oauth2.js"));
    }

    let canonical_binary = fs::canonicalize(gemini_binary).ok();
    if let Some(canonical_binary) = &canonical_binary
        && let Some(parent) = canonical_binary.parent()
    {
        candidates.push(parent.join(
            "../lib/node_modules/@google/gemini-cli/node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js",
        ));
        candidates
            .push(parent.join(
                "../lib/node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js",
            ));
        candidates.push(
            parent.join("../lib/node_modules/@google/gemini-cli/dist/src/code_assist/oauth2.js"),
        );
        candidates.push(parent.join("../lib/node_modules/@google/gemini-cli/dist/oauth2.js"));
        candidates.push(parent.join("../lib/node_modules/gemini-cli/dist/oauth2.js"));
        candidates.push(parent.join("../lib/node_modules/@google/gemini-cli/build/oauth2.js"));
    }

    if let Ok(contents) = fs::read_to_string(gemini_binary)
        && let Some(parent) = gemini_binary.parent()
        && let Some(module_path) = extract_module_path_from_launcher(parent, &contents)
        && let Some(module_dir) = module_path.parent()
    {
        candidates.push(module_dir.join("oauth2.js"));
        candidates.push(module_dir.join("auth/oauth2.js"));
        for ancestor in module_dir.ancestors().take(5) {
            candidates.push(ancestor.join("oauth2.js"));
            candidates.push(ancestor.join("dist/oauth2.js"));
            candidates.push(ancestor.join("build/oauth2.js"));
        }
    }

    if let Some(canonical_binary) = canonical_binary
        && let Ok(contents) = fs::read_to_string(&canonical_binary)
        && let Some(parent) = canonical_binary.parent()
        && let Some(module_path) = extract_module_path_from_launcher(parent, &contents)
        && let Some(module_dir) = module_path.parent()
    {
        candidates.push(module_dir.join("oauth2.js"));
        candidates.push(module_dir.join("auth/oauth2.js"));
    }

    let mut seen = HashSet::new();
    for candidate in candidates {
        let normalized = normalize_candidate_path(candidate);
        if !seen.insert(normalized.clone()) {
            continue;
        }
        if normalized.is_file() {
            return Some(normalized);
        }
    }
    None
}

fn normalize_candidate_path(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

fn extract_module_path_from_launcher(base_dir: &Path, contents: &str) -> Option<PathBuf> {
    for quote in ['\'', '"'] {
        for segment in contents.split(quote) {
            if !(segment.contains("node_modules/")
                && segment.contains("gemini")
                && segment.ends_with(".js"))
            {
                continue;
            }
            let candidate = Path::new(segment);
            let resolved = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                base_dir.join(candidate)
            };
            return Some(resolved);
        }
    }
    None
}

fn extract_js_constant(contents: &str, key: &str) -> Option<String> {
    for (index, _) in contents.match_indices(key) {
        let rest = &contents[index + key.len()..];
        let assignment = rest
            .find(['=', ':'])
            .map(|offset| &rest[offset + 1..])
            .unwrap_or(rest);
        if let Some(value) = extract_first_quoted_string(assignment) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn extract_first_quoted_string(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        let current = bytes[idx];
        if current != b'\'' && current != b'"' {
            idx += 1;
            continue;
        }
        let quote = current;
        let mut escaped = false;
        let mut out = String::new();
        idx += 1;
        while idx < bytes.len() {
            let b = bytes[idx];
            if escaped {
                out.push(b as char);
                escaped = false;
                idx += 1;
                continue;
            }
            if b == b'\\' {
                escaped = true;
                idx += 1;
                continue;
            }
            if b == quote {
                return Some(out);
            }
            out.push(b as char);
            idx += 1;
        }
    }
    None
}

async fn load_code_assist(token: &AgentToken, client: &reqwest::Client) -> Result<Value> {
    let response = client
        .post(LOAD_CODE_ASSIST_URL)
        .bearer_auth(&token.access_token)
        .json(&json!({
            "metadata": {
                "ideType": "GEMINI_CLI",
                "pluginType": "GEMINI",
            }
        }))
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        });
    }

    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Gemini loadCodeAssist failed with status {}",
            response.status()
        )));
    }

    response.json().await.map_err(Into::into)
}

async fn resolve_project_id(
    token: &AgentToken,
    client: &reqwest::Client,
    load_code_assist: &Value,
) -> Result<Option<String>> {
    if let Some(project) = extract_project_id(load_code_assist) {
        return Ok(Some(project));
    }
    discover_fallback_project(token, client).await
}

fn extract_project_id(value: &Value) -> Option<String> {
    if let Some(project) = find_first_string(
        value,
        &[
            "cloudaicompanionProject",
            "cloudAiCompanionProject",
            "projectId",
            "project_id",
        ],
    ) {
        return Some(project);
    }
    None
}

async fn discover_fallback_project(
    token: &AgentToken,
    client: &reqwest::Client,
) -> Result<Option<String>> {
    let response = client
        .get(CLOUD_PROJECTS_URL)
        .bearer_auth(&token.access_token)
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        });
    }

    if !response.status().is_success() {
        return Ok(None);
    }

    let value: Value = response.json().await?;
    let mut ids = Vec::new();
    collect_project_ids(&value, &mut ids);
    if let Some(id) = ids.iter().find(|id| id.starts_with("gen-lang-client")) {
        return Ok(Some(id.to_string()));
    }
    Ok(ids.into_iter().next())
}

fn collect_project_ids(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for key in ["projectId", "project_id"] {
                if let Some(project_id) = map.get(key).and_then(value_to_string) {
                    out.push(project_id);
                }
            }
            for nested in map.values() {
                collect_project_ids(nested, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_project_ids(item, out);
            }
        }
        _ => {}
    }
}

async fn retrieve_user_quota(
    token: &AgentToken,
    client: &reqwest::Client,
    project: Option<&str>,
) -> Result<Value> {
    let request_body = if let Some(project) = project {
        json!({ "project": project })
    } else {
        json!({})
    };

    let response = client
        .post(RETRIEVE_USER_QUOTA_URL)
        .bearer_auth(&token.access_token)
        .json(&request_body)
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        });
    }

    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Gemini retrieveUserQuota failed with status {}",
            response.status()
        )));
    }

    response.json().await.map_err(Into::into)
}

fn parse_usage(load_code_assist: &Value, retrieve_user_quota: &Value) -> UsageInfo {
    UsageInfo {
        agent_name: DESCRIPTOR.id.to_string(),
        display_name: DESCRIPTOR.display_name.to_string(),
        plan: find_first_string(
            load_code_assist,
            &[
                "tier",
                "subscriptionTier",
                "codeAssistTier",
                "plan",
                "planType",
            ],
        )
        .map(|tier| normalize_tier_name(&tier)),
        windows: parse_quota_windows(retrieve_user_quota),
        extra_usage: None,
    }
}

fn parse_quota_windows(value: &Value) -> Vec<UsageWindow> {
    let mut windows = Vec::new();
    let mut path = Vec::new();
    let mut seen = HashSet::new();
    collect_quota_windows(value, &mut path, &mut seen, &mut windows);
    windows
}

fn collect_quota_windows(
    value: &Value,
    path: &mut Vec<String>,
    seen: &mut HashSet<String>,
    windows: &mut Vec<UsageWindow>,
) {
    match value {
        Value::Object(map) => {
            if let Some(window) = parse_quota_bucket(map, path)
                && seen.insert(window.name.clone())
            {
                windows.push(window);
            }

            for (key, nested) in map {
                path.push(normalize_name(key));
                collect_quota_windows(nested, path, seen, windows);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (idx, nested) in items.iter().enumerate() {
                path.push(format!("bucket-{}", idx + 1));
                collect_quota_windows(nested, path, seen, windows);
                path.pop();
            }
        }
        _ => {}
    }
}

fn parse_quota_bucket(
    map: &serde_json::Map<String, Value>,
    path: &[String],
) -> Option<UsageWindow> {
    let remaining_fraction = parse_map_f64(
        map,
        &[
            "remainingFraction",
            "remaining_fraction",
            "remainingFractionValue",
        ],
    )?;

    let remaining_fraction = normalize_fraction(remaining_fraction)?;
    let utilization_pct = ((1.0 - remaining_fraction) * 100.0).clamp(0.0, 100.0);
    let name = parse_map_string(
        map,
        &[
            "modelId",
            "model_id",
            "quotaName",
            "quota_name",
            "bucketId",
            "bucket_id",
            "name",
        ],
    )
    .map(|value| normalize_name(&value))
    .filter(|value| !value.is_empty())
    .or_else(|| {
        path.last().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() || trimmed.starts_with("bucket-") {
                return None;
            }
            Some(trimmed.to_string())
        })
    });
    let name = name?;
    let resets_at = parse_map_timestamp_string(
        map,
        &[
            "resetTime",
            "reset_time",
            "resetsAt",
            "resetAt",
            "nextResetTime",
        ],
    );

    Some(UsageWindow {
        name,
        utilization_pct,
        resets_at,
        is_extra: false,
        source_limit_name: None,
    })
}

fn parse_map_f64(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        if let Some(parsed) = value.as_f64() {
            return Some(parsed);
        }
        if let Some(parsed) = value
            .as_i64()
            .map(|raw| raw as f64)
            .or_else(|| value.as_u64().map(|raw| raw as f64))
        {
            return Some(parsed);
        }
        if let Some(parsed) = value.as_str().and_then(|raw| raw.parse::<f64>().ok()) {
            return Some(parsed);
        }
    }
    None
}

fn parse_map_string(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = map.get(*key).and_then(value_to_string) {
            return Some(value);
        }
    }
    None
}

fn parse_map_timestamp_string(
    map: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        if let Some(value) = value_to_string(value) {
            if let Ok(parsed) = DateTime::parse_from_rfc3339(&value) {
                return Some(parsed.with_timezone(&Utc).to_rfc3339());
            }
            if let Ok(ts) = value.parse::<i64>()
                && let Some(parsed) = timestamp_to_datetime(ts)
            {
                return Some(parsed.to_rfc3339());
            }
            return Some(value);
        }
    }
    None
}

fn normalize_fraction(value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    let mut normalized = value;
    if normalized > 1.0 && normalized <= 100.0 {
        normalized /= 100.0;
    }
    Some(normalized.clamp(0.0, 1.0))
}

fn normalize_name(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut last_dash = false;
    for ch in input.chars() {
        let mapped = if ch.is_ascii_alphanumeric() || ch == '.' {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if !last_dash {
                output.push('-');
                last_dash = true;
            }
        } else {
            output.push(mapped);
            last_dash = false;
        }
    }
    output.trim_matches('-').to_string()
}

fn normalize_tier_name(tier: &str) -> String {
    let lower = tier.to_ascii_lowercase();
    if lower.contains("free") {
        return "Free".to_string();
    }
    if lower.contains("workspace")
        || lower.contains("team")
        || lower.contains("business")
        || lower.contains("enterprise")
    {
        return "Workspace".to_string();
    }
    if lower.contains("paid") || lower.contains("pro") || lower.contains("standard") {
        return "Paid".to_string();
    }
    if lower.contains("legacy") {
        return "Legacy".to_string();
    }
    humanize_model_name(tier)
}

fn humanize_model_name(name: &str) -> String {
    if name.trim().is_empty() {
        return String::new();
    }

    let normalized = name.trim().replace('_', "-");
    let mut words = Vec::new();
    for part in normalized.split('-').filter(|part| !part.is_empty()) {
        let lower = part.to_ascii_lowercase();
        if lower == "gemini" {
            words.push("Gemini".to_string());
            continue;
        }
        if lower == "api" {
            words.push("API".to_string());
            continue;
        }
        if part.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
            words.push(part.to_string());
            continue;
        }
        let mut chars = lower.chars();
        if let Some(first) = chars.next() {
            words.push(format!("{}{}", first.to_ascii_uppercase(), chars.as_str()));
        }
    }

    if words.is_empty() {
        return name.to_string();
    }
    words.join(" ")
}

impl AgentIdentity for GeminiAgent {
    fn descriptor(&self) -> &'static AgentDescriptor {
        &DESCRIPTOR
    }
}

impl Agent for GeminiAgent {
    fn auth_capability(&self) -> Option<&dyn AgentAuthCapability> {
        Some(self)
    }

    fn usage_capability(&self) -> Option<&dyn AgentUsageCapability> {
        Some(self)
    }
}

#[async_trait]
impl AgentAuthCapability for GeminiAgent {
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
impl AgentUsageCapability for GeminiAgent {
    async fn get_usage(&self, token: &AgentToken, client: &reqwest::Client) -> Result<UsageInfo> {
        get_usage(token, client).await
    }

    async fn get_usage_raw(&self, token: &AgentToken, client: &reqwest::Client) -> Result<Value> {
        get_usage_raw(token, client).await
    }

    fn process_quota_name(&self, quota_name: &str) -> String {
        humanize_model_name(quota_name)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{humanize_model_name, import_gemini_creds, parse_usage};

    #[test]
    fn parse_usage_maps_quota_buckets() {
        let load = json!({
            "tier": "paid",
            "cloudaicompanionProject": "gen-lang-client-123",
        });
        let quota = json!({
            "quotaBuckets": [
                {
                    "modelId": "gemini-2.5-pro",
                    "remainingFraction": 0.75,
                    "resetTime": "2026-03-06T00:00:00Z"
                },
                {
                    "modelId": "gemini-2.5-flash",
                    "remainingFraction": 0.25,
                    "resetTime": "2026-03-06T01:00:00Z"
                }
            ]
        });

        let usage = parse_usage(&load, &quota);
        assert_eq!(usage.plan.as_deref(), Some("Paid"));
        assert_eq!(usage.windows.len(), 2);
        assert!(usage.windows.iter().any(|window| {
            window.name == "gemini-2.5-pro"
                && (window.utilization_pct - 25.0).abs() < 0.0001
                && window.resets_at.as_deref() == Some("2026-03-06T00:00:00+00:00")
        }));
        assert!(usage.windows.iter().any(|window| {
            window.name == "gemini-2.5-flash"
                && (window.utilization_pct - 75.0).abs() < 0.0001
                && window.resets_at.as_deref() == Some("2026-03-06T01:00:00+00:00")
        }));
    }

    #[test]
    fn parse_usage_supports_alternate_remaining_fraction_keys() {
        let usage = parse_usage(
            &json!({}),
            &json!({
                "quotaBuckets": [
                    {
                        "modelId": "gemini-2.5-pro",
                        "remaining_fraction": 0.8
                    },
                    {
                        "modelId": "gemini-2.5-flash",
                        "remainingFractionValue": 0.3
                    }
                ]
            }),
        );

        assert_eq!(usage.windows.len(), 2);
        assert!(usage.windows.iter().any(|window| {
            window.name == "gemini-2.5-pro" && (window.utilization_pct - 20.0).abs() < 0.0001
        }));
        assert!(usage.windows.iter().any(|window| {
            window.name == "gemini-2.5-flash" && (window.utilization_pct - 70.0).abs() < 0.0001
        }));
    }

    #[test]
    fn parse_usage_handles_empty_response() {
        let usage = parse_usage(&json!({}), &json!({}));
        assert!(usage.plan.is_none());
        assert!(usage.windows.is_empty());
    }

    #[test]
    fn parse_usage_skips_unnamed_quota_buckets() {
        let usage = parse_usage(
            &json!({}),
            &json!({
                "quotaBuckets": [
                    {
                        "modelId": "gemini-2.5-pro",
                        "remainingFraction": 0.5,
                        "resetTime": "2026-03-06T00:00:00Z"
                    },
                    {
                        "modelId": " ",
                        "remainingFraction": 0.2,
                        "resetTime": "2026-03-06T00:00:00Z"
                    }
                ]
            }),
        );
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].name, "gemini-2.5-pro");
    }

    #[test]
    fn humanize_model_name_formats_gemini_ids() {
        assert_eq!(humanize_model_name("gemini-2.5-pro"), "Gemini 2.5 Pro");
        assert_eq!(
            humanize_model_name("gemini-2.5-flash-lite"),
            "Gemini 2.5 Flash Lite"
        );
        assert_eq!(humanize_model_name("api-usage"), "API Usage");
    }

    #[test]
    fn import_gemini_creds_parses_oauth_json() {
        let value = json!({
            "access_token": "access-token",
            "refresh_token": "refresh-token",
            "expiry_date": 1778000000000i64
        });

        let token = import_gemini_creds(&value).expect("should parse credentials");
        assert_eq!(token.access_token, "access-token");
        assert_eq!(token.refresh_token.as_deref(), Some("refresh-token"));
        assert_eq!(token.expires_at.timestamp_millis(), 1778000000000i64);
        assert!(token.last_refresh.is_some());
    }
}
