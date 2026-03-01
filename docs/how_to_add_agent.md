# How to Add an Agent

This guide walks through adding a new agent to Vibemate. An agent represents an AI coding tool (e.g., Claude Code, Codex) and provides OAuth authentication and usage tracking.

## Overview

The agent system is built on a trait hierarchy defined in `src/agent/traits.rs`:

| Trait                  | Purpose                                          | Required |
| ---------------------- | ------------------------------------------------ | -------- |
| `AgentIdentity`        | Returns the agent's `AgentDescriptor` (id, name, token file) | Yes      |
| `Agent`                | Composes identity with optional capabilities     | Yes      |
| `AgentAuthCapability`  | OAuth login, token loading, and token refresh     | Optional |
| `AgentUsageCapability` | Fetching and parsing usage data                  | Optional |

An agent must implement `AgentIdentity` and `Agent`. Auth and usage capabilities are opt-in — return `Some(self)` from `Agent::auth_capability()` or `Agent::usage_capability()` to enable them.

## Prerequisites

Before starting, gather:

- **OAuth credentials**: Client ID, authorization URL, token URL, redirect URI, and scopes.
- **Usage API details**: Endpoint URL, authentication headers, and response format.
- **Token file name**: A unique filename for storing tokens (e.g., `myagent_auth.json`).

## Step-by-Step Guide

### Step 1: Create the Agent Implementation

Create a new file at `src/agent/impls/<name>.rs`. Use `src/agent/impls/claude.rs` (manual paste OAuth) or `src/agent/impls/codex.rs` (local callback server OAuth) as a reference.

#### 1a. Define Constants and Descriptor

```rust
use crate::agent::auth::token::{AgentToken, auth_file_path, load_token, save_token};
use crate::agent::{
    Agent, AgentAuthCapability, AgentDescriptor, AgentIdentity,
    AgentUsageCapability, UsageInfo, UsageWindow,
};
use crate::error::{AppError, Result};

pub const CLIENT_ID: &str = "<your-client-id>";
pub const AUTH_URL: &str = "https://example.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://example.com/oauth/token";
pub const REDIRECT_URI: &str = "http://localhost:<port>/auth/callback";
pub const USAGE_URL: &str = "https://example.com/api/usage";
const TOKEN_FILE_NAME: &str = "myagent_auth.json";

pub const DESCRIPTOR: AgentDescriptor = AgentDescriptor {
    id: "my-agent",
    display_name: "My Agent",
    token_file_name: TOKEN_FILE_NAME,
};
```

The `AgentDescriptor` fields:

| Field             | Description                                                 |
| ----------------- | ----------------------------------------------------------- |
| `id`              | Unique identifier used in CLI commands and config (kebab-case) |
| `display_name`    | Human-readable name shown in the dashboard and output       |
| `token_file_name` | Filename for token storage under `~/.vibemate/auth/`        |

#### 1b. Define the Agent Struct and Response Types

```rust
pub struct MyAgent;

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}
```

Define any provider-specific response types needed for usage parsing.

#### 1c. Implement Traits

**`AgentIdentity`** — return a reference to the static descriptor:

```rust
impl AgentIdentity for MyAgent {
    fn descriptor(&self) -> &'static AgentDescriptor {
        &DESCRIPTOR
    }
}
```

**`Agent`** — wire up capabilities:

```rust
impl Agent for MyAgent {
    fn auth_capability(&self) -> Option<&dyn AgentAuthCapability> {
        Some(self)
    }

    fn usage_capability(&self) -> Option<&dyn AgentUsageCapability> {
        Some(self)
    }
}
```

**`AgentAuthCapability`** — implement OAuth login, token loading, and refresh:

```rust
#[async_trait]
impl AgentAuthCapability for MyAgent {
    async fn login(&self, client: &reqwest::Client) -> Result<()> {
        login(client).await
    }

    async fn load_saved_token(&self) -> Result<Option<AgentToken>> {
        let path = auth_file_path(TOKEN_FILE_NAME)?;
        load_token(&path)
    }

    async fn refresh_if_needed(
        &self,
        token: &mut AgentToken,
        client: &reqwest::Client,
    ) -> Result<()> {
        refresh_if_needed(token, client).await
    }
}
```

**`AgentUsageCapability`** — implement usage fetching:

```rust
#[async_trait]
impl AgentUsageCapability for MyAgent {
    async fn get_usage(
        &self,
        token: &AgentToken,
        client: &reqwest::Client,
    ) -> Result<UsageInfo> {
        get_usage(token, client).await
    }

    async fn get_usage_raw(
        &self,
        token: &AgentToken,
        client: &reqwest::Client,
    ) -> Result<Value> {
        get_usage_raw(token, client).await
    }
}
```

You can optionally override `quota_name()` and `display_quota_name()` for custom window name formatting. See `src/agent/impls/codex.rs` for an example.

#### 1d. Implement the OAuth Login Function

Choose one of two patterns:

**Pattern A: Local callback server** (used by Codex)

The browser redirects to `http://localhost:<port>/auth/callback` after authorization. A local Axum server receives the callback automatically.

```rust
pub async fn login(client: &reqwest::Client) -> Result<()> {
    let verifier = generate_verifier();
    let challenge = generate_challenge(&verifier);
    let expected_state = generate_state();

    // Build authorization URL with query parameters
    let mut auth_url = Url::parse(AUTH_URL)
        .map_err(|e| AppError::OAuth(format!("Invalid AUTH_URL: {e}")))?;
    auth_url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &expected_state);

    // Start local callback server
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", CALLBACK_PORT)).await?;
    let callback = tokio::spawn(
        crate::agent::auth::callback::start_callback_server(listener)
    );

    // Open browser
    let _ = open::that(auth_url.as_str());

    // Wait for callback, validate state, exchange code for token
    let payload = callback.await??;
    // ... validate state, exchange code, save token
}
```

