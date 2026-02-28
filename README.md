# Vibemate - Your Vibe Coding Mate

Vibemate is a local CLI companion for coding agents, with OAuth login, usage tracking, and a unified model router.

## Features
1. Agent OAuth login
2. Usage and quota query
3. Model router with model-based routing

## Supported Agents
| Name | Status |
| --- | --- |
| Codex | ✅ |
| Claude Code | ✅ |

## Installation
### 1. Download binary
Download the latest binaries from [GitHub Releases](https://github.com/zhenlohuang/vibemate/releases)

Example (macOS / Linux):

```bash
chmod +x vibemate
mv vibemate /usr/local/bin/vibemate
```

### 2. Build from source
Requires Rust (stable).

```bash
git clone https://github.com/zhenlohuang/vibemate.git
cd vibemate
cargo build --release
./target/release/vibemate --help
```

## Quick Start
Run the dashboard:

```bash
vibemate dashboard
```

Then configure your providers and routing in `~/.vibemate/config.toml`.
For full configuration details, see [docs/configuration.md](./docs/configuration.md).

## Commands
```text
A CLI for logging into supported agents, checking quota usage, running a local model router, and viewing a terminal dashboard.

Usage: vibemate [OPTIONS] <COMMAND>

Commands:
  login      Authenticate with an agent provider
  usage      Show usage and quota information
  router     Run the local model router server
  dashboard  Launch the interactive terminal dashboard
  config     Inspect or initialize the config file
  help       Print this message or the help of the given subcommand(s)

Options:
      --config <PATH>
          Path to the config file

          [default: ~/.vibemate/config.toml]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

## Configuration
Vibemate initializes an empty config file by default at:

```text
~/.vibemate/config.toml
```

## Minimal working example

```toml
[server]
host = "127.0.0.1"
port = 12345

[agents]
show_extra_quota = false
usage_refresh_interval_secs = 300

[providers.openai-official]
base_url = "https://api.openai.com/v1"
api_key = "sk-your-openai-api-key"

[routing]
default_provider = "openai-official"
rules = []
```

## Multi-provider routing example
```toml
[providers.openai-official]
base_url = "https://api.openai.com/v1"
api_key = "sk-your-openai-api-key"

[providers.openrouter]
base_url = "https://openrouter.ai/api/v1"
api_key = "sk-or-v1-your-openrouter-key"
headers = { "HTTP-Referer" = "https://example.com", "X-Title" = "Vibemate" }

[routing]
default_provider = "openai-official"
rules = [
  { pattern = "claude-*", provider = "openrouter" },
  { pattern = "o1-mini", provider = "openrouter", model = "openai/o1-mini" }
]
```

Detailed configuration guide: [docs/configuration.md](./docs/configuration.md)

## License
MIT License. See [LICENSE](./LICENSE).
