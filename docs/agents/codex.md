# Codex Technical Reference

## Overview

| Field          | Value              |
| -------------- | ------------------ |
| Agent ID       | `codex`            |
| Display Name   | Codex              |
| Token File     | `codex_auth.json`  |
| Source          | `src/agent/impls/codex.rs` |

## OAuth Login

### Endpoints

| Endpoint     | URL                                              |
| ------------ | ------------------------------------------------ |
| Auth URL     | `https://auth.openai.com/oauth/authorize`        |
| Token URL    | `https://auth.openai.com/oauth/token`            |
| Redirect URI | `http://localhost:1455/auth/callback`             |

### Constants

- **Client ID**: `app_EMoamEEZ73f0CkXaXp7hrann`
- **Callback Port**: `1455`

### Flow

1. Generate a PKCE verifier, challenge (S256), and state parameter.
2. Build the authorization URL with query parameters:
   - `response_type=code`
   - `client_id`
   - `redirect_uri`
   - `code_challenge` (S256)
   - `state`
3. Start a local TCP listener on `127.0.0.1:1455` to receive the OAuth callback.
4. Open the authorization URL in the user's default browser.
5. Wait for the callback server to receive the redirect with `code` and `state`.
6. Validate that the returned `state` matches the expected value.
7. Exchange the authorization code for tokens via POST to the Token URL.

### Token Exchange Parameters

```json
{
  "grant_type": "authorization_code",
  "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
  "redirect_uri": "http://localhost:1455/auth/callback",
  "code": "<authorization_code>",
  "code_verifier": "<pkce_verifier>"
}
```

### Token Storage

Tokens are stored as an `AgentToken` struct in `~/.vibemate/codex_auth.json`:

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

A token refresh is triggered when **either** condition is true:

1. **Expiring soon**: token expires within 5 minutes (`expires_at - now <= 5min`).
2. **Stale refresh**: last refresh was 8 or more days ago (`now - last_refresh >= 8 days`).

### Refresh Exchange Parameters

```json
{
  "grant_type": "refresh_token",
  "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
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
| URL     | `https://chatgpt.com/backend-api/wham/usage`      |
| Auth    | `Authorization: Bearer <access_token>`             |

### Response Format

The response is a JSON object with a flexible, multi-format structure. The parser handles several shapes:

```json
{
  "plan": "plus",
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
}
```

### Parsing Logic

The parser extracts a `plan` field (checking `plan`, `plan_type`, `subscription_plan` in order) and builds `UsageWindow` entries from multiple sources, deduplicating by name:

1. **`rate_limit`** — top-level rate limit with `primary_window` (18000s = five-hour) and `secondary_window` (604800s = seven-day).
2. **`code_review_rate_limit`** — same structure, prefixed with `code-review`.
3. **`additional_rate_limits`** — array of `{ limit_name, rate_limit }` objects; windows are marked as `is_extra: true` and prefixed with the normalized `limit_name`.
4. **`windows`** — array or object of direct window entries.
5. **Top-level keys** — `five_hour`, `seven_day`, `seven_day_opus` as direct window objects.
6. **Group keys** — `usage`, `rate_limits`, `limits`, `buckets` as objects or arrays of windows.

### Parsed Output Structure (`UsageWindow`)

| Field               | Type             | Description                                              |
| ------------------- | ---------------- | -------------------------------------------------------- |
| `name`              | `String`         | Window name, e.g. `five-hour`, `seven-day` (underscores replaced with hyphens) |
| `utilization_pct`   | `f64`            | Usage percentage (0-100)                                 |
| `resets_at`         | `Option<String>` | ISO 8601 reset timestamp (unix timestamps are converted) |
| `is_extra`          | `bool`           | `true` for windows from `additional_rate_limits`         |
| `source_limit_name` | `Option<String>` | Original `limit_name` for extra windows                  |

Utilization is derived from (in priority order):
- Direct percentage fields (`utilization_pct`, `used_percent`, etc.) — values <= 1.0 are treated as ratios and multiplied by 100
- Ratio fields (`utilization_ratio`, `usage_ratio`, etc.) — always multiplied by 100
- `used / limit` computation
- `(limit - remaining) / limit` computation
