# Vibemate System Design Document

## 1. Project Overview & Scope

Vibemate is a Rust CLI tool that acts as a local AI model proxy and usage dashboard for coding agents. It is inspired by [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) (Go), but intentionally limited in scope to two coding agents: **Codex** (OpenAI) and **Claude Code** (Anthropic).

### Goals

- **Unified proxy**: Expose a single local HTTP endpoint that routes requests to multiple upstream AI providers based on configurable model routing rules.
- **OAuth login**: Authenticate with Codex and Claude Code via PKCE OAuth flows, storing tokens locally.
- **Usage monitoring**: Query quota/usage information from both providers and display it in the terminal.
- **TUI dashboard**: A terminal UI that combines the proxy server, usage gauges, and live request logs into one view.

### CLI Commands

| Command | Description |
|---|---|
| `vibemate login codex` | Authenticate with OpenAI Codex via OAuth PKCE |
| `vibemate login claude-code` | Authenticate with Anthropic Claude Code via OAuth PKCE |
| `vibemate usage` | Display quota/usage for logged-in coding agents |
| `vibemate usage --json` | Display normalized usage as pretty JSON |
| `vibemate usage --raw` | Display raw usage payloads from each agent as pretty JSON |
| `vibemate proxy` | Start the proxy server (foreground) |
| `vibemate dashboard` | Start the TUI dashboard (proxy + usage + request logs) |

---

## 2. Project Structure

Vibemate is a **single Cargo crate** (not a workspace — a workspace is overkill for a CLI tool of this scope).

```
vibemate/
  Cargo.toml
  Cargo.lock
  README.md
  LICENSE
  .gitignore
  docs/
    system_design.md
  src/
    main.rs                    # Entry point, CLI argument parsing
    cli/
      mod.rs                   # CLI module root
      login.rs                 # `vibemate login` command
      usage.rs                 # `vibemate usage` command
      proxy.rs                 # `vibemate proxy` command
      dashboard.rs             # `vibemate dashboard` command
    config/
      mod.rs                   # Config loading, validation, defaults
      types.rs                 # Config structs (serde-backed)
    proxy/
      mod.rs                   # Proxy module root
      server.rs                # Axum server, route registration
      router.rs                # Model-based routing logic
      handler.rs               # Request handler (forward + stream)
      middleware.rs             # Logging middleware, request ID
      stream.rs                # SSE streaming utilities
    oauth/
      mod.rs                   # OAuth module root, trait definition
      codex.rs                 # OpenAI Codex OAuth + usage
      claude.rs                # Anthropic Claude Code OAuth + usage
      token.rs                 # Token storage, refresh, types
      pkce.rs                  # PKCE challenge/verifier generation
      callback.rs              # Local HTTP callback server
    provider/
      mod.rs                   # Provider trait + registry
      openai.rs                # OpenAI provider (forward requests)
      anthropic.rs             # Anthropic provider (forward requests)
    tui/
      mod.rs                   # TUI module root
      app.rs                   # Application state
      ui.rs                    # Layout rendering
      widgets/
        mod.rs
        status.rs              # Proxy status widget
        usage.rs               # Quota/usage widget
        logs.rs                # Request log widget
    error.rs                   # Unified error types
```

---

## 3. Architecture Overview

Vibemate follows a layered architecture where each layer has well-defined responsibilities and dependencies flow downward.

```
+------------------+
|   CLI Layer      |  clap — parse commands, dispatch
+------------------+
         |
+------------------+     +------------------+
|  Proxy Layer     |     |   TUI Layer      |  ratatui
|  (axum server)   |<--->|  (dashboard)     |
+------------------+     +------------------+
         |                        |
+------------------+     +------------------+
|  Router Layer    |     |   OAuth Layer    |
|  (model routing) |     |   (login/usage)  |
+------------------+     +------------------+
         |                        |
+------------------+     +------------------+
|  Provider Layer  |     |  Token Storage   |
|  (reqwest fwd)   |     |  (~/.vibemate/)  |
+------------------+     +------------------+
         |
+------------------+
|  Config Layer    |
|  (TOML + serde)  |
+------------------+
```

