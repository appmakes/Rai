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

- `target/debug/rai` (debug)
- `target/release/rai` (release)

Common commands:

- `rai start` — first-time setup wizard
- `rai run "Summarize this text"` — run an ad-hoc prompt
- `rai run task.md` — run a task file
- `rai run task.md --input foo --output bar` — run task file with named task args
- `rai plan task.md` — preview task structure before execution
- `rai create task.md` — create a task file interactively
- `rai config` — open configuration menu
- `rai profile list` — list profiles

## Test and lint

- Run tests: `cargo test`
- Run lints: `cargo clippy`

## Configuration notes

- Global config: `~/.config/rai/config.toml`
- Profile config: `~/.config/rai/config.<profile>.toml`
- Supported provider today: `poe`
- API key lookup order:
  1. `RAI_API_KEY`
  2. provider env var (for example `POE_API_KEY`)
  3. OS keyring
