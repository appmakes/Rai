# Design Document: Agent Loop with Tool Calling

## 1. Problem Statement

Rai currently operates in a single-turn request-response model: the user sends a prompt, the AI responds, done. This breaks down when the AI needs **real-world data** it doesn't have — live weather, current file contents, API responses, system state, database queries, etc.

The model knows *what* it needs but has no way to *get* it.

**Example:**
```
$ rai "weather in shanghai"
→ "I'm unable to provide real-time weather updates..."
```

The model *could* answer if it had access to `curl wttr.in/Shanghai` — but today it can't ask for that, and rai can't provide it.

## 2. Solution: Agent Loop

Transform rai from a single-turn tool into a **multi-turn agent** that can:

1. Receive the user's prompt
2. Send it to the AI with a set of **available tools**
3. If the AI responds with a **tool call** → execute it → feed the result back
4. Repeat until the AI produces a **final text response**
5. If the AI returns terminal `fail` after tool usage but the failure looks recoverable (for example poor source quality), inject retry guidance and allow a bounded number of additional attempts

```
User: "weather in shanghai"
  │
  ▼
┌─────────────────────────────────────┐
│  Rai Agent Loop                     │
│                                     │
│  1. Send prompt + tool definitions  │
│     to AI provider                  │
│                                     │
│  2. AI responds:                    │
│     ┌─ text response → print & done │
│     └─ tool_call("shell",           │
│         {"cmd": "curl ..."})        │
│                                     │
│  3. Safeguard check (two layers)    │
│     → Global blocklist              │
│     → Per-tool permission           │
│                                     │
│  4. Execute tool, capture output    │
│                                     │
│  5. Send tool result back to AI     │
│     → goto step 2                   │
│                                     │
└─────────────────────────────────────┘
```

## 3. Tool System Design

### 3.1 Built-in Tools

Rai ships with a small set of built-in tools that the AI can invoke:

| Tool | Description | Default Permission |
|------|-------------|--------------------|
| `shell` | Execute a shell command and return stdout/stderr | `ask` |
| `ls_tools` | List built-in tools and permissions | `allow` |
| `file_read` | Read a file's contents | `allow` |
| `file_write` | Write content to a file | `ask` |
| `list_dir` | List files in a directory | `allow` |
| `http_get` | Fetch a URL and return the body | `allow` |

Each tool has a JSON schema describing its parameters, which is sent to the AI provider as part of the tool definition.

### 3.2 User-Defined Tools (Tool Registry)

Users can define custom tools in `~/.config/rai/tools.toml` or in the task file frontmatter.

**Global tools (`~/.config/rai/tools.toml`):**
```toml
[[tools]]
name = "weather"
description = "Get current weather for a city"
command = "curl -s 'wttr.in/{city}?format=j1'"
params = ["city"]
permission = "allow"

[[tools]]
name = "git_log"
description = "Get recent git commits"
command = "git log --oneline -n {count}"
params = ["count"]
permission = "allow"

[[tools]]
name = "deploy"
description = "Deploy to production"
command = "scripts/deploy.sh {env}"
params = ["env"]
permission = "ask"
```

### 3.3 Tool Call Protocol

Rai uses the AI provider's native tool/function calling API:

- **OpenAI**: `tools` parameter with `function` type
- **Anthropic**: `tools` parameter with `input_schema`
- **Poe**: OpenAI-compatible format

When the provider doesn't support native tool calling, rai falls back to a **prompt-based approach**: the system prompt describes the tools in text, and rai parses structured tool-call blocks from the AI's text response (e.g., ` ```tool:shell ... ``` `).

## 4. Permission System

### 4.1 Design Principle

The old design used `risk` levels (low / medium / high) which describe *what a tool is*. The new design uses `permission` which describes *what rai should do*. No translation table needed — the permission **is** the policy.

### 4.2 Permission Levels

Each tool has a `permission` field with one of six values, ordered from most permissive to most restrictive:

```
  allow                     Most permissive
    │                       Auto-approve everything.
    ▼
  blacklist: <patterns>
    │                       Auto-approve, UNLESS the resolved
    │                       command matches a pattern → deny.
    ▼
  ask_once
    │                       Ask the user on the first invocation.
    │                       Remember the answer for this session.
    ▼
  ask
    │                       Ask the user every time.
    ▼
  whitelist: <patterns>
    │                       Deny by default, UNLESS the resolved
    │                       command matches a pattern → allow.
    ▼
  deny                      Most restrictive
                            Always reject. Tool is disabled.
```

**Examples:**
```toml
# Auto-approve all invocations
[[tools]]
name = "file_read"
permission = "allow"

# Allow shell, but block destructive patterns
[[tools]]
name = "shell"
permission = 'blacklist: rm\s+-rf|mkfs|dd\s+if=|shutdown|reboot'

# Ask the first time, then remember
[[tools]]
name = "http_get"
permission = "ask_once"

# Ask every single time
[[tools]]
name = "file_write"
permission = "ask"

# Only allow curl and wget commands through shell
[[tools]]
name = "shell"
permission = "whitelist: ^curl\\s|^wget\\s"

# Completely disabled
[[tools]]
name = "file_write"
permission = "deny"
```

### 4.3 Pattern Matching

`blacklist` and `whitelist` patterns are regex and match against the **fully resolved command/argument string**. What constitutes the match target depends on the tool:

| Tool | Pattern matches against | Example |
|------|------------------------|---------|
| `shell` | The full command string | `rm -rf /tmp/data` |
| `file_read` | The file path | `/etc/passwd` |
| `file_write` | The file path | `src/main.rs` |
| `list_dir` | The directory path | `/home/user` |
| `http_get` | The full URL | `https://evil.com/steal` |
| User-defined | The interpolated `command` string | `curl -s 'wttr.in/Shanghai'` |

Multiple patterns are separated by `|` (regex alternation) or specified as a list:

```toml
# Single-line with alternation
permission = 'blacklist: rm\s+-rf|mkfs|shutdown'

# Multi-pattern list (TOML array syntax)
permission = ["blacklist", 'rm\s+-rf', 'mkfs', 'shutdown']
```

## 5. Two-Layer Safeguard Architecture

Every tool call passes through **two independent layers**. Both must approve for execution to proceed.

```
Tool Call from AI
       │
       ▼
┌──────────────────────────────────────┐
│  Layer 1: Global Blocklist           │
│                                      │
│  Unconditional. Not overridable.     │
│  Applies to ALL tools, ALL modes.    │
│  Even --yes cannot bypass this.      │
│                                      │
│  Match? → DENY (always)             │
│  No match? → pass to Layer 2        │
└──────────────┬───────────────────────┘
               │
               ▼
┌──────────────────────────────────────┐
│  Layer 2: Per-Tool Permission        │
│                                      │
│  allow      → execute                │
│  blacklist  → check pattern          │
│  ask_once   → prompt (or recall)     │
│  ask        → prompt                 │
│  whitelist  → check pattern          │
│  deny       → reject                 │
└──────────────────────────────────────┘
```

### 5.1 Layer 1: Global Blocklist

A hardcoded + user-extensible set of patterns that are **always denied**, regardless of per-tool permissions, `--yes` flags, or any other setting. This is the nuclear safety net.

**Built-in (hardcoded, cannot be removed):**
```
rm\s+-rf\s+/[^.]       # rm -rf / (but not rm -rf ./local)
mkfs\.
dd\s+if=.*of=/dev/
:()\{.*\|.*&\s*\};     # fork bomb
>\s*/dev/sd
chmod\s+-R\s+777\s+/
```

**User-extensible in config:**
```toml
# ~/.config/rai/config.toml
[safety]
blocked_patterns = [
    'DROP\s+TABLE',
    'DELETE\s+FROM.*WHERE\s+1',
    'curl.*\|\s*bash',
    'eval\s*\(',
]
```

### 5.2 Layer 2: Per-Tool Permission

Resolved from the tool's `permission` field. See §4.2.

### 5.3 Interactive Approval Prompt

When permission requires user input (`ask` or `ask_once`):

```
[rai] AI wants to execute:

  shell: curl -s 'wttr.in/Shanghai?format=j1'

  [Y]es  [N]o  [E]dit  [A]lways  → 
```