### Data Flow

1. **CLI** parses commands and dispatches to the appropriate subsystem.
2. The **proxy server** receives HTTP requests from coding agents.
3. The **router** inspects the `model` field in the request body to select a provider and optionally remap the model name.
4. The **provider** forwards the request to the upstream API with the configured headers.
5. Responses (including SSE streams) are relayed back to the client.
6. The **TUI dashboard** composes the proxy server, usage polling, and request logging into one terminal interface.

---

## 4. Module Design

### 4.1 CLI (`src/cli/`)

Uses clap derive for argument parsing.

```rust
#[derive(Parser)]
#[command(name = "vibemate", version, about = "AI model proxy and usage dashboard")]
struct Cli {
    #[arg(long, default_value = "~/.vibemate/config.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with a coding agent
    Login {
        /// Agent name: "codex" or "claude-code"
        agent: String,
    },
    /// Display quota/usage for logged-in agents
    Usage {
        /// Output normalized usage as JSON
        #[arg(long, conflicts_with = "raw")]
        json: bool,
        /// Output provider raw usage payloads as JSON
        #[arg(long, conflicts_with = "json")]
        raw: bool,
    },
    /// Start the proxy server
    Proxy,
    /// Start the TUI dashboard
    Dashboard,
}
```

### 4.2 Config (`src/config/`)

