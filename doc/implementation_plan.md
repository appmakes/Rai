# Implementation Plan: Rai

## Phase 1: Core CLI Infrastructure
**Goal:** Establish the Rust project and CLI parsing.
1.  Initialize Cargo project (`cargo init`).
2.  Add dependencies: `clap` (derive), `tokio` (full), `anyhow`.
3.  Define `Cli` struct with subcommands: `Config`, `Run`, `Create`, `Plan`.
4.  Implement basic logging (`tracing`).
5.  Create placeholders for command handlers.

## Phase 2: Configuration Management
**Goal:** Allow users to store API keys and settings.
1.  Implement `Config` struct (API key, default model, provider).
2.  Add `config` crate or `serde` + `toml` logic.
3.  Implement `rai config`:
    -   Prompts user for values.
    -   Saves to `~/.config/rai/config.toml` (or platform equivalent).
4.  Implement loading config on startup.

## Phase 3: Basic AI Integration
**Goal:** Send a simple prompt to an AI provider and print the response.
1.  Create `AiProvider` trait.
2.  Implement `OpenAiProvider` using `async-openai` (or `reqwest` manually).
3.  Implement `rai run "prompt string"`:
    -   Pass the prompt to the provider.
    -   Stream the response to stdout.

## Phase 4: `task.md` Parser & Executor
**Goal:** Support file-based tasks.
1.  Add `pulldown-cmark` dependency.
2.  Implement `TaskParser`:
    -   Read file content.
    -   Parse YAML frontmatter (for model override).
    -   Extract task description (Markdown content).
3.  Connect `rai run task.md` to the parser and executor.

## Phase 5: Templating & Arguments
**Goal:** Support dynamic tasks with variables.
1.  Add `tera` or implement simple regex replacement for `{{ var }}`.
2.  Update `TaskParser` to identify variables.
3.  Update `rai run` to accept trailing arguments.
4.  Map CLI arguments to task variables (positional or named).

## Phase 6: Sub-tasks
**Goal:** Support `#sub-task` syntax.
1.  Update `TaskParser` to split content by headers (`#`, `##`).
2.  Implement logic to select a specific section based on the `#tag` provided in the command.
3.  Update `rai run` to handle the optional `#sub-task` argument.

## Phase 7: Interactive Features (`create`, `plan`)
**Goal:** Enhance user experience.
1.  Add `inquire` or `dialoguer`.
2.  Implement `rai create`:
    -   Wizard to ask for task name, description, variables.
    -   Generate a `task.md` file.
3.  Implement `rai plan`:
    -   Parse `task.md`.
    -   Show a summary of the task and variables.
    -   Allow user to edit/confirm variables.
    -   Display estimated cost (token count approximation).
    -   Execute upon confirmation.

## Phase 8: Polish & CI/CD
**Goal:** Ensure robustness.
1.  Detect non-interactive environments (CI) to disable prompts.
2.  Add error handling for missing API keys, network errors, invalid markdown.
3.  Write integration tests.
