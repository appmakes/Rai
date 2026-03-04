# Model and Provider Configuration Guide

Rai supports multiple AI providers. This guide explains how to configure providers and models.

## Supported Providers

Rai currently supports:

- **Native provider implementations**
  - `poe`
  - `openai`
  - `anthropic`
  - `google` (Gemini API)
- **OpenAI-compatible built-ins** (using `/chat/completions`)
  - `xai`
  - `openrouter`
  - `ollama` (local)
  - `deepseek`
  - `minimax`
  - `kimi`
  - `zai` (`z.ai` alias supported)
  - `bedrock` (requires OpenAI-compatible Bedrock gateway/base URL)
- **Generic OpenAI-compatible provider**
  - `openai-compatible` (fully custom base URL + key + model)

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
- `~/.config/rai/config.toml` (global settings + default profile settings)
- `~/.config/rai/config.<profile>.toml` (non-default profile settings)

Example:

```toml
# ~/.config/rai/config.toml
default_profile = "default"
active_profile = "default"
providers = ["openai-compatible"]
default_provider = "openai-compatible"
default_model = "gpt-4o"
provider_base_url = "https://api.openai.com/v1"
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

### Provider-Specific Keys
Rai automatically detects keys from standard environment variables used by other tools:

| Provider | Environment Variables |
|----------|----------------------|
| **Poe** | `POE_API_KEY` |
| **OpenAI** | `OPENAI_API_KEY` |
| **Anthropic** | `ANTHROPIC_API_KEY` |
| **Google** | `GEMINI_API_KEY`, `GOOGLE_API_KEY` |
| **xAI** | `XAI_API_KEY` |
| **OpenRouter** | `OPENROUTER_API_KEY` |
| **DeepSeek** | `DEEPSEEK_API_KEY` |
| **MiniMax** | `MINIMAX_API_KEY` |
| **Kimi** | `KIMI_API_KEY`, `MOONSHOT_API_KEY` |
| **z.ai / zai** | `ZAI_API_KEY` |
| **OpenAI-compatible** | `RAI_OPENAI_COMPAT_API_KEY`, `OPENAI_COMPAT_API_KEY` |

API key resolution order:
1. Provider-specific environment variable(s)
2. System keyring (profile-scoped first, then provider-level fallback)

## OpenAI-compatible Generic Provider

Use `openai-compatible` when a provider exposes OpenAI-style endpoints but does not need a dedicated implementation.

Required/typical profile settings:

```toml
providers = ["openai-compatible"]
default_provider = "openai-compatible"
provider_base_url = "https://your-llm.example.com/v1"
default_model = "your-model-name"
```

Then set API key via keyring (`rai config`) or env var (`RAI_OPENAI_COMPAT_API_KEY`/`OPENAI_COMPAT_API_KEY`).

Notes:
- `provider_base_url` may be either:
  - base API URL ending with `/v1`, or
  - full endpoint ending with `/chat/completions`.
- `default_model` is used unless overridden via CLI `--model`.

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
Rai also shows a lightweight ASCII processing spinner while waiting for provider responses on TTY terminals.

Rai uses an internal model status contract and shows a user-facing final state:

- `success`
- `fail`

Failures (including inability responses) are treated as command failure and return a non-zero exit code.
For recoverable tool-based failures after the model has already used tools (for example oversized/noisy fetched pages), Rai now prompts the model to retry with alternate sources before surfacing a final `fail`.

By default, Rai keeps `run` output concise and prints the final answer in your terminal's default text color.

Use `--think` to ask the model for a reasoning trace. Rai requests `<think>...</think>` blocks and renders them as low-contrast info text before the final answer.

Use `--silent` (or `-s`) to disable follow-up prompts when the model needs more input (`state: "proceeding"`). In silent mode, unresolved proceeding responses fail fast with a non-zero exit code.

Common tool-driven prompts:

```bash
rai run "weather in Shanghai"
rai run "search latest Rust 2025 edition updates"
rai run "fetch https://ziglang.org and summarize"
```

Built-in tool discovery is available via `ls_tools`, and nullclaw-compatible tool aliases are included (`file_read`, `file_write`, `file_append`, `file_edit`, `http_request`, `web_fetch`, `web_search`, `git_operations`) so cross-agent tool prompts map cleanly in Rai.

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
