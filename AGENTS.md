# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` is the CLI entrypoint and command dispatcher.
- `src/cli/` contains subcommand handlers: `login`, `usage`, `router`, `dashboard`, and `config`.
- `src/agent/` handles agent capabilities, OAuth/token flows, and provider-specific implementations.
- `src/model_router/` contains the Axum server, routing logic, middleware, handlers, and streaming utilities.
- `src/config/` defines config loading and typed schema; `src/provider/` handles upstream API forwarding; `src/tui/` contains dashboard UI state and widgets.
- `docs/` stores design/config docs. Tests are inline (`#[cfg(test)]`) within source modules; there is no separate `tests/` directory currently.

## Build, Test, and Development Commands
- `cargo run -- --help`: show all CLI commands.
- `cargo run -- dashboard`: launch the TUI dashboard.
- `cargo run -- usage --json`: inspect normalized usage output.
- `cargo build`: debug build.
- `cargo build --release`: optimized release build.
- `cargo test`: run unit tests (CI baseline).
- `cargo fmt` and `cargo clippy --all-targets --all-features`: formatting and lint checks before opening a PR.

## Coding Style & Naming Conventions
- Rust edition is `2024`; follow `rustfmt` defaults (4-space indentation, standard formatting).
- Use `snake_case` for files/modules/functions, `UpperCamelCase` for types/traits/enums, and `SCREAMING_SNAKE_CASE` for constants.
- Keep CLI orchestration in `src/cli/*`; keep shared domain types in `types.rs` within each module when appropriate.
- Prefer `Result`-based error propagation with context-rich error variants.

## Testing Guidelines
- Add focused unit tests next to changed code using `#[cfg(test)]`.
- Name tests by behavior (example: `parse_usage_supports_ratio_fields`).
- Use `#[tokio::test]` for async behavior; keep fixtures minimal and deterministic.
- Run `cargo test` locally before commit; add regression tests for routing, usage parsing, and config normalization changes.

## Commit & Pull Request Guidelines
- Use Conventional Commits for all commit messages.
- Preferred format: `<type>(<scope>): <subject>`.
- Scope is required and should map to the changed module.
- Recommended scopes: `cli`, `dashboard`, `agent`, `router`, `docs`.
- Common types: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `build`, `ci`, `chore`, `revert`.
- Examples: `fix(cli): handle empty usage response`, `feat(agent): add cursor usage capability`.
- Keep commits scoped to one logical change.
- PRs should include: purpose, key changes, test evidence (`cargo test`), and any config/doc updates.
- Link related issues when available; include terminal screenshots only for TUI/dashboard UX changes.

## Security & Configuration Tips
- Runtime config lives at `~/.vibemate/config.toml`.
- Never commit API keys or tokens; use placeholder values in docs/examples.
- When adding provider headers or auth fields, update `docs/configuration.md` in the same PR.
