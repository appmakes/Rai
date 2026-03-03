# rai

`rai` is a Rust CLI tool for running AI tasks from the terminal or CI.

## Prerequisites

- Rust toolchain (stable)
- Linux packages for keyring support:
  - `libdbus-1-dev`
  - `pkg-config`

## Build

- Debug build: `cargo build`
- Release build: `cargo build --release`

## Run

After building, run the binary from:

- `cargo run -- --help`
- `target/debug/rai` (debug)
- `target/release/rai` (release)

Common commands:

- `rai start` — first-time setup wizard
- `rai run "Summarize this text"` — run an ad-hoc prompt
- `rai run task.md` — run a task file
- `rai run task.md --input foo --output bar` — run task file with named task args
- `rai plan task.md` — preview task structure before execution
(optional: `--subtask name`, trailing args)
- `rai create task.md` — create a task file interactively
- `rai config` — open configuration menu
- `rai profile list` — list profiles

Flags (global unless noted):

- `-v, --verbose` — debugging (repeat to increase level)
- `-m, --model <MODEL>` — override AI model (e.g. `gpt-4o`, `kimi-k2`)
- `--profile <NAME>` — select configuration profile
- `-y, --yes` — auto-approve all tool calls
- `--no-tools` — disable tool calling (single-turn only)
- `-s, --silent` — do not ask for follow-up input
- `--bill` — print API and token usage summary
- `--detail` — show detailed runtime logs (tool calls, prompts, responses)
- `--think` — ask provider to show thinking chain

## Test and lint

- Run tests: `cargo test`
- Run lints: `cargo clippy`

## Configuration notes

- Global config: `~/.config/rai/config.toml`
- Default profile config: `~/.config/rai/config.toml`
- Non-default profile config: `~/.config/rai/config.<profile>.toml`
- If no profile is explicitly selected, `rai` falls back to `default` and auto-creates it when missing
- Supported providers:
  - Native: `poe`, `anthropic`, `google`
  - OpenAI-compatible built-ins: `openai`, `xai`, `openrouter`, `ollama`, `deepseek`, `minimax`, `kimi`, `zai`, `bedrock`
  - Generic OpenAI-compatible: `openai-compatible` (configure `provider_base_url`)
- `provider_base_url` can override endpoint base URL per profile (required for `openai-compatible`, optional for OpenAI-compatible built-ins)
- API key lookup order:
  1. `RAI_API_KEY`
  2. provider env var (e.g. `POE_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`); a `.env` file in the current directory is loaded automatically
  3. OS keyring (recommended)