Options:
- **Yes**: Execute this call
- **No**: Reject this call, tell AI it was denied
- **Edit**: Modify the command, then execute the edited version
- **Always**: Switch this tool to `allow` for the rest of the session

## 6. Task-Level Permission Overrides

Task files (`task.md`) can reference tools in their frontmatter for two purposes:

1. **Add new tools** scoped to this task
2. **Restrict existing tools** by tightening their permission

### 6.1 Restriction Rule

A task file can only move a tool's permission **down** (more restrictive), never **up**:

```
allow → blacklist → ask_once → ask → whitelist → deny
                    ◄── task can move this direction only
```

If a task tries to relax a permission (e.g., global is `ask`, task says `allow`), the task override is **ignored** and rai logs a warning.

### 6.2 Pattern Merging

When a task modifies a `blacklist` or `whitelist` tool:

- **Blacklist**: task patterns are **added** to the global patterns (union). The task makes the blacklist stricter.
- **Whitelist**: task patterns are **intersected** with the global patterns. The task can only narrow what's allowed, never expand.

**Example:**

```toml
# ~/.config/rai/tools.toml — global
[[tools]]
name = "shell"
permission = 'blacklist: rm|shutdown'
```

```yaml
# task.md — task-level override
---
tools:
  - name: shell
    permission: "blacklist: curl|wget"
---
```

**Effective permission for this task:**
`blacklist: rm|shutdown|curl|wget`

The task added `curl|wget` to the existing blocklist. It did not remove `rm|shutdown`.

### 6.3 Adding New Tools

Tasks can define tools that don't exist globally. These are available only during that task's execution and can have any permission level:

```yaml
---
model: gpt-4o
agent: true
tools:
  - name: run_tests
    description: "Run the test suite"
    command: "cargo test 2>&1"
    permission: allow
  - name: read_source
    description: "Read a source file"
    command: "cat {filepath}"
    params: ["filepath"]
    permission: allow
max_iterations: 5
---

# Debug Test Failure
The test `test_parse_subtask_frontmatter` is failing.
Investigate the cause and suggest a fix.
```

### 6.4 Denying Tools

A task can disable a globally available tool:

```yaml
---
tools:
  - name: file_write
    permission: deny
  - name: shell
    permission: "whitelist: ^cat\\s|^ls\\s|^grep\\s"
---

# Code Review
Review this codebase. Read files as needed. Do not modify anything.
```

This restricts the task to read-only operations even if the global config allows writes.

## 7. Agent Loop Data Flow

```
User Prompt
    │
    ▼
┌────────────────────────────┐
│ Build Messages:            │
│ - system prompt            │
│ - tool definitions         │
│   (global + task, merged)  │
│ - user prompt              │
└────────────┬───────────────┘
             │
        ┌────▼────┐
        │ AI Call │◄──────────────────────────────┐
        └────┬────┘                               │
             │                                    │
        ┌────▼──────────────┐                     │
        │ Response Type?    │                     │
        ├───────────────────┤                     │
        │ text → print,done │                     │
        │ tool_call ────────┼───┐                 │
        └───────────────────┘   │                 │
                                ▼                 │
                   ┌────────────────────┐         │
                   │ Layer 1:           │         │
                   │ Global Blocklist   │         │
                   │ Match? → DENY      │         │
                   └─────────┬──────────┘         │
                             │ pass               │
                             ▼                    │
                   ┌────────────────────┐         │
                   │ Layer 2:           │         │
                   │ Per-Tool Permission│         │
                   │ allow/ask/deny/... │         │
                   └─────────┬──────────┘         │
                             │ approved           │
                             ▼                    │
                   ┌────────────────────┐         │
                   │ Execute Tool       │         │
                   │ capture output     │         │
                   └─────────┬──────────┘         │
                             │                    │
                   ┌─────────▼──────────┐         │
                   │ Append tool result │─────────┘
                   │ to conversation    │
                   └────────────────────┘
```

## 8. System Prompt Design

The agent loop prepends a system prompt that defines the AI's behavior:

