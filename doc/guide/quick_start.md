# Quick Start

Get up and running with `rai` in under a minute.

## Install

Via cargo:

```bash
cargo install rai-cli
```

Or via the install script:

```bash
curl -sSL https://appmakes.github.io/Rai/install.sh | sh
```

## First-time setup

Run the setup wizard to pick your AI provider, enter your API key, and choose a default model:

```bash
rai start
```

The wizard walks you through:
1. Select provider (e.g., OpenAI, Anthropic, Google, Ollama).
2. Enter API key.
3. Choose default model (e.g., `gpt-4o`, `claude-sonnet-4-20250514`).
4. Optionally open advanced settings.

## Run your first task

Run a prompt directly:

```bash
rai "explain this error"
```

Pipe content in:

```bash
ls -la | rai "count all file sizes"
```

Run a task file:

```bash
rai review.md
```

## What's next

- [Configure providers and models](providers_and_models.md) — set up multiple providers, profiles, and environment variables.
- [Tools and permissions](tools_and_permissions.md) — understand built-in tools and control what the AI can do.
- [Task files](task_files.md) — write reusable markdown tasks with variables and subtasks.
- [Advanced flags](advanced_flags.md) — detailed reference for all CLI flags.
