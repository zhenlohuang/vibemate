## v0.2.3

### Highlights
- Fixed `vibemate login codex` hanging after browser OAuth callback.
- Added timeout and clearer diagnostics for Codex token exchange network/proxy failures.
- Added a dedicated troubleshooting doc and aligned default proxy behavior with optional proxy usage.

### Changes
- fix(agent): add callback/token-exchange timeout handling and step-by-step login progress output for Codex OAuth.
- fix(agent): improve callback server shutdown behavior to avoid post-callback hangs.
- fix: default `[system].proxy` to `None` unless explicitly configured.
- docs: add `docs/troubleshooting.md` and link troubleshooting guidance from README/config docs.

**Full Changelog**: [v0.2.2...v0.2.3](https://github.com/zhenlohuang/vibemate/compare/v0.2.2...v0.2.3)
