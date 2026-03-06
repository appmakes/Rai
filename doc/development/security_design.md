# Security & Configuration Design

## Secret Management

To ensure security, `rai` will **never** store API keys or secrets in plain text configuration files. We will use a tiered approach for managing credentials:

### 1. Environment Variables (Highest Priority)
Ideal for CI/CD pipelines and temporary overrides.
- **Provider-Specific Discovery**: `rai` automatically checks standard environment variables used by other tools.
    - **OpenAI**: `OPENAI_API_KEY`
    - **Anthropic (Claude)**: `ANTHROPIC_API_KEY`, `CLAUDE_API_KEY`
    - **Gemini (Google)**: `GEMINI_API_KEY`, `GOOGLE_API_KEY`
    - **Poe**: `POE_API_KEY`

### 2. Local Credential Storage (Recommended for Local Use)
We use secure local credential storage via the `keyring` crate. Depending on platform/backend, this may be OS-native keyring services (macOS Keychain, Windows Credential Manager, Linux Secret Service) or a credential-file backend.
- **Service**: `rai`
- **Username**: `<provider_name>` (e.g., `openai`, `anthropic`)
- **Action**: The `rai config` command will prompt for the key and save it securely here.

### 3. Configuration File (Non-Sensitive Data)
Stored in `~/.config/rai/config.toml`.
Contains **only** safe metadata:
- `provider`: Active provider name (e.g., "openai").
- `default_model`: Default model to use (e.g., "gpt-4o").

## Data Flow

1. **Load Config**: Read `config.toml` to get the preferred `provider`.
2. **Resolve API Key**:
   1. Check provider-specific standard env vars (e.g., `ANTHROPIC_API_KEY` or `CLAUDE_API_KEY` for Claude).
   2. If specific env var is missing, try to fetch from the local **Credential Store** (stored by `rai`).
   3. If all fail, prompt the user (interactive mode) or error out.

## Implementation Plan

1.  **Dependencies**: Add `keyring` crate.
2.  **Config Refactor**: Remove `api_key` field from `Config` struct serialization.
3.  **KeyStore Module**: Create a helper to interface with the `keyring` crate.
4.  **Integration**: Update `rai config` to save to keyring, and `rai run` to read from it.
