# Product Requirements Document: Rai (Run AI)

## 1. Overview
`rai` is a command-line interface (CLI) tool designed to run AI tasks directly in the terminal or within CI/CD pipelines. It bridges the gap between natural language commands and structured task execution using Markdown files (`task.md`).

## 2. Core Features

### 2.1 Configuration Management
- **Command:** `rai config`
- **Functionality:** 
  - Configure AI model providers (OpenAI, Anthropic, Google Gemini, etc.).
  - Set API keys securely.
  - Define default models and parameters (temperature, max tokens).
  - Manage global settings (e.g., default output directory).

### 2.2 Task Execution
- **Command:** `rai run <task>` (or simply `rai <task>`)
- **Modes:**
  1. **Ad-hoc Execution:**
     - `rai "count all file numbers under src folder"`
     - Executes a single prompt directly.
  2. **File-based Execution:**
     - `rai task.md`
     - Reads instructions from `task.md` and executes the main task described therein.
  3. **Sub-task Execution:**
     - `rai task.md "#sub-task"`
     - Executes a specific section (header) within the `task.md` file.
  4. **Parameterized Execution:**
     - `rai task.md <...arguments>`
     - Supports passing arguments to the task (e.g., `rai summary.md report.pdf`).
     - Arguments should replace placeholders in the task prompt (e.g., `{{ 1 }}`).

### 2.3 Task Creation Assistant
- **Command:** `rai create <task.md>`
- **Functionality:**
  - Interactive CLI wizard or chat interface.
  - Guides the user to define the task description, inputs, and expected outputs.
  - Generates a valid `task.md` template.

### 2.4 Task Planning & Preview
- **Command:** `rai plan <task.md>`
- **Functionality:**
  - Parses the `task.md` and identifies all available sub-tasks and parameters.
  - Provides an interactive selection menu (using arrow keys/checkboxes) to choose which parts to run.
  - Prompts for missing required parameters.
  - Displays a summary of the execution plan (what prompts will be sent, estimated cost/tokens if possible).
  - Offers `Execute` or `Cancel` options.

## 3. `task.md` Specification
The `task.md` file serves as the definition for AI workflows.

### Structure
- **Frontmatter (Optional):** YAML block for metadata (model override, temperature) and explicit argument list.
- **H1 (`# Task Name`):** The main entry point description.
- **H2 (`## Sub-task Name`):** distinct sub-tasks addressable via `#`.
- **Sub-task Frontmatter (Optional):** YAML block immediately following a sub-task header to define sub-task specific metadata or arguments.
- **Variables:** Use `{{ variable_name }}` or `{{ 1 }}` syntax for arguments.

**Example:**
```markdown
---
model: gpt-4o
args:
  - filename
---

# Code Review
Review the provided code file for bugs and security issues in {{ filename }}.

## security
---
args:
  - risk_level
---
Focus only on security vulnerabilities in {{ filename }} with risk level {{ risk_level }}.
```

## 4. User Interface
- **CLI Flags:**
  - `--verbose` / `-v`: Enable verbose logging.
  - `--model <name>` / `-m <name>`: Override the AI model (e.g., `--model kimi-k2`).
- **Output:** clear, colored terminal output (using ANSI codes).
- **Interactive:** Use libraries like `dialoguer` or `inquire` for prompts and selections.
- **CI/CD Mode:** Detects non-interactive environments and suppresses interactive prompts (fails if required inputs are missing).
