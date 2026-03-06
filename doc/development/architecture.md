# Technical Architecture: Rai

## 1. System Design

Rai is a Rust-based CLI application. It follows a modular architecture:

1.  **CLI Layer (`src/cli/`)**: Handles command-line arguments using `clap`.
    -   Global flags: `--verbose` (sets log level), `--model` (overrides config).
    -   Passes structured data to the core logic.
2.  **Core Logic (`src/core/`)**:
    -   **Config Manager**: Loads/saves configuration from `~/.config/rai/config.toml`.
    -   **Task Parser**: Reads `task.md` files.
        -   Extracts **Global Frontmatter** (YAML) for task-wide settings and `args` list.
        -   Identifies task sections (Markdown headers).
        -   Extracts **Sub-task Frontmatter** (YAML) within sections for local `args` or model overrides.
    -   **Template Engine**: Replaces variables in prompts (e.g., `{{ file }}`) using a lightweight template engine (e.g., `tera` or simple regex).
    - **Executor**: Orchestrates the AI request. It constructs the final prompt, calls the AI provider, and handles the response.
3.  **AI Integration (`src/ai/`)**:
    - **Provider Interface**: A trait `AiProvider` to abstract different backends (OpenAI, Anthropic, etc.).
    - **Implementations**: `OpenAiProvider`, `AnthropicProvider`, etc.
4.  **UI/TUI (`src/ui/`)**:
    - **Interactive Prompts**: Uses `inquire` or `dialoguer` for user input (e.g., selecting tasks in `rai plan`).
    - **Output Formatting**: Uses `colored` or `ratatui` for rich terminal output.

## 2. Technology Stack

-   **Language**: Rust (Edition 2021+)
-   **Argument Parsing**: `clap` (derive feature)
-   **Async Runtime**: `tokio`
-   **AI Client**: `async-openai` (for OpenAI compatible APIs), `reqwest` (for custom API calls)
-   **Configuration**: `config` crate + `serde` + `toml`
-   **Markdown Parsing**: `pulldown-cmark` (to parse task structure)
-   **Templating**: `tera` (powerful, familiar syntax)
-   **Interactive UI**: `inquire` (modern, type-safe prompts)
-   **Logging**: `tracing` + `tracing-subscriber`
-   **Error Handling**: `thiserror` + `anyhow`

## 3. Data Flow

1.  **User Input**: `rai run task.md "#summary" file.txt`
2.  **CLI Parser**: Identifies command `run`, file `task.md`, sub-task `#summary`, argument `file.txt`.
3.  **Task Parser**:
    -   Reads `task.md`.
    -   Parses Frontmatter -> Config overrides (e.g., model).
    -   Finds section `## Summary` (matching `#summary`).
    -   Extracts the prompt text.
4.  **Template Engine**:
    -   Detects variable `{{ input }}` in prompt.
    -   Maps `file.txt` to `{{ input }}` (based on argument position or name).
    -   Reads `file.txt` content (if the prompt implies reading the file, or just passes the filename). *Decision: Rai should have a helper or syntax to read file content, e.g. `{{ read(file) }}` or automatically read if it's a file path.*
    -   Produces final prompt string.
5.  **Executor**:
    -   Selects AI Provider based on config.
    -   Sends request (System Prompt + User Prompt).
    -   Streams response to stdout.

## 4. `task.md` Format Details

Rai uses a convention-based Markdown format.

-   **Metadata**: YAML frontmatter at the top.
-   **Task Definitions**:
    -   Level 1 Header (`#`) defines the **Default Task**.
    -   Level 2 Headers (`##`) define **Sub-tasks**, addressable via tags (e.g., `## security` -> `rai run ... "#security"`).
    -   Code blocks can be used to provide context or examples, but the text outside code blocks is treated as the prompt instructions.

## 5. Security & Safety

-   **API Keys**: Stored in OS-native keyring (optional) or config file with restricted permissions (0600).
-   **File Access**: Rai should only read files explicitly passed as arguments or referenced in the task.
-   **Confirmation**: In `plan` mode, show the full prompt and estimated cost before execution.
