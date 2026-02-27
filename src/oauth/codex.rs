use chrono::{Duration, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::error::{AppError, Result};
use crate::oauth::pkce::{generate_challenge, generate_verifier};
use crate::oauth::token::{load_token, save_token, vibemate_dir, TokenData};
use crate::oauth::{UsageInfo, UsageWindow};

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const CALLBACK_PORT: u16 = 1455;
pub const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

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

    let mut auth_url =
        Url::parse(AUTH_URL).map_err(|e| AppError::OAuth(format!("Invalid AUTH_URL: {e}")))?;
    auth_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");

    let callback = tokio::spawn(crate::oauth::callback::start_callback_server(CALLBACK_PORT));

    let auth_url_string = auth_url.to_string();
    tracing::info!(
        "Open this URL in your browser if it does not open automatically: {auth_url_string}"
    );
    let _ = open::that(&auth_url_string);

    let code = callback
        .await
        .map_err(|e| AppError::OAuth(format!("Callback task failed: {e}")))??;

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
    let token = TokenData {
        access_token: token_payload.access_token,
        refresh_token: token_payload.refresh_token,
        expires_at: Utc::now() + Duration::seconds(token_payload.expires_in.unwrap_or(3600)),
        last_refresh: Some(Utc::now()),
    };

    let path = vibemate_dir()?.join("codex_auth.json");
    save_token(&path, &token)?;
    Ok(())
}

pub async fn refresh_if_needed(token: &mut TokenData) -> Result<()> {
    let now = Utc::now();

    if let Some(last_refresh) = token.last_refresh {
        if now.signed_duration_since(last_refresh) < Duration::days(8) {
            return Ok(());
        }
    } else if now < token.expires_at {
        return Ok(());
    }

    let refresh_token = token
        .refresh_token
        .as_deref()
        .ok_or_else(|| AppError::TokenExpired {
            agent: "codex".to_string(),
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
            agent: "codex".to_string(),
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

    let path = vibemate_dir()?.join("codex_auth.json");
    save_token(&path, token)?;
    Ok(())
}

pub async fn get_usage(token: &TokenData) -> Result<UsageInfo> {
    let client = reqwest::Client::new();
    let response = client
        .get(USAGE_URL)
        .bearer_auth(&token.access_token)
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(AppError::TokenExpired {
            agent: "codex".to_string(),
        });
    }

    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Codex usage request failed with status {}",
            response.status()
        )));
    }

    let value: Value = response.json().await?;
    Ok(parse_usage(value))
}

pub async fn load_saved_token() -> Result<Option<TokenData>> {
    let path = vibemate_dir()?.join("codex_auth.json");
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

    if let Some(items) = value.get("windows").and_then(Value::as_array) {
        for item in items {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let utilization_pct = item
                .get("utilization_pct")
                .and_then(Value::as_f64)
                .or_else(|| item.get("utilization").and_then(Value::as_f64))
                .unwrap_or(0.0);
            let resets_at = item
                .get("resets_at")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            windows.push(UsageWindow {
                name,
                utilization_pct,
                resets_at,
            });
        }
    }

    if windows.is_empty() {
        for key in ["five_hour", "seven_day", "seven_day_opus"] {
            if let Some(window) = value.get(key) {
                windows.push(UsageWindow {
                    name: key.replace('_', "-"),
                    utilization_pct: window
                        .get("utilization")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    resets_at: window
                        .get("resets_at")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                });
            }
        }
    }

    UsageInfo {
        agent_name: "codex".to_string(),
        plan,
        windows,
    }
}
