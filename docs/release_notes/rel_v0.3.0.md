## v0.3.0

### Highlights
- Added Gemini agent support for login, token refresh, and usage/quota querying.
- Upgraded dashboard usage cards with keyboard/mouse selection and per-card scrolling.
- Improved quota visibility so windows without `resets_at` are still shown when useful.

### Changes
- feat(agent): add Gemini agent implementation and register `gemini` in the agent registry.
- refactor(agent): rename Claude agent ID/display from `claude-code`/`Claude Code` to `claude`/`Claude`.
- fix(agent): keep Codex/Claude quota windows visible when either utilization or reset time is available.
- feat(dashboard): add usage-card focus, scroll controls (`Enter`, `Tab`, `Esc`, arrows, `PageUp/PageDown`, mouse wheel/click).
- feat(tui): add usage render metadata and per-card scroll truncation hints.
- docs: update configuration routing examples and Claude agent docs for the new naming.

**Full Changelog**: [v0.2.3...v0.3.0](https://github.com/zhenlohuang/vibemate/compare/v0.2.3...v0.3.0)
