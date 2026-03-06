<p align="center">
  <img src="icon.svg" width="80" alt="rai icon">
</p>

<h1 align="center">rai</h1>

<p align="center">
  Run AI instructions directly from your terminal, scripts, and CI/CD pipelines.
</p>

<p align="center">
  <a href="https://appmakes.github.io/Rai/">Website</a> &nbsp;&bull;&nbsp;
  <a href="https://appmakes.github.io/Rai/docs.html">Documentation</a> &nbsp;&bull;&nbsp;
  <a href="https://github.com/appmakes/Rai/releases">Releases</a>
</p>

---

## Install

**cargo:**

```bash
cargo install rai-cli
```

**curl:**

```bash
curl -sSL https://appmakes.github.io/Rai/install.sh | sh
```

**From source:**

```bash
cargo install --path .
```

## Quick start

**1. First-time setup:**

```bash
rai start
```

Pick a provider, enter your API key, and choose a default model.

**2. Run a prompt:**

```bash
rai "whois github.com"
```

**3. Pipe input:**

```bash
ls -a | rai "count all file size"
```

**4. Run a task file:**

```bash
rai run task.md
```

**5. Auto-approve tool calls:**

```bash
rai --yes "Clean up feature flags.md"
```

## Usage

```
rai [OPTIONS] <PROMPT|FILE>
```

| Command | Description |
|---------|-------------|
| `rai "prompt"` | Run an ad-hoc prompt |
| `rai run task.md` | Run a task file |
| `rai plan task.md` | Preview task structure before execution |
| `rai create task.md` | Create a task file interactively |
| `rai start` | First-time setup wizard |
| `rai config` | Open configuration menu |
| `rai profile list` | List configuration profiles |

| Flag | Description |
|------|-------------|
| `-y, --yes` | Auto-approve all tool calls |
| `-m, --model <MODEL>` | Override AI model (e.g. `gpt-4o`, `kimi-k2`) |
| `--profile <NAME>` | Select configuration profile |
| `-s, --silent` | No follow-up input |
| `--no-tools` | Disable tool calling |
| `--bill` | Print API and token usage summary |
| `--detail` | Show detailed runtime logs |
| `--think` | Ask provider to show thinking chain |
| `-v, --verbose` | Debug logging |

## Configuration

Config files live in `~/.config/rai/`:

| File | Purpose |
|------|---------|
| `config.toml` | Default profile config |
| `config.<profile>.toml` | Named profile config |

API keys are stored in `~/.local/share/rai/credentials` (mode 0600). Use `--keyring` to store in the OS keyring instead.

**Supported providers:** OpenAI, Anthropic, Google, Poe, xAI, OpenRouter, Ollama, DeepSeek, MiniMax, Kimi, ZAI, Bedrock, and any OpenAI-compatible endpoint.

## Documentation

- [Quick Start](doc/guide/quick_start.md) - installation and first run
- [Task Files](doc/guide/task_files.md) - markdown runbooks, variables, and subtasks
- [Tools & Permissions](doc/guide/tools_and_permissions.md) - built-in tools and permission system
- [Providers & Models](doc/guide/providers_and_models.md) - provider setup and model selection
- [Advanced Flags](doc/guide/advanced_flags.md) - CLI flags and options

### Development docs

- [Architecture](doc/development/architecture.md)
- [Agent Loop Design](doc/development/agent_loop_design.md)

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Lint
cargo clippy
```

> **Note:** On Linux, if using `--keyring`, install: `libdbus-1-dev`, `pkg-config`

## License

See [LICENSE](LICENSE) for details.
