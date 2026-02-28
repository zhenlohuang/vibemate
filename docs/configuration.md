# Vibemate Configuration

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
- `config.toml`: main Vibemate config (server, providers, routing, agent settings).
- `auth/codex_auth.json`: OAuth token cache for Codex, created after `vibemate login codex`.
- `auth/claude_auth.json`: OAuth token cache for Claude Code, created after `vibemate login claude-code`.

## Key fields reference
`[server]`
- `host`: local bind host.
- `port`: local bind port.
- `proxy`: optional outbound HTTP/SOCKS proxy for upstream requests.

`[agents]`
- `show_extra_quota`: show extra quota windows in usage/dashboard.
- `usage_refresh_interval_secs`: usage refresh interval in dashboard.

`[providers.<name>]`
- `base_url`: upstream API base URL.
- `api_key`: optional API key; Vibemate auto-adds `Authorization: Bearer <api_key>` if no authorization header already exists.
- `headers`: optional custom request headers.

`[routing]`
- `default_provider`: fallback provider when no rule matches.
- `rules`: ordered rules, first match wins.
- `pattern`: model glob pattern (`*` supported).
- `provider`: target provider name.
- `model`: optional rewritten model name.

## Notes
- Keep `~/.vibemate/auth/*.json` private because they contain OAuth tokens.
- If provider auth fails, verify both the provider section and matching routing rule names.
