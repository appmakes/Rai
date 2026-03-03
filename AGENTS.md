# AGENTS.md

## Cursor Cloud specific instructions

**Rai** is a Rust CLI tool for running AI tasks from the terminal. Single binary, no microservices or databases.

### Build / Test / Lint

- `cargo build` — debug build
- `cargo test` — run all unit + integration tests (29 tests total)
- `cargo clippy` — lint (should pass clean)
- `cargo build --release` — optimized build

### Running the CLI

The binary is at `target/debug/rai` (debug) or `target/release/rai` (release).

Key commands:
- `rai start` — first-time setup wizard
- `rai run "prompt"` — ad-hoc AI query
- `rai run task.md` — file-based task execution
- `rai run task.md --subtask security arg1` — sub-task with arguments
- `rai plan task.md` — preview task structure and variables
- `rai create output.md` — interactive task file wizard
- `rai config` — configuration hub (provider/tools/model/profile sections)
- `rai profile list` — list and manage profiles

### Configuration

- Global config file: `~/.config/rai/config.toml` (e.g., `default_profile`, `active_profile`)
- Profile config files: `~/.config/rai/config.<profile>.toml` (e.g., `providers`, `default_provider`, `default_model`, tool defaults)
- API keys: default store is `~/.local/share/rai/credentials` (mode 0600). Use `--keyring` to use OS keyring. Resolution: credentials store (file or keyring) then provider env var (e.g. `POE_API_KEY`).
- Only the `poe` provider is implemented; set `providers = ["poe"]` and `default_provider = "poe"` in config
- The `keyring` crate (used only with `--keyring`) requires `libdbus-1-dev` on Linux
- CI/CD mode (detected via `CI` env var or non-TTY stdin) disables interactive prompts

### System dependencies

- `libdbus-1-dev` and `pkg-config` are required for the `keyring` crate to compile

### Workflow preference

- When implementing code modifications or new features, always update related documentation in the same change.
- After making code changes, ensure `cargo run` and `cargo build` complete without warnings.
