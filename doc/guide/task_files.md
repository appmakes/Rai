# Task Files

Task files are markdown documents that define reusable AI prompts. They support frontmatter for model configuration, variables for dynamic inputs, and subtasks for organizing related prompts.

## Running tasks

### Direct prompts

Run a one-off prompt from the command line:

```bash
rai "explain quantum computing in one sentence"
```

### File-based tasks

For complex or reusable workflows, write your prompt in a markdown file:

```bash
rai review.md
```

## Task file syntax

Rai uses standard Markdown with a few conventions:

- **Frontmatter** — YAML block at the top for settings.
- **Variables** — `{{ name }}` syntax for dynamic inputs.
- **Headers** — `# Title` defines the main task; `## Subtask` defines subtasks.

### Example

```markdown
---
model: gpt-4o
temperature: 0.7
args:
  - file
---

# Code Review
Review the following code file: {{ file }}

## Security
Analyze {{ file }} specifically for security vulnerabilities.
```

## Variables

Declare variables in the `args` frontmatter field. Pass them as positional or named arguments.

### Positional arguments

```bash
rai review.md src/main.rs
```

Variables are filled in declaration order.

### Named arguments

```bash
rai review.md --file src/main.rs
```

Named flags support both `--name value` and `--name=value` forms.

### Optional variables

Suffix a variable name with `?` to make it optional:

```yaml
args:
  - input
  - output
  - format?
```

- Required args (no `?`) must be provided.
- Optional args (`?`) default to empty when omitted.

### Full example

```markdown
---
model: gpt-4o
args:
  - target
  - language
  - focus?
---

Review {{ target }} written in {{ language }}.
Focus on {{ focus }} if specified.
```

```bash
# Positional
rai review.md src/main.rs rust

# Named
rai review.md --target src/ --language rust --focus security
```

## Subtasks

A single task file can contain multiple related prompts under different headers. Run a specific subtask with the `#` syntax:

```bash
rai task.md "#summary"
```

Quote or escape `#` if your shell treats it as a comment character.

## Creating tasks

Use the interactive assistant to scaffold a new task file:

```bash
rai create my_task.md
```

This prompts you for the task description and variables, then generates a template.

## Planning and preview

Inspect a task before running it:

```bash
rai plan task.md
```

This shows:
1. Available subtasks.
2. Prompts for missing arguments.
3. Estimated token cost and final prompt.
4. Confirmation before execution.