```
You are Rai, a CLI assistant. You have access to tools to help answer questions.

Rules:
- If you can answer directly from your knowledge, do so without tools.
- If you need real-time data or system information, use the available tools.
- Prefer the most specific tool available (e.g., `file_read` over `shell cat`).
- For shell commands: use simple, portable, read-only commands when possible.
- Never run destructive commands (rm -rf, drop table, etc.).
- If a tool call is rejected, explain what you needed and suggest alternatives.
- Keep tool usage minimal — only call tools when necessary.

Environment:
- OS: {os}
- Shell: {shell}
- Working directory: {cwd}
```

The system prompt is configurable:
```toml
# ~/.config/rai/config.toml
[agent]
system_prompt_file = "~/.config/rai/system_prompt.md"  # Optional override
```

## 9. Task File Integration

**Non-agent task files** (default) continue to work exactly as before — single-turn, no tools. The `agent: true` flag is opt-in.

**Ad-hoc prompts** use agent mode by default when tools are available, but this can be disabled:
```bash
rai --no-tools "just answer from your knowledge"
```

## 10. CLI Flags

```bash
# Default: tools enabled, permissions as configured
rai "weather in shanghai"

# Auto-approve all tool calls (skips ask/ask_once prompts)
# Global blocklist still enforced
rai --yes "weather in shanghai"

# Prompt for every tool call, regardless of permission level
rai --ask-all "do something complex"

# Disable all tool calling (single-turn only)
rai --no-tools "just answer from your knowledge"

# Read-only mode: deny file_write, restrict shell
rai --read-only "analyze my project structure"
```

| Flag | Effect |
|------|--------|
| `--yes` | Treat `ask` and `ask_once` as `allow`. Global blocklist still enforced. |
| `--ask-all` | Treat every tool as `ask`. Override `allow` to require confirmation. |
| `--no-tools` | Disable agent loop entirely. Single-turn mode. |
| `--read-only` | Set `file_write` to `deny`, restrict `shell` to read-only patterns. |

## 11. CI/CD Considerations

In non-interactive environments (`CI=1` or non-TTY):

- Tools with `ask` or `ask_once` permission are **auto-denied** unless `--yes` is passed
- `--yes` must be explicitly provided to allow tool execution
- The audit log is always written
- Iteration limits are enforced strictly

```bash
# CI pipeline example
OPENAI_API_KEY=$KEY rai --yes --read-only "summarize recent git changes"
```

## 12. Iteration Limits

Prevent runaway agent loops:

```toml
# ~/.config/rai/config.toml
[agent]
max_iterations = 30      # Max tool-call round-trips per session
max_execution_time = 120  # Seconds before the loop is killed
max_output_size = 65536   # Max bytes of tool output fed back to AI
```

When a limit is hit, rai stops the loop and presents whatever the AI has produced so far.

## 13. Audit Log

Every tool execution is logged to `~/.local/share/rai/audit.log`:

```jsonl
{"ts":"2026-02-28T10:30:00Z","tool":"shell","cmd":"curl -s wttr.in/Shanghai","permission":"allow","decision":"approved","exit_code":0,"duration_ms":450}
{"ts":"2026-02-28T10:30:01Z","tool":"shell","cmd":"rm -rf /tmp/test","permission":"ask","decision":"user_denied"}
{"ts":"2026-02-28T10:30:02Z","tool":"shell","cmd":"rm -rf /","permission":"allow","decision":"global_blocklist","matched_pattern":"rm\\s+-rf\\s+/"}
```

The third entry shows the global blocklist overriding a tool-level `allow` — the audit trail makes this visible.

## 14. Provider Trait Extension

The `Provider` trait gains a new method for tool-calling:

```rust
#[async_trait]
pub trait Provider {
    /// Single-turn chat (existing)
    async fn chat(&self, model: &str, message: &str) -> Result<String>;

    /// Multi-turn chat with tool support
    async fn chat_with_tools(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<ProviderResponse>;
}

pub enum ProviderResponse {
    Text(String),
    ToolCalls(Vec<ToolCall>),
}

pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}
```

Providers that don't support native tool calling return `ProviderResponse::Text` always, and the agent loop handles the prompt-based fallback.

## 15. Example Sessions

### Weather query (tools auto-approved)

