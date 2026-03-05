# Claude Code Technical Reference

## Overview

| Field          | Value                          |
| -------------- | ------------------------------ |
| Agent ID       | `claude`                       |
| Display Name   | Claude                         |
| Token File     | `claude_auth.json`             |
| Source         | `src/agent/impls/claude.rs`    |

## OAuth Login

### Endpoints

| Endpoint     | URL                                                       |
| ------------ | --------------------------------------------------------- |
| Auth URL     | `https://claude.ai/oauth/authorize`                       |
| Token URL    | `https://console.anthropic.com/v1/oauth/token`            |
| Redirect URI | `https://console.anthropic.com/oauth/code/callback`       |

### Constants

- **Client ID**: `9d1c250a-e61b-44d9-88ed-5944d1962f5e`
- **Scope**: `org:create_api_key user:profile user:inference`

### Flow

1. Generate a PKCE verifier, challenge (S256), and state parameter.
2. Build the authorization URL with query parameters:
   - `code=true`
   - `client_id`
   - `response_type=code`
   - `redirect_uri`
   - `scope=org:create_api_key user:profile user:inference`
   - `code_challenge` (S256)
   - `state`
3. Open the authorization URL in the user's default browser.
4. Prompt the user to paste the code shown in the browser (format: `code#state`).
5. Split the pasted value on `#` to extract `code` and `state`.
6. Validate that the returned `state` matches the expected value.
7. Exchange the authorization code for tokens via POST to the Token URL.

**Note**: Unlike Codex, Claude Code does not use a local callback server. The user manually copies and pastes the authorization code from the browser.

### Token Exchange Parameters

```json
{
  "grant_type": "authorization_code",
  "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
  "redirect_uri": "https://console.anthropic.com/oauth/code/callback",
  "code": "<authorization_code>",
  "code_verifier": "<pkce_verifier>",
  "state": "<state>"
}
```

**Note**: The Claude Code token exchange includes a `state` parameter, which Codex does not send.

### Token Storage

Tokens are stored as an `AgentToken` struct in `~/.vibemate/claude_auth.json`:

```json
{
  "access_token": "...",
  "refresh_token": "...",
  "expires_at": "2026-03-01T12:00:00Z",
  "last_refresh": "2026-03-01T11:00:00Z"
}
```

The `expires_at` field defaults to `now + expires_in` (or `now + 3600s` if `expires_in` is absent).

## Token Refresh

A token refresh is triggered when **one** condition is true:

1. **Expiring soon**: token expires within 5 minutes (`expires_at - now <= 5min`).

**Note**: Unlike Codex, Claude Code does **not** have a staleness check (no 8-day refresh).

### Refresh Exchange Parameters

```json
{
  "grant_type": "refresh_token",
  "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
  "refresh_token": "<refresh_token>"
}
```

- On `401 Unauthorized`, a `TokenExpired` error is returned (user must re-login).
- On success, the access token, refresh token (falls back to existing if absent), `expires_at`, and `last_refresh` are updated and saved.

## Usage API

### Request

| Field   | Value                                              |
| ------- | -------------------------------------------------- |
| Method  | `GET`                                              |
| URL     | `https://api.anthropic.com/api/oauth/usage`        |
| Auth    | `Authorization: Bearer <access_token>`             |
| Headers | `anthropic-beta: oauth-2025-04-20`                 |

### Response Format

The response is a structured JSON object with known fields:

```json
{
  "five_hour": {
    "utilization": 6.0,
    "resets_at": "2026-02-27T20:00:00Z"
  },
  "seven_day": {
    "utilization": 1.0,
    "resets_at": "2026-03-06T15:00:00Z"
  },
  "seven_day_opus": null,
  "extra_usage": {
    "sonnet_4": {
      "used_percent": 23,
      "reset_at": 1772760122
    }
  }
}
```

The response is deserialized into a typed `ClaudeUsageResponse` struct:

| Field            | Type                  | Description                          |
| ---------------- | --------------------- | ------------------------------------ |
| `five_hour`      | `Option<UsageBucket>` | 5-hour rolling usage window          |
| `seven_day`      | `Option<UsageBucket>` | 7-day rolling usage window           |
| `seven_day_opus` | `Option<UsageBucket>` | 7-day Opus-specific usage window     |
| `extra_usage`    | `Option<Value>`       | Additional usage data (flexible JSON)|

Each `UsageBucket` contains:

| Field         | Type             | Description                     |
| ------------- | ---------------- | ------------------------------- |
| `utilization` | `Option<f64>`    | Usage percentage (0-100)        |
| `resets_at`   | `Option<String>` | ISO 8601 reset timestamp        |

### Parsing Logic

1. The three primary windows (`five_hour`, `seven_day`, `seven_day_opus`) are extracted from their typed fields. A window is included only if both `utilization` and `resets_at` are present and non-empty.
2. The `extra_usage` field is recursively traversed to extract additional `UsageWindow` entries (marked as `is_extra: true`). It supports:
   - Object form: keys become window names (e.g. `sonnet_4` becomes `sonnet-4`).
   - Array form: items use `name`, `quota_name`, `window`, or `type` fields as names.
   - Aggregate form: top-level `extra_usage` with `used_credits` / `monthly_limit` fields.
3. Extra windows use a flexible utilization parser that checks percent fields, ratio fields, `used/limit`, and `(limit - remaining)/limit`.
4. Reset timestamps in extra windows support `resets_at`, `reset_at`, `resetAt`, `reset_after_seconds`, and several other variants. Duration-style keys (`reset_after_seconds`, `reset_after`, `seconds_until_reset`) are converted to absolute timestamps.

### Parsed Output Structure (`UsageWindow`)

| Field               | Type             | Description                                              |
| ------------------- | ---------------- | -------------------------------------------------------- |
| `name`              | `String`         | Window name, e.g. `five-hour`, `seven-day`, `seven-day-opus` |
| `utilization_pct`   | `f64`            | Usage percentage (0-100)                                 |
| `resets_at`         | `Option<String>` | ISO 8601 reset timestamp                                 |
| `is_extra`          | `bool`           | `true` for windows from `extra_usage`                    |
| `source_limit_name` | `Option<String>` | Always `None` for Claude Code windows                    |

### Output Metadata

The `UsageInfo` struct for Claude Code:

| Field          | Value                           |
| -------------- | ------------------------------- |
| `agent_name`   | `claude`                        |
| `display_name` | `Claude`                        |
| `plan`         | Always `None` (not provided by API) |
| `extra_usage`  | Raw `extra_usage` JSON value (preserved for display) |
