# Rai User Guide

Welcome to `rai`, your CLI companion for running AI tasks directly from your terminal or CI/CD pipelines.

## 1. Installation

*(Installation instructions will be added here once the release builds are available. For now, build from source using `cargo build --release`)*

## 2. Configuration

Before running tasks, you need to configure your AI provider (e.g., OpenAI, Anthropic).

```bash
rai config
```

This interactive command will guide you through setting up:
- **Provider**: The AI service you want to use.
- **API Key**: Your secret key for authentication.
- **Default Model**: The model to use by default (e.g., `gpt-4o`, `claude-3-opus`).

## 3. Running Tasks

### 3.1 Quick Tasks (Ad-hoc)
Run a simple prompt directly from the command line:

```bash
rai "Explain quantum computing in one sentence"
```

### 3.2 File-based Tasks
For more complex or reusable workflows, define your task in a Markdown file (e.g., `task.md`) and run it:

```bash
rai task.md
```

### 3.3 Sub-tasks
A single `task.md` file can contain multiple related tasks defined by Markdown headers. You can run a specific sub-task using the `#` syntax (ensure to quote it or escape it if your shell treats `#` as a comment):

```bash
rai task.md "#summary"
```

### 3.4 Tasks with Arguments
You can pass arguments to your task. These replace `{{ variable }}` placeholders in your `task.md`.

```bash
rai task.md src/main.rs
```

If your `task.md` has `{{ filename }}`, the above command will inject `src/main.rs` into that position.

## 4. Creating Tasks

Use the interactive assistant to generate a new task file:

```bash
rai create my_new_task.md
```

This will ask you for the task description and intended variables, then generate a template for you.

## 5. Planning & Preview

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

## 6. `task.md` Syntax

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