```
$ rai "what's the weather in Shanghai and Tokyo? compare them"

[rai] shell: curl -s 'wttr.in/Shanghai?format=j1'  ✓
[rai] shell: curl -s 'wttr.in/Tokyo?format=j1'  ✓

Weather Comparison:

| City     | Temp | Humidity | Condition |
|----------|------|----------|-----------|
| Shanghai | 28°C | 65%      | Sunny     |
| Tokyo    | 24°C | 72%      | Cloudy    |

Shanghai is warmer and drier. Tokyo has cloud cover with
a chance of rain later today. Pack an umbrella for Tokyo.
```

### Dangerous command (permission = ask, user rejects)

```
$ rai "delete all .tmp files in /home"

[rai] AI wants to execute:

  shell: find /home -name "*.tmp" -delete

  [Y]es  [N]o  [E]dit  [A]lways  → n

[rai] Tool call rejected.

I understand the command was rejected. Here's what I was going to do:
- Find all .tmp files under /home and delete them.

If you'd like to proceed, you can run it manually:
  find /home -name "*.tmp" -delete

Or run a dry-run first to see what would be deleted:
  find /home -name "*.tmp" -print
```

### Global blocklist stops catastrophic command

```
$ rai --yes "clean up everything, start fresh"

[rai] shell: rm -rf /  ✗ (blocked by global safety rule)

I attempted to run `rm -rf /` but it was blocked by a safety rule.
This command would delete your entire filesystem. I cannot execute it
under any circumstances.

If you want to clean up specific directories, tell me which ones
and I'll suggest safer alternatives.
```

### Task file with restricted tools

```yaml
# review.md
---
agent: true
tools:
  - name: shell
    permission: "whitelist: ^cat\\s|^wc\\s|^grep\\s"
  - name: file_write
    permission: deny
---
# Code Review
Review the source files in src/ for potential bugs.
```

```
$ rai review.md

[rai] shell: cat src/main.rs  ✓
[rai] shell: cat src/config.rs  ✓
[rai] shell: grep -rn "unwrap()" src/  ✓
[rai] shell: python3 -c "..."  ✗ (not in whitelist)

Found 3 potential issues:
1. ...
```

## 16. Implementation Phases

| Phase | Scope | Depends On |
|-------|-------|------------|
| C.1 | Provider trait extension (`chat_with_tools`), `ProviderResponse` types | — |
| C.2 | Permission system: parsing, resolution, two-layer check | — |
| C.3 | Agent loop core: iteration, tool dispatch, conversation management | C.1, C.2 |
| C.4 | Built-in tools: `shell`, `ls_tools`, `file_read`, `file_write`, `list_dir`, `http_get` | C.3 |
| C.5 | Interactive approval prompt (Yes/No/Edit/Always) | C.3 |
| C.6 | User-defined tools: `tools.toml` parsing, task-file tool merging | C.4 |
| C.7 | Task-level overrides: restriction enforcement, pattern merging | C.6 |
| C.8 | Audit logging, iteration limits | C.3 |
| C.9 | CLI flags: `--yes`, `--ask-all`, `--no-tools`, `--read-only` | C.5 |
| C.10 | CI/CD mode: auto-deny without `--yes` | C.9 |

## 17. Open Questions

1. **Output truncation**: When a tool returns very large output (e.g., `cat` on a big file), how aggressively should we truncate? Fixed byte limit? Let the model ask for specific line ranges?

2. **Streaming**: Should tool results be streamed to the terminal as they come in, or only shown after the AI processes them?

3. **Cost control**: Agent loops consume many tokens (each round-trip sends the full conversation). Should rai track token usage and warn/stop at a threshold?

4. **Concurrent tool calls**: Some providers return multiple tool calls at once. Should rai execute them in parallel or sequentially? Parallel is faster, but sequential is easier to audit and approve.

5. **`ask_once` scope**: Should `ask_once` memory be per-tool or per-tool-per-arguments? If I approve `shell: curl` once, does that also approve `shell: rm`?

6. **Permission inheritance for user-defined tools**: If a user defines a tool with `command = "curl ..."`, should it inherit the `shell` tool's permission, or start fresh?