Loads and validates the TOML configuration file. See [Section 5](#5-configuration-schema) for the full schema.

```rust
#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub providers: HashMap<String, ProviderConfig>,
    pub routing: RoutingConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,          // default: "127.0.0.1"
    pub port: u16,             // default: 12345
    pub proxy: Option<String>, // optional network proxy URL
}

#[derive(Debug, Deserialize)]
pub struct ProviderConfig {
    pub base_url: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct RoutingConfig {
    pub default_provider: String,
    pub rules: Vec<RoutingRule>,
}

#[derive(Debug, Deserialize)]
pub struct RoutingRule {
    pub pattern: String,        // glob-style wildcard pattern
    pub provider: String,
    pub model: Option<String>,  // optional: remap model name
}
```

### 4.3 Provider (`src/provider/`)

Providers are generic — no type distinction. Each provider is defined by a `base_url` and a set of `headers`.

```rust
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub headers: HashMap<String, String>,
}

pub struct ProviderRegistry {
    providers: HashMap<String, Provider>,
}

impl ProviderRegistry {
    pub fn get(&self, name: &str) -> Option<&Provider>;
}
```

### 4.4 Router (`src/proxy/router.rs`)

Routes incoming requests to the appropriate provider based on the model name.

```rust
pub struct ResolvedRoute {
    pub provider: String,  // provider name
    pub model: String,     // resolved model name (after optional remapping)
}

pub struct ModelRouter {
    default_provider: String,
    rules: Vec<RoutingRule>,
}

impl ModelRouter {
    /// Match rules in order, first match wins.
    /// Unmatched models fall through to default_provider with original name.
    pub fn resolve(&self, model: &str) -> ResolvedRoute;
}
```

Routing rules use glob-style wildcard matching (`*` matches any sequence of characters). Rules are evaluated in order; the first match wins. If no rule matches, the `default_provider` is used and the original model name is preserved.

### 4.5 OAuth (`src/oauth/`)

```rust
#[async_trait]
pub trait OAuthAgent: Send + Sync {
    fn name(&self) -> &str;
    async fn login(&self) -> Result<()>;
    fn is_logged_in(&self) -> bool;
    async fn get_usage(&self) -> Result<UsageInfo>;
    async fn refresh_if_needed(&mut self) -> Result<()>;
}

pub struct UsageInfo {
    pub agent_name: String,
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
}

pub struct UsageWindow {
    pub name: String,          // e.g., "5-hour", "weekly"
    pub utilization_pct: f64,  // 0.0 – 100.0
    pub resets_at: Option<String>,
}
```

PKCE is implemented directly with `sha2` + `base64` + `rand` (the `oauth2` crate is intentionally omitted to avoid complexity and transitive dependencies).

### 4.6 TUI (`src/tui/`)

```rust
pub struct App {
    pub proxy_addr: String,
    pub proxy_running: bool,
    pub usage: Vec<UsageInfo>,
    pub logs: VecDeque<RequestLog>,  // bounded buffer (~1000 entries)
    pub log_scroll: usize,
}

pub struct RequestLog {
    pub timestamp: String,
    pub method: String,
    pub path: String,
    pub model: String,
    pub provider: String,
    pub status: u16,
    pub latency_ms: u64,
}
```

Communication between the proxy handler and the TUI uses a `tokio::sync::broadcast::Sender<RequestLog>` channel.

### 4.7 Error (`src/error.rs`)

```rust
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP client error: {0}")]
    HttpClient(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("Token expired for {agent}. Please run `vibemate login {agent}`")]
    TokenExpired { agent: String },

    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Missing 'model' field in request body")]
    MissingModel,

    #[error("Proxy server error: {0}")]
    ProxyServer(String),

    #[error("Upstream error: HTTP {status} from {provider}")]
    Upstream { status: u16, provider: String },
}
```

---

## 5. Configuration Schema

Configuration is stored at `~/.vibemate/config.toml`. The path can be overridden with `--config`.

```toml
[server]
host = "127.0.0.1"
port = 12345
proxy = "socks5://127.0.0.1:1080"   # optional, HTTP/HTTPS/SOCKS5

# Providers: generic targets with base_url + headers
[providers.openai-official]
base_url = "https://api.openai.com"
headers = { Authorization = "Bearer sk-..." }

[providers.anthropic-official]
base_url = "https://api.anthropic.com"
headers = { x-api-key = "sk-ant-...", anthropic-version = "2023-06-01" }

# Routing: default provider + ordered wildcard rules
[routing]
default_provider = "openai-official"

[[routing.rules]]
pattern = "my-gpt4"
provider = "openai-official"
model = "gpt-4"                      # optional: remap model name

[[routing.rules]]
pattern = "claude-*"
provider = "anthropic-official"

# Unmatched models -> default_provider, original model name preserved
```

### Key Design Decisions

1. **No provider type distinction** — providers are purely generic with `base_url` + `headers`.
2. **`default_provider`** in `[routing]` handles unmatched models (no catch-all `*` rule needed).
3. **Routing rules** are an ordered list with glob-style wildcard matching (`*` matches any characters); first match wins.
4. **Optional model name remapping** — if `model` field is omitted in a rule, the original model name passes through.
5. **Server and network proxy are merged** into a single `[server]` section.

### Token Storage (separate from config)

| File | Purpose |
|---|---|
| `~/.vibemate/auth/codex_auth.json` | Codex OAuth tokens |
| `~/.vibemate/auth/claude_auth.json` | Claude Code OAuth tokens |

---

## 6. API Endpoints & Proxy Flow

### Endpoints

| Vibemate Endpoint | Format | Upstream Path |
|---|---|---|
| `POST /api/v1/chat/completions` | OpenAI | `{base_url}/v1/chat/completions` |
| `POST /api/v1/responses` | OpenAI | `{base_url}/v1/responses` |
| `POST /api/v1/messages` | Anthropic | `{base_url}/v1/messages` |

### Proxy Flow (Step-by-Step)

1. **Receive**: Axum handler receives the incoming request with the raw body bytes.
2. **Parse**: Deserialize body as JSON, extract the `model` field.
3. **Route**: `ModelRouter::resolve(model)` returns `ResolvedRoute { provider, model }`.
4. **Transform**: Replace the `model` field in the body with the resolved model name (if remapped).
5. **Build upstream request**: Use the shared `reqwest::Client` (configured with optional network proxy from `[server].proxy`).
6. **Forward**: Send request to `{provider.base_url}{upstream_path}` with the provider's configured headers.
7. **Check streaming**: If the request has `stream: true`:
   - Read the upstream response as a byte stream.
   - Relay each SSE event back through axum's SSE response.
   - Use `tokio::io::BufReader` on the stream for line-by-line reading.
8. **Non-streaming**: Read the full upstream response body and return it with the original status code and content-type.
9. **Log**: Emit a `RequestLog` event via the broadcast channel (consumed by the TUI if active).
10. **Error handling**: On upstream errors, return an appropriate HTTP error response (502 for upstream failures, 400 for bad requests, 500 for internal errors).

### Header Handling

Headers are configured per-provider in the TOML config (not hardcoded by type). The proxy:
- Sets the provider's configured headers on the upstream request.
- Sets `Content-Type: application/json`.
- Forwards the client's `Accept` header.

---

## 7. OAuth Flow

### 7.1 Codex (OpenAI)

**Constants:**

| Key | Value |
|---|---|
| `CLIENT_ID` | `app_EMoamEEZ73f0CkXaXp7hrann` |
| `AUTH_URL` | `https://auth.openai.com/oauth/authorize` |
| `TOKEN_URL` | `https://auth.openai.com/oauth/token` |
| `CALLBACK_PORT` | `1455` |
| `CALLBACK_PATH` | `/auth/callback` |
| `REDIRECT_URI` | `http://localhost:1455/auth/callback` |
| `USAGE_URL` | `https://chatgpt.com/backend-api/wham/usage` |
| Token file | `~/.vibemate/auth/codex_auth.json` |

**Flow:**

1. Generate PKCE `code_verifier` (32 random bytes, base64url-encoded), `code_challenge` (SHA-256 of verifier, base64url-encoded), and random `state` for CSRF protection.
2. Start a local HTTP callback server on `127.0.0.1:1455`.
3. Build the authorization URL with parameters: `response_type=code`, `client_id`, `redirect_uri`, `code_challenge`, `code_challenge_method=S256`, `state`.
4. Open the browser to the authorization URL (print URL to terminal as fallback).
5. User authenticates at `auth.openai.com`.
6. OpenAI redirects to `http://localhost:1455/auth/callback?code=<code>&state=<state>`.
7. Callback server captures the callback payload and validates `state`.
8. Exchange code for tokens: POST to `TOKEN_URL` with `grant_type=authorization_code`, `client_id`, `redirect_uri`, `code`, `code_verifier`.
9. Save tokens to `~/.vibemate/auth/codex_auth.json` (`access_token`, `refresh_token`, `expires_at`).

**Token Refresh:** Refresh access token if `last_refresh` is older than 8 days.

**Usage Query:** `GET https://chatgpt.com/backend-api/wham/usage` with `Authorization: Bearer <access_token>`. Returns rate limit windows (5h, weekly) with utilization percentages.

### 7.2 Claude Code (Anthropic)

**Constants:**

| Key | Value |
|---|---|
| `CLIENT_ID` | `9d1c250a-e61b-44d9-88ed-5944d1962f5e` |
| `AUTH_URL` | `https://claude.ai/oauth/authorize` |
| `TOKEN_URL` | `https://console.anthropic.com/v1/oauth/token` |
| `REDIRECT_URI` | `https://console.anthropic.com/oauth/code/callback` |
| `SCOPE` | `org:create_api_key user:profile user:inference` |
| `USAGE_URL` | `https://api.anthropic.com/api/oauth/usage` |
| `ANTHROPIC_BETA` | `oauth-2025-04-20` |
| Token file | `~/.vibemate/auth/claude_auth.json` |

**Flow:**

1. Generate PKCE `code_verifier` (43–128 chars, base64url-encoded) and `code_challenge` (SHA-256).
2. Generate random `state` token (32 random bytes, base64url-encoded) for CSRF protection.
3. Build authorization URL with parameters: `code=true`, `client_id`, `response_type=code`, `redirect_uri`, `scope`, `code_challenge`, `code_challenge_method=S256`, `state`.
4. Open the browser to the authorization URL.
5. User authenticates at `claude.ai`.
6. Anthropic redirects to `console.anthropic.com/oauth/code/callback` which **displays the code on screen** (format: `code#state`).
7. **User copies and pastes the code into the terminal** (manual code paste, not a localhost callback).
8. Validate that the `state` matches the expected value (CSRF protection).
9. Exchange code at `TOKEN_URL` with `grant_type=authorization_code`, `client_id`, `redirect_uri`, `code`, `code_verifier`, `state`.
10. Save tokens to `~/.vibemate/auth/claude_auth.json`.

**Token Refresh:** POST to `TOKEN_URL` with `grant_type=refresh_token`, `refresh_token`, `client_id`. Refresh when token is within 5 minutes of expiry.

**Usage Query:** `GET https://api.anthropic.com/api/oauth/usage` with headers `Authorization: Bearer <access_token>` and `anthropic-beta: oauth-2025-04-20`.

Response format:
```json
{
  "five_hour": { "utilization": 6.0, "resets_at": "2025-11-04T04:59:59Z" },
  "seven_day": { "utilization": 35.0, "resets_at": "2025-11-06T03:59:59Z" },
  "seven_day_opus": { "utilization": 0.0, "resets_at": null }
}
```

---

## 8. TUI Dashboard Design

### Layout

```
+============================================+
|  vibemate v0.1.0               [q]uit     |
+============================================+
|  Proxy: http://127.0.0.1:12345/api   [ON] |
+--------------------------------------------+
|                 Quotas                     |
| +-------------------+-------------------+ |
| | Codex (Pro)       | Claude Code (Max) | |
| | 5h:  [####--] 60% | 5h:  [#-----] 6% | |
| | Week: [######] 85% | Week: [###---] 35%| |
| | Resets: 1h 23m    | Resets: 3h 42m    | |
| +-------------------+-------------------+ |
+--------------------------------------------+
|              Request Logs                  |
| TIME     METHOD PATH              PROVIDER |
| 14:32:01 POST   /chat/completions openai   |
| 14:32:05 POST   /messages         anthropic|
| 14:32:08 POST   /responses        openai   |
| 14:32:12 POST   /chat/completions openai   |
|                                     [v] [^]|
+--------------------------------------------+
| q:quit  r:refresh  Tab:focus  j/k:scroll   |
+--------------------------------------------+
```

### Components

1. **Header Bar**: Application name, version, keybinding hints.
2. **Proxy Status**: Listening address, running indicator (green/red).
3. **Quota Panel**: Side-by-side cards for each logged-in agent, with gauge bars for each usage window and reset countdown.
4. **Request Log Panel**: Scrollable table of recent forwarded requests — timestamp, method, path, resolved provider, status code, latency.
5. **Footer**: Keyboard shortcuts.

### Keyboard Shortcuts

| Key | Action |
|---|---|
| `q` | Quit |
| `r` | Refresh usage |
| `j` / Down | Scroll log down |
| `k` / Up | Scroll log up |
| `Tab` | Cycle focus between panels |

### Implementation Details

- App state holds a `VecDeque<RequestLog>` bounded at ~1000 entries.
- The proxy server runs as a background `tokio` task.
- Usage polling runs every 60 seconds in a separate task.
- Request logs flow from the proxy handler to the TUI via `tokio::sync::broadcast` channel.
- Ratatui terminal event loop with 100ms tick rate.

---

## 9. Error Handling & Logging

### Error Handling Principles

| Layer | Strategy |
|---|---|
| **CLI** | Errors printed with `anyhow` context and exit code 1. Messages suggest corrective actions. |
| **Proxy** | Returns appropriate HTTP status codes (502 upstream, 400 bad request, 500 internal). Errors logged via `tracing`. |
| **OAuth** | Token refresh failures suggest re-login. Network errors suggest checking connectivity/proxy. |
| **TUI** | Errors displayed in the status bar, dashboard continues running. |

### Logging Levels (tracing)

| Level | Usage |
|---|---|
| `TRACE` | Raw request/response bodies (only when explicitly enabled) |
| `DEBUG` | Request routing decisions, token refresh events |
| `INFO` | Server start/stop, successful logins, request summaries |
| `WARN` | Token nearing expiry, upstream slow responses |
| `ERROR` | Failed requests, OAuth errors, config issues |

---

## 10. Dependencies

```toml
[dependencies]
# CLI
clap = { version = "4.5", features = ["derive"] }

# Async runtime
tokio = { version = "1", features = ["full"] }

# HTTP server
axum = { version = "0.8", features = ["macros"] }
tower-http = { version = "0.6", features = ["trace", "cors"] }

# HTTP client
reqwest = { version = "0.12", default-features = false, features = [
    "rustls-tls", "json", "stream", "socks"
] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# TUI
ratatui = "0.29"
crossterm = "0.28"

# OAuth / crypto
sha2 = "0.10"
base64 = "0.22"
rand = "0.8"
url = "2"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Error handling
thiserror = "2"
anyhow = "1"

# Async utilities
async-stream = "0.3"
futures = "0.3"
tokio-util = { version = "0.7", features = ["io"] }

# Misc
dirs = "6"
chrono = { version = "0.4", features = ["serde"] }
open = "5"
```

### Rationale

| Concern | Crate | Rationale |
|---|---|---|
| CLI | `clap` (derive) | De facto standard, derive macros reduce boilerplate |
| HTTP server | `axum` | First-party tokio ecosystem, ergonomic, well-maintained |
| HTTP client | `reqwest` | Most popular Rust HTTP client, supports streaming and SOCKS5 |
| TUI | `ratatui` + `crossterm` | Active successor to tui-rs, cross-platform terminal rendering |
| Config | `toml` + `serde` | TOML is human-friendly; serde makes (de)serialization trivial |
| Async | `tokio` | Dominant async runtime, required by axum and reqwest |
| Logging | `tracing` | Structured, async-aware, de facto standard for axum apps |
| Error | `thiserror` + `anyhow` | thiserror for typed enums, anyhow for CLI-level context |
| OAuth crypto | `sha2` + `base64` + `rand` | Lightweight PKCE implementation; avoids the heavy `oauth2` crate |
| Browser | `open` | Cross-platform browser opening, minimal dependency |

### Notable Omission

The `oauth2` crate is **intentionally omitted**. Both Codex and Claude Code use straightforward PKCE flows with well-known endpoints. Implementing PKCE directly with `sha2` + `base64` + `rand` is simpler and avoids the crate's complexity and transitive dependencies.

---

## 11. Implementation Phases

### Phase 1 — Skeleton

- Set up `Cargo.toml` with all dependencies.
- Implement `main.rs` with clap CLI parsing.
- Implement `config/` module with TOML loading and defaults.
- Implement `error.rs`.

### Phase 2 — OAuth

- Implement `oauth/pkce.rs` (PKCE challenge/verifier generation).
- Implement `oauth/token.rs` (token storage and refresh logic).
- Implement `oauth/callback.rs` (local HTTP server for Codex OAuth callback).
- Implement `oauth/codex.rs` (Codex login + usage query).
- Implement `oauth/claude.rs` (Claude Code login + usage query).
- Implement `cli/login.rs` and `cli/usage.rs`.

### Phase 3 — Proxy Server

- Implement `provider/` module (provider registry, request forwarding).
- Implement `proxy/router.rs` (model routing with wildcard matching).
- Implement `proxy/handler.rs` (request forwarding, body transformation).
- Implement `proxy/stream.rs` (SSE relay for streaming responses).
- Implement `proxy/server.rs` (axum server setup, route registration).
- Implement `proxy/middleware.rs` (logging middleware).
- Implement `cli/proxy.rs`.

### Phase 4 — TUI Dashboard

- Implement `tui/app.rs` (application state management).
- Implement `tui/ui.rs` (layout rendering with ratatui).
- Implement `tui/widgets/` (status, usage gauges, request log table).
- Implement `cli/dashboard.rs` (compose proxy + TUI event loop).

### Phase 5 — Polish

- Network proxy support testing (HTTP/HTTPS/SOCKS5).
- Error messages and edge case handling.
- README documentation.