**Pattern B: Manual paste** (used by Claude Code)

The user copies a `code#state` value from the browser and pastes it into the terminal.

```rust
pub async fn login(client: &reqwest::Client) -> Result<()> {
    let verifier = generate_verifier();
    let challenge = generate_challenge(&verifier);
    let state = generate_state();

    // Build and open authorization URL
    let auth_url = format!(
        "{AUTH_URL}?client_id={CLIENT_ID}&response_type=code\
         &redirect_uri={REDIRECT_URI}&code_challenge={challenge}\
         &code_challenge_method=S256&state={state}"
    );
    let _ = open::that(&auth_url);

    // Read pasted code from stdin
    println!("Paste the code shown in browser (format: code#state):");
    // ... read input, split on '#', validate state, exchange code, save token
}
```

Both patterns use PKCE utilities from `src/agent/auth/pkce.rs` and token persistence from `src/agent/auth/token.rs`.

#### 1e. Implement Token Refresh

Check token expiry and refresh proactively:

```rust
pub async fn refresh_if_needed(
    token: &mut AgentToken,
    client: &reqwest::Client,
) -> Result<()> {
    let now = Utc::now();
    if token.expires_at - now > Duration::minutes(5) {
        return Ok(());
    }

    let refresh_token = token.refresh_token.as_deref()
        .ok_or_else(|| AppError::TokenExpired {
            agent: DESCRIPTOR.id.to_string(),
        })?;

    // POST to TOKEN_URL with grant_type=refresh_token
    // Update token fields and save to disk
}
```

Return `AppError::TokenExpired` on `401 Unauthorized` so the user is prompted to re-login.

#### 1f. Implement Usage Parsing

Fetch usage data and convert it into `UsageInfo`:

```rust
pub async fn get_usage(
    token: &AgentToken,
    client: &reqwest::Client,
) -> Result<UsageInfo> {
    let value = get_usage_raw(token, client).await?;
    // Parse provider-specific JSON into UsageInfo
    Ok(UsageInfo {
        agent_name: DESCRIPTOR.id.to_string(),
        display_name: DESCRIPTOR.display_name.to_string(),
        plan: None, // or parse from response
        windows: vec![/* parsed UsageWindow entries */],
        extra_usage: None,
    })
}
```

Each `UsageWindow` needs at minimum:

| Field             | Description                                    |
| ----------------- | ---------------------------------------------- |
| `name`            | Window identifier in kebab-case (e.g., `five-hour`) |
| `utilization_pct` | Usage percentage from 0.0 to 100.0             |
| `resets_at`       | ISO 8601 timestamp when the window resets       |

### Step 2: Register the Module

Add the new module to `src/agent/impls/mod.rs`:

```rust
pub mod claude;
pub mod codex;
pub mod myagent; // Add this line
```

### Step 3: Register the Agent

Add the agent to the registry in `src/agent/registry.rs`:

```rust
impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: vec![
                Box::new(codex::CodexAgent),
                Box::new(claude::ClaudeAgent),
                Box::new(myagent::MyAgent), // Add this line
            ],
        }
    }
}
```

Update the import at the top of the file:

```rust
use super::impls::{claude, codex, myagent};
```

### Step 4: Add Unit Tests

Add a `#[cfg(test)]` module at the bottom of your implementation file. Test usage parsing with representative JSON payloads:

```rust
#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::parse_usage;

    #[test]
    fn parse_usage_extracts_windows() {
        let value = json!({
            // Provider-specific response shape
        });

        let usage = parse_usage(value).expect("parse should succeed");
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].name, "five-hour");
        assert!((usage.windows[0].utilization_pct - 50.0).abs() < 0.0001);
    }
}
```

Run tests with `cargo test` before committing.

### Step 5: Update Documentation

1. **Create `docs/agents/<name>.md`** — document the agent's OAuth endpoints, usage API format, and parsing logic. Follow the format of `docs/agents/claude_code.md` or `docs/agents/codex.md`.
2. **Update `docs/configuration.md`** — if the agent requires config entries, document them.

## Key Types Reference

| Type              | Location                    | Description                                    |
| ----------------- | --------------------------- | ---------------------------------------------- |
| `AgentDescriptor` | `src/agent/types.rs`        | Static metadata: `id`, `display_name`, `token_file_name` |
| `AgentToken`      | `src/agent/auth/token.rs`   | Persisted OAuth token with `access_token`, `refresh_token`, `expires_at`, `last_refresh` |
| `UsageInfo`       | `src/agent/types.rs`        | Parsed usage output: `agent_name`, `display_name`, `plan`, `windows`, `extra_usage` |
| `UsageWindow`     | `src/agent/types.rs`        | Single usage window: `name`, `utilization_pct`, `resets_at`, `is_extra`, `source_limit_name` |
| `CallbackPayload` | `src/agent/auth/callback.rs`| OAuth callback data: `code`, `state`, `error`, `error_description` |

## Verification Checklist

1. `cargo build` — project compiles without errors.
2. `cargo test` — all tests pass, including new ones.
3. `cargo clippy --all-targets --all-features` — no warnings.
4. `cargo fmt` — code is formatted.
5. `cargo run -- login <agent-id>` — OAuth flow completes and token is saved to `~/.vibemate/auth/<token_file_name>`.
6. `cargo run -- usage` — new agent appears in usage output with correct windows.
7. `cargo run -- dashboard` — new agent renders in the TUI dashboard.
