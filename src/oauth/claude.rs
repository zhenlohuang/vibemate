use std::io;

use chrono::{Duration, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::oauth::pkce::{generate_challenge, generate_state, generate_verifier};
use crate::oauth::token::{load_token, save_token, vibemate_dir, TokenData};
use crate::oauth::{UsageInfo, UsageWindow};

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTH_URL: &str = "https://claude.ai/oauth/authorize";
pub const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
pub const REDIRECT_URI: &str = "https://console.anthropic.com/oauth/code/callback";
pub const SCOPE: &str = "org:create_api_key user:profile user:inference";
pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const ANTHROPIC_BETA: &str = "oauth-2025-04-20";

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
    five_hour: UsageBucket,
    seven_day: UsageBucket,
    seven_day_opus: UsageBucket,
}

#[derive(Debug, Deserialize)]
struct UsageBucket {
    utilization: f64,
    resets_at: Option<String>,
}

pub async fn login() -> Result<()> {
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

    let client = reqwest::Client::new();
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
    let token = TokenData {
        access_token: payload.access_token,
        refresh_token: payload.refresh_token,
        expires_at: Utc::now() + Duration::seconds(payload.expires_in.unwrap_or(3600)),
        last_refresh: Some(Utc::now()),
    };

    let path = vibemate_dir()?.join("claude_auth.json");
    save_token(&path, &token)?;
    Ok(())
}

pub async fn refresh_if_needed(token: &mut TokenData) -> Result<()> {
    let now = Utc::now();
    if token.expires_at - now > Duration::minutes(5) {
        return Ok(());
    }

    let refresh_token = token
        .refresh_token
        .as_deref()
        .ok_or_else(|| AppError::TokenExpired {
            agent: "claude-code".to_string(),
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
            agent: "claude-code".to_string(),
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

    let path = vibemate_dir()?.join("claude_auth.json");
    save_token(&path, token)?;
    Ok(())
}

pub async fn get_usage(token: &TokenData) -> Result<UsageInfo> {
    let client = reqwest::Client::new();
    let response = client
        .get(USAGE_URL)
        .bearer_auth(&token.access_token)
        .header("anthropic-beta", ANTHROPIC_BETA)
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(AppError::TokenExpired {
            agent: "claude-code".to_string(),
        });
    }

    if !response.status().is_success() {
        return Err(AppError::OAuth(format!(
            "Claude usage request failed with status {}",
            response.status()
        )));
    }

    let usage: ClaudeUsageResponse = response.json().await?;

    Ok(UsageInfo {
        agent_name: "claude-code".to_string(),
        plan: None,
        windows: vec![
            UsageWindow {
                name: "five-hour".to_string(),
                utilization_pct: usage.five_hour.utilization,
                resets_at: usage.five_hour.resets_at,
            },
            UsageWindow {
                name: "seven-day".to_string(),
                utilization_pct: usage.seven_day.utilization,
                resets_at: usage.seven_day.resets_at,
            },
            UsageWindow {
                name: "seven-day-opus".to_string(),
                utilization_pct: usage.seven_day_opus.utilization,
                resets_at: usage.seven_day_opus.resets_at,
            },
        ],
    })
}

pub async fn load_saved_token() -> Result<Option<TokenData>> {
    let path = vibemate_dir()?.join("claude_auth.json");
    load_token(&path)
}
