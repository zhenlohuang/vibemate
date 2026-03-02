# Troubleshooting

## `vibemate login codex` hangs after browser OAuth success
- Re-run with logs:
  - `RUST_LOG=info vibemate login codex`
- If terminal reaches `Exchanging token...` and then times out, verify proxy connectivity:
  - `curl -I --max-time 5 -x http://127.0.0.1:7890 https://auth.openai.com/oauth/token`
- If you do not need a proxy, remove/disable all of these and retry:
  - `https_proxy` / `all_proxy` / `http_proxy` (and uppercase variants)
  - `[system].proxy` in `~/.vibemate/config.toml`
