# Advanced Flags

Complete reference for all `rai` CLI flags and runtime options.

## Global flags

These flags work with any command or when running tasks directly.

| Flag | Short | Description |
|------|-------|-------------|
| `--model <name>` | `-m` | Override the AI model for this run |
| `--yes` | `-y` | Auto-approve all tool calls (global blocklist still enforced) |
| `--silent` | `-s` | Disable follow-up input prompts |
| `--no-tools` | | Disable tool calling (single-turn mode) |
| `--bill` | | Print token and cost summary after execution |
| `--detail` | | Show detailed runtime logs (tool calls, prompts, responses) |
| `--think` | | Ask the model to include a reasoning chain |
| `--profile <name>` | | Use a named configuration profile |
| `--keyring` | | Use OS keyring for API key storage |
| `--verbose` | `-v` | Increase debug verbosity (use `-vv` for more) |

## Flag details

### `--model` / `-m`

Override the default model for a single run:

```bash
rai -m claude-sonnet-4-20250514 "explain this error"
rai --model kimi-k2.5 review.md
```

### `--yes` / `-y`

Auto-approve all tool calls without prompting. The hardcoded safety blocklist (e.g., `rm -rf /`, `shutdown`) is still enforced.

```bash
rai -y "clean up temp files in ./build"
```

### `--silent` / `-s`

Suppress follow-up prompts when the model needs more input. In silent mode, unresolved `proceeding` responses fail immediately with a non-zero exit code. Useful for CI/CD and scripts.

```bash
rai -s "summarize README.md"
```

### `--no-tools`

Run in single-turn mode with no tool access. The model can only respond with text — no file operations, web access, or shell commands.

```bash
rai --no-tools "what is the capital of France"
```

### `--bill`

Print API usage after execution:

```bash
rai "summarize this" --bill
```

Output includes API calls made, input tokens, and output tokens.

### `--detail`

Show detailed runtime logs including tool calls, prompts sent to the provider, and raw responses. Provider exchanges are numbered as `request #N` / `response #N`.

```bash
rai "weather in Shanghai" --detail
```

Combine with `--bill` for full diagnostics:

```bash
rai "weather in Shanghai" --bill --detail
```

### `--think`

Request a reasoning chain from the model. Rai instructs the model to wrap reasoning in `<think>...</think>` tags, then displays them as low-contrast info lines before the final answer.

Works with all providers and models (prompt-based, not API-specific).

```bash
rai "compare rust and go for backend APIs" --think
```

### `--profile`

Use a specific configuration profile for this run:

```bash
rai "review this code" --profile heavy-task
```

### `--keyring`

Store and retrieve API keys from the OS keyring instead of the default credentials file (`~/.local/share/rai/credentials`).

```bash
rai start --keyring
```

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| Non-zero | Failure (including inability responses from the model) |

Rai gives the model a bounded retry budget to recover from tool-driven failures (e.g., oversized web pages) before surfacing a final failure.

## Output behavior

- By default, Rai prints only the final answer in your terminal's default text color.
- A small ASCII spinner (`[rai] processing`) shows while waiting for provider responses on TTY terminals.
- `--detail` enables verbose output with numbered request/response pairs.
- `--think` adds gray-styled reasoning blocks before the answer.
- `--silent` suppresses interactive follow-up prompts.

## Custom variables

Pass custom `--<variable> <value>` pairs to task files. These fill `{{ variable }}` placeholders in the task markdown:

```bash
rai review.md --target src/main.rs --language rust --focus security
```

See [Task files](task_files.md) for full variable syntax.
