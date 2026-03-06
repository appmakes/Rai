# Providers and Models

Rai supports multiple AI providers out of the box. Switch between them freely — no lock-in.

## Supported providers

### Native implementations
- `openai`
- `anthropic`
- `google` (Gemini API)
- `poe`

### OpenAI-compatible built-ins
These providers use the standard `/chat/completions` endpoint:
- `xai`
- `openrouter`
- `ollama` (local)
- `deepseek`
- `minimax`
- `kimi`
- `zai` (`z.ai` alias supported)
- `bedrock` (requires OpenAI-compatible Bedrock gateway)

### Generic OpenAI-compatible
- `openai-compatible` — fully custom base URL, API key, and model name. Use this for any provider that exposes OpenAI-style endpoints.

## Configuration

### Setup wizard

```bash
rai start
```

Walks you through provider, API key, and default model selection. Config is saved to `~/.config/rai/config.toml`.

### Config hub

```bash
rai config
```

Opens an interactive menu to change provider, model, tools, and profile settings.

### Config file format

```toml
# ~/.config/rai/config.toml
default_profile = "default"
active_profile = "default"
providers = ["openai"]
default_provider = "openai"
default_model = "gpt-4o"
tool_mode = "ask"
no_tools = false
auto_approve = false
```

### Provider selection rules

- If one provider is configured, Rai uses it.
- If multiple providers are configured, `default_provider` is used.
- If `default_provider` is missing or invalid, Rai falls back to the first entry in `providers`.

## Environment variables

Rai detects API keys from standard environment variables, making it easy to use in CI/CD or alongside other tools.

| Provider | Environment Variables |
|----------|----------------------|
| OpenAI | `OPENAI_API_KEY` |
| Anthropic | `ANTHROPIC_API_KEY`, `CLAUDE_API_KEY` |
| Google | `GEMINI_API_KEY`, `GOOGLE_API_KEY` |
| Poe | `POE_API_KEY` |
| xAI | `XAI_API_KEY` |
| OpenRouter | `OPENROUTER_API_KEY` |
| DeepSeek | `DEEPSEEK_API_KEY` |
| MiniMax | `MINIMAX_API_KEY` |
| Kimi | `KIMI_API_KEY`, `MOONSHOT_API_KEY` |
| z.ai | `ZAI_API_KEY` |
| OpenAI-compatible | `RAI_OPENAI_COMPAT_API_KEY`, `OPENAI_COMPAT_API_KEY` |

**Resolution order:** local credential storage (profile-scoped, then provider-level) → environment variable.

## Model selection

Set a default model during setup, or override per run:

```bash
# Use default model
rai "summarize this file"

# Override model for this run
rai --model claude-sonnet-4-20250514 "summarize this file"
```

## OpenAI-compatible generic provider

For any provider that exposes OpenAI-style endpoints:

```toml
providers = ["openai-compatible"]
default_provider = "openai-compatible"
provider_base_url = "https://your-llm.example.com/v1"
default_model = "your-model-name"
```

Set the API key via `rai config` or the `RAI_OPENAI_COMPAT_API_KEY` environment variable.

`provider_base_url` can be either a base URL ending with `/v1` or a full endpoint ending with `/chat/completions`.

## Profiles

Keep separate provider/model configurations with profiles:

```bash
rai profile list
rai profile create heavy-task
rai profile switch heavy-task
rai profile default default
```

Use a profile for a single command:

```bash
rai "review this code" --profile heavy-task
```

Each profile stores its own provider, model, API key, and tool settings in `~/.config/rai/config.<profile>.toml`.
