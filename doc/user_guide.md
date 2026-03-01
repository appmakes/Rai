# Rai User Guide

Welcome to `rai`, your CLI companion for running AI tasks directly from your terminal or CI/CD pipelines.

## 1. Installation

*(Installation instructions will be added here once the release builds are available. For now, build from source using `cargo build --release`)*

## 2. First-time setup

Use `start` for first-time setup:

```bash
rai start
```

`rai start` keeps onboarding minimal:
1. Pick provider.
2. Set API key.
3. Pick default model.
4. Continue (default) or open more settings.

If you choose more settings, Rai opens `rai config`.

## 3. Configuration hub

Use `config` to choose what to configure:

```bash
rai config
```

Config sections include:
- Provider & API key
- Model defaults
- Tools
- Profiles

## 4. Running Tasks

### 4.1 Quick Tasks (Ad-hoc)
Run a simple prompt directly from the command line:

```bash
rai "Explain quantum computing in one sentence"
```

Examples that trigger built-in tools:

```bash
rai run "weather in Shanghai"
rai run "whois google.com"
```

### 4.2 File-based Tasks
For more complex or reusable workflows, define your task in a Markdown file (e.g., `task.md`) and run it:

```bash
rai task.md
```

### 4.3 Sub-tasks
A single `task.md` file can contain multiple related tasks defined by Markdown headers. You can run a specific sub-task using the `#` syntax (ensure to quote it or escape it if your shell treats `#` as a comment):

```bash
rai task.md "#summary"
```

### 4.4 Tasks with Arguments
You can pass arguments to your task. These replace `{{ variable }}` placeholders in your `task.md`.

```bash
rai task.md src/main.rs
```

If your `task.md` has `{{ filename }}`, the above command will inject `src/main.rs` into that position.

### 4.5 Billing Summary (`--bill`)
Use `--bill` to print API usage for the current command:

```bash
rai run "Summarize this" --bill
```

At the end of execution, Rai prints:
- API calls made
- Input tokens used
- Output tokens used

### 4.6 Clean vs Detailed Output (`--log`)
By default, `rai run` keeps output short and highlights the final answer for faster scanning.

Use `--log` when you want detailed runtime info (tool calls, provider notices):

```bash
rai run "weather in Shanghai" --log
```

Tip for local dev: when using Cargo, `cargo run` itself prints build/run lines. Use quiet mode for cleaner output:

```bash
cargo run -q -- run "weather in Shanghai" --bill
```

## 5. Profiles

Rai supports multiple profiles.

Examples:

```bash
rai profile list
rai profile create hard-task
rai profile switch hard-task
rai profile default default
```

Use a profile for one command:

```bash
rai run "Summarize this file" --profile hard-task
```

## 6. Creating Tasks

Use the interactive assistant to generate a new task file:

```bash
rai create my_new_task.md
```

This will ask you for the task description and intended variables, then generate a template for you.

## 7. Planning & Preview

Unsure what a task file does? Use the `plan` command to inspect it before execution:

```bash
rai plan task.md
```

This will:
1. Show you the available sub-tasks.
2. Allow you to select which one to run.
3. Prompt you for any missing arguments.
4. Display the estimated cost (tokens) and final prompt.
5. Ask for confirmation before running.

## 8. `task.md` Syntax

`rai` uses standard Markdown with a few special conventions:

- **Frontmatter**: YAML block at the top for settings.
- **Variables**: `{{ name }}` syntax for dynamic inputs.
- **Headers**: `# Title` defines the main task, `## Subtask` defines sub-tasks.

**Example:**

```markdown
---
model: gpt-4o
temperature: 0.7
---

# Code Review
Review the following code file: {{ file }}

## Security
Analyze {{ file }} specifically for security vulnerabilities.
```
