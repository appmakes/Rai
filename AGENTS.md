# AGENTS.md

## Cursor Cloud specific instructions

**Rai** is a Rust CLI tool for running AI tasks from the terminal. Single binary, no microservices or databases.

### Build / Test / Lint

- `cargo build` — debug build
- `cargo test` — run tests (no tests exist yet)
- `cargo clippy` — lint (there is a pre-existing `needless_borrow` warning in `src/main.rs:152`)
- `cargo build --release` — optimized build

### Running the CLI

The binary is at `target/debug/rai` (debug) or `target/release/rai` (release).

**Important:** The `rai run` subcommand panics in debug builds due to a clap debug assertion about conflicting positional arguments (`task` and `args`). Use the **release** binary (`cargo build --release`) to test `rai run`.

### Configuration

- Config file: `~/.config/rai/config.toml` (fields: `provider`, `default_model`)
- API key resolution order: `RAI_API_KEY` env var → provider-specific env var (e.g. `POE_API_KEY`) → OS keyring
- Only the `poe` provider is implemented; set `provider = "poe"` in config
- The `keyring` crate requires `libdbus-1-dev` on Linux (system dependency)

### System dependencies

- `libdbus-1-dev` and `pkg-config` are required for the `keyring` crate to compile
