# Model and Provider Configuration Guide

Rai supports multiple AI providers. This guide explains how to configure providers and models.

## Supported Providers

Currently, the following providers are supported:

- **Poe** (Phase 3 Integration)
- **OpenAI** (Planned)
- **Anthropic** (Planned)
- **Google** (Planned)

## Configuration

You can configure the provider and default model using the interactive config command:

```bash
rai config
```

This will prompt you for:
1. **AI Provider**: e.g., `poe`, `openai`.
2. **API Key**: Securely stored in your system keyring.
3. **Default Model**: e.g., `gpt-4o`, `claude-3-opus`.

You can also configure providers manually in `~/.config/rai/config.toml`:

```toml
providers = ["poe", "openai"]
default_provider = "poe"
default_model = "gpt-4o"
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

1. **Default Model**: Set via `rai config` or in `~/.config/rai/config.toml`.
2. **Command Line Override**: Use the `--model` (or `-m`) flag.

```bash
# Use default model
rai run "Explain quantum computing"

# Override model
rai run --model claude-3-opus "Explain quantum computing"
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
