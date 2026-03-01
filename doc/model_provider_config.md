# Model and Provider Configuration Guide

Rai supports multiple AI providers. This guide explains how to configure providers and models.

## Supported Providers

Currently, the following providers are supported:

- **Poe** (Phase 3 Integration)
- **OpenAI** (Planned)
- **Anthropic** (Planned)
- **Google** (Planned)

## Configuration

For first-time setup, use:

```bash
rai start
```

This guides you through:
1. **AI Provider**: e.g., `poe`, `openai`.
2. **API Key**: Securely stored in your system keyring.
3. **Default Model**: e.g., `gpt-4o`, `claude-3-opus`.

For advanced changes, use the config hub:

```bash
rai config
```

Profile settings are stored per file:
- `~/.config/rai/config.toml` (global default/active profile)
- `~/.config/rai/config.<profile>.toml` (profile settings)

Example:

```toml
# ~/.config/rai/config.toml
default_profile = "default"
active_profile = "default"
```

```toml
# ~/.config/rai/config.default.toml
providers = ["poe", "openai"]
default_provider = "poe"
default_model = "gpt-4o"
tool_mode = "ask"
no_tools = false
auto_approve = false
```

Provider selection rules:
- If exactly one provider is configured, Rai uses that provider.
- If multiple providers are configured, Rai uses `default_provider` when present.
- If `default_provider` is missing/invalid, Rai uses the first provider in `providers`.

## Environment Variables

Rai supports setting API keys via environment variables for automation or to piggyback on existing CLI tools.

### Global Override
- `RAI_API_KEY`: Overrides any provider-specific key.

### Provider-Specific Keys
Rai automatically detects keys from standard environment variables used by other tools:

| Provider | Environment Variables |
|----------|----------------------|
| **Poe** | `POE_API_KEY` |
| **OpenAI** | `OPENAI_API_KEY` |
| **Anthropic** | `ANTHROPIC_API_KEY` |
| **Google** | `GEMINI_API_KEY`, `GOOGLE_API_KEY` |

## Model Selection

You can specify the model to use in two ways:

1. **Default Model**: Set via `rai start`/`rai config` or in `config.<profile>.toml`.
2. **Command Line Override**: Use the `--model` (or `-m`) flag.

```bash
# Use default model
rai run "Explain quantum computing"

# Override model
rai run --model claude-3-opus "Explain quantum computing"
```

## Billing Output

Use `--bill` to print usage stats for the current command:

```bash
rai run "Hello world" --bill
```

The summary includes:
- API calls
- Input tokens
- Output tokens

For detailed runtime logs (tool calls/prompts/provider responses), add `--detail`:

```bash
rai run "weather in Shanghai" --bill --detail
```

Detailed provider exchanges are numbered as `request #N` and `response #N`.

By default, Rai keeps `run` output concise and prints the final answer in your terminal's default text color.

Use `--think` to ask the model for a reasoning trace. Rai requests `<think>...</think>` blocks and renders them as low-contrast info text before the final answer.

Common tool-driven prompts:

```bash
rai run "weather in Shanghai"
rai run "search latest Rust 2025 edition updates"
rai run "fetch https://ziglang.org and summarize"
```

Nullclaw-compatible tool aliases are included (`file_read`, `file_write`, `file_append`, `file_edit`, `http_request`, `web_fetch`, `web_search`, `git_operations`) so cross-agent tool prompts map cleanly in Rai.

Rai does not hardcode task-specific shortcuts (such as weather/whois). General prompts are handled through the provider and tool-calling flow (for example via `web_search` and `web_fetch`).

On Unix, filesystem and git-path operations enforce a hard safety block on system-critical prefixes (for example `/etc`, `/bin`, `/usr/bin`, `/dev`, `/proc`, `/sys`) regardless of other allow rules.

## Profiles

Use profiles to keep separate provider/model setups:

```bash
rai profile list
rai profile create hard-task
rai profile switch hard-task
rai run "Review this code" --profile hard-task
```

## Poe Integration (Phase 3)

To use Poe:

1. Get your API key from [Poe](https://poe.com/api_key).
2. Configure Rai:
   ```bash
   rai config
   # Enter 'poe' as provider
   # Paste your API key
   # Enter a model name (e.g., 'gpt-4o', 'claude-3-5-sonnet')
   ```
   Or set the environment variable:
   ```bash
   export POE_API_KEY=your_key_here
   ```

3. Run a task:
   ```bash
   rai run "Hello world"
   ```
