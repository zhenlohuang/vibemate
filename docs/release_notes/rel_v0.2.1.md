## v0.2.1

### Highlights
- Fixed Dashboard footer visibility and added regression tests.
- Improved release build versioning so CLI/Homebrew versions match Git tags.
- Polished release documentation links.

### Changes
- fix(dashboard): adjust TUI layout to keep footer content visible and add footer rendering tests.
- chore: set `VIBEMATE_VERSION` from release tags via `build.rs` and CI environment.
- chore: ensure Homebrew formula includes the release version during publish workflow.
- docs: fix Full Changelog link in `v0.2.0` release notes.

**Full Changelog**: [v0.2.0...v0.2.1](https://github.com/zhenlohuang/vibemate/compare/v0.2.0...v0.2.1)
