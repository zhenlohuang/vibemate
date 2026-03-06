# VibeMate Configuration

## Configuration directory layout
Default configuration directory:

```text
~/.vibemate/
```

Typical structure:

```text
~/.vibemate/
├── config.toml
└── auth/
    ├── codex_auth.json
    └── claude_auth.json
```

What each file is for:
- `config.toml`: main VibeMate config (system, router, providers, agent settings).
- `auth/codex_auth.json`: OAuth token cache for Codex, created after `vibemate login codex`.
- `auth/claude_auth.json`: OAuth token cache for Claude, created after `vibemate login claude`.

## Key fields reference

### `[system]`
- `proxy`: optional outbound HTTP/SOCKS proxy for upstream requests.
  - Examples: `http://127.0.0.1:7890`, `socks5h://127.0.0.1:7890`

### `[router]`
- `host`: local bind host.
- `port`: local bind port.
- `default_provider`: fallback provider when no rule matches.
- `rules`: ordered rules, first match wins.

### `[[router.rules]]`
- `pattern`: model glob pattern (`*` supported).
- `provider`: target provider name.
- `model`: optional rewritten model name.

### `[router.logging]`
- `enabled`: whether to persist router access logs.
- `file_path`: access log file path (JSON Lines). Default: `~/.vibemate/logs/router-access.log`.
- `max_file_size_mb`: rotate when file exceeds this size.
- `max_files`: number of rotated files to retain (`.1`, `.2`, ...).

### `[agents]`
- `show_extra_quota`: show extra quota windows in usage/dashboard.
- `usage_refresh_interval_secs`: usage refresh interval in dashboard.

### `[agents.<agent>]`
- `usage_source`: usage data source selection.
  - `auto`: try the agent's fallback chain.
  - `oauth`: use VibeMate's saved OAuth token only.
  - `web`: use browser/agent-web auth fallback.
  - `cli`: use CLI process fallback when available.
  - `local`: use local session/cache files only.
- `cli_path`: optional absolute path to the agent CLI binary.
- `session_dir`: optional override for the local session/cache directory.
- `cookie_browser`: browser name for cookie-based web fallback. Current values: `chrome`, `firefox`, `safari`. Default: `chrome`.

Example:

```toml
[agents]
show_extra_quota = false
usage_refresh_interval_secs = 300

[agents.codex]
usage_source = "auto"
cli_path = "/opt/homebrew/bin/codex"
session_dir = "~/.codex/sessions"

[agents.claude]
usage_source = "local"
session_dir = "~/.claude/projects"
cookie_browser = "chrome"

[agents.cursor]
usage_source = "web"
cookie_browser = "chrome"

[agents.gemini]
usage_source = "local"
session_dir = "~/.gemini"
```

Behavior notes:
- `codex` auto order: `oauth -> web -> local -> cli`
- `claude` auto order: `oauth -> local -> web -> cli`
- `cursor` auto order: `oauth -> web`
- `gemini` auto order: `oauth -> local`
- `gemini` `local` reads `oauth_creds.json` from the configured session dir (or `~/.gemini/oauth_creds.json`) and then calls the Gemini quota APIs with that credential.
- `local` fallback is best-effort and summarizes recent local activity when a quota API is unavailable.

### `[providers.<name>]`
- `base_url`: upstream API base URL.
- `api_key`: optional API key; VibeMate auto-adds `Authorization: Bearer <api_key>` if no authorization header already exists.
- `headers`: optional custom request headers.

## How to configure routing rules

`router.rules` is matched in order, and the first matching rule is applied.

```toml
[router]
host = "127.0.0.1"
port = 12345
default_provider = "openai"

[[router.rules]]
pattern = "gpt-*"
provider = "openai"
model = "gpt-5-mini"

[[router.rules]]
pattern = "claude-*"
provider = "anthropic"

[[router.rules]]
pattern = "deepseek-*"
provider = "deepseek"
```

Example behavior:
- request model `gpt-mini` -> routed to `openai/gpt-5-mini` (rewritten by `model`).
- request model `claude-sonnet` -> routed to `anthropic/claude-sonnet` (no rewrite).
- request model `llama-3` -> no rule match, fallback to `default_provider`.

## Notes
- Keep `~/.vibemate/auth/*.json` private because they contain OAuth tokens.
- If provider auth fails, verify both the provider section and matching routing rule names.
- Proxy precedence is: environment proxy variables first (`https_proxy`, `all_proxy`, `http_proxy`, and uppercase forms), then `[system].proxy`.
- Dashboard log source behavior:
  - `router.logging.enabled = false`: dashboard router log panel reads embedded in-memory logs only.
  - `router.logging.enabled = true`: dashboard reads router logs from `file_path` (works with external `vibemate router` too).

For troubleshooting steps, see [docs/troubleshooting.md](./troubleshooting.md).
