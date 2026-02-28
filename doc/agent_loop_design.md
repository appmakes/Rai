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
│  3. Safeguard check                 │
│     → Approve / Reject / Modify     │
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

| Tool | Description | Risk Level |
|------|-------------|------------|
| `shell` | Execute a shell command and return stdout/stderr | **High** |
| `read_file` | Read a file's contents | Medium |
| `write_file` | Write content to a file | **High** |
| `list_dir` | List files in a directory | Low |
| `http_get` | Fetch a URL and return the body | Medium |

Each tool has a JSON schema describing its parameters, which is sent to the AI provider as part of the tool definition.

### 3.2 User-Defined Tools (Tool Registry)

Users can define custom tools in `~/.config/rai/tools.toml` or in the task file frontmatter:

**Global tools (`~/.config/rai/tools.toml`):**
```toml
[[tools]]
name = "weather"
description = "Get current weather for a city"
command = "curl -s 'wttr.in/{city}?format=j1'"
params = ["city"]
risk = "low"

[[tools]]
name = "git_log"
description = "Get recent git commits"
command = "git log --oneline -n {count}"
params = ["count"]
risk = "low"
```

**Task-scoped tools (in `task.md` frontmatter):**
```markdown
---
model: gpt-4o
tools:
  - name: test_runner
    description: "Run project tests"
    command: "cargo test {test_name}"
    params: ["test_name"]
    risk: low
---

# Fix Failing Test
Find and fix the failing test. Use the test_runner tool to verify your fix.
```

### 3.3 Tool Call Protocol

Rai uses the AI provider's native tool/function calling API:

- **OpenAI**: `tools` parameter with `function` type
- **Anthropic**: `tools` parameter with `input_schema`
- **Poe**: OpenAI-compatible format

When the provider doesn't support native tool calling, rai falls back to a **prompt-based approach**: the system prompt describes the tools in text, and rai parses structured tool-call blocks from the AI's text response (e.g., ` ```tool:shell ... ``` `).

## 4. Safeguard System

Executing AI-requested commands is inherently dangerous. The safeguard system is the most critical part of this design.

### 4.1 Risk Classification

Every tool invocation is classified into one of three risk levels:

| Level | Description | Default Policy |
|-------|-------------|----------------|
| **Low** | Read-only, no side effects (e.g., `read_file`, `list_dir`, `http_get`) | Auto-approve |
| **Medium** | Potentially revealing (e.g., reading sensitive files, network calls to unknown hosts) | Auto-approve with logging |
| **High** | Side effects possible (e.g., `shell`, `write_file`) | **Require explicit approval** |

### 4.2 Approval Modes

Controlled via CLI flag or config:

```bash
# Default: prompt for high-risk, auto-approve low/medium
rai "weather in shanghai"

# Auto-approve everything (for CI/CD or trusted tasks)
rai --yes "weather in shanghai"
rai --auto-approve "weather in shanghai"

# Approve every single tool call regardless of risk
rai --approve-all "weather in shanghai"

# Disable all tool calling (single-turn only)
rai --no-tools "weather in shanghai"
```

**Interactive approval prompt:**
```
[rai] AI wants to execute:

  shell: curl -s 'wttr.in/Shanghai?format=j1'

  Risk: low (read-only HTTP request)

  [A]pprove  [R]eject  [E]dit  [A]pprove all remaining  → 
```

The user can:
- **Approve**: Execute as-is
- **Reject**: Skip this tool call, tell the AI it was denied
- **Edit**: Modify the command before execution, then approve
- **Approve all**: Stop asking for the rest of this session

### 4.3 Command Blocklist

A built-in blocklist prevents catastrophic commands from ever executing, even with `--yes`:

```
# Never executed, regardless of approval mode
rm -rf /
mkfs.*
dd if=.* of=/dev/.*
:(){ :|:& };:
shutdown
reboot
> /dev/sda
chmod -R 777 /
```

The blocklist is pattern-based (regex) and can be extended by the user in config:

```toml
# ~/.config/rai/config.toml
[safety]
blocked_patterns = [
    "rm\\s+-rf\\s+/",
    "DROP\\s+TABLE",
    "DELETE\\s+FROM.*WHERE\\s+1",
]
```

### 4.4 Sandboxing Options

For maximum safety, rai can restrict tool execution:

1. **Working directory jail**: Tools can only access files within the current working directory (or a configured root). Paths outside are rejected.

2. **Command allowlist mode**: Instead of a blocklist, only explicitly allowed commands can run:
   ```toml
   [safety]
   mode = "allowlist"   # "blocklist" (default) or "allowlist"
   allowed_commands = ["curl", "cat", "ls", "grep", "jq", "git"]
   ```

3. **Network restrictions**: Optionally limit HTTP tools to specific domains:
   ```toml
   [safety]
   allowed_domains = ["wttr.in", "api.github.com", "httpbin.org"]
   ```

4. **Read-only mode**: Disable `write_file` and restrict `shell` to read-only commands:
   ```bash
   rai --read-only "analyze my project structure"
   ```

### 4.5 Iteration Limits

Prevent runaway agent loops:

```toml
[agent]
max_iterations = 10      # Max tool-call round-trips per session
max_execution_time = 120  # Seconds before the loop is killed
max_output_size = 65536   # Max bytes of tool output fed back to AI
```

When a limit is hit, rai stops the loop and prints what it has so far.

### 4.6 Audit Log

Every tool execution is logged to `~/.local/share/rai/audit.log`:

```jsonl
{"ts":"2026-02-28T10:30:00Z","tool":"shell","cmd":"curl -s wttr.in/Shanghai","approved":"auto","risk":"low","exit_code":0,"duration_ms":450}
{"ts":"2026-02-28T10:30:01Z","tool":"shell","cmd":"rm -rf /tmp/test","approved":"rejected","risk":"high","reason":"user_denied"}
```

This provides a complete trace of what the AI asked for and what was actually executed.

## 5. Agent Loop Data Flow

```
User Prompt
    │
    ▼
┌────────────────────┐
│ Build Messages:    │
│ - system prompt    │
│ - tool definitions │
│ - user prompt      │
└────────┬───────────┘
         │
    ┌────▼────┐
    │ AI Call │◄─────────────────────────┐
    └────┬────┘                          │
         │                               │
    ┌────▼──────────────┐                │
    │ Response Type?    │                │
    ├───────────────────┤                │
    │ text → print,done │                │
    │ tool_call ────────┼──┐             │
    └───────────────────┘  │             │
                           ▼             │
                  ┌─────────────────┐    │
                  │ Safeguard Check │    │
                  ├─────────────────┤    │
                  │ blocked → deny  │    │
                  │ low risk → auto │    │
                  │ high risk → ask │    │
                  └────────┬────────┘    │
                           │             │
                  ┌────────▼────────┐    │
                  │ Execute Tool    │    │
                  │ capture output  │    │
                  └────────┬────────┘    │
                           │             │
                  ┌────────▼────────┐    │
                  │ Append tool     │    │
                  │ result to       │────┘
                  │ conversation    │
                  └─────────────────┘
```

## 6. System Prompt Design

The agent loop prepends a system prompt that defines the AI's behavior:

```
You are Rai, a CLI assistant. You have access to tools to help answer questions.

Rules:
- If you can answer directly from your knowledge, do so without tools.
- If you need real-time data or system information, use the available tools.
- Prefer the most specific tool available (e.g., `read_file` over `shell cat`).
- For shell commands: use simple, portable, read-only commands when possible.
- Never run destructive commands (rm -rf, drop table, etc.).
- If a tool call is rejected, explain what you needed and suggest alternatives.
- Keep tool usage minimal — only call tools when necessary.

Environment:
- OS: {os}
- Shell: {shell}
- Working directory: {cwd}
```

The system prompt is configurable in `~/.config/rai/config.toml`:
```toml
[agent]
system_prompt_file = "~/.config/rai/system_prompt.md"  # Optional override
```

## 7. Task File Integration

The agent loop integrates with `task.md` files. A task can opt into agent mode and define task-specific tools:

```markdown
---
model: gpt-4o
agent: true
tools:
  - name: run_tests
    description: "Run the test suite"
    command: "cargo test 2>&1"
    risk: low
  - name: read_source
    description: "Read a source file"
    command: "cat {filepath}"
    params: ["filepath"]
    risk: low
max_iterations: 5
---

# Debug Test Failure
The test `test_parse_subtask_frontmatter` is failing.
Investigate the cause and suggest a fix.
Use `run_tests` to see the current failure and `read_source` to examine the code.
```

**Non-agent task files** (default) continue to work exactly as before — single-turn, no tools. The `agent: true` flag is opt-in.

**Ad-hoc prompts** use agent mode by default when tools are available, but this can be disabled:
```bash
rai --no-tools "just answer from your knowledge"
```

## 8. CI/CD Considerations

In non-interactive environments (CI=1 or non-TTY):

- `--yes` must be explicitly passed to allow tool execution
- Without `--yes`, tool calls are **rejected** with an explanation
- The audit log is always written
- `max_iterations` and `max_execution_time` are enforced strictly

```bash
# CI pipeline example
RAI_API_KEY=$KEY rai --yes --read-only "summarize recent git changes"
```

## 9. Provider Trait Extension

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

## 10. Example Session

```
$ rai "what's the weather in Shanghai and Tokyo? compare them"

[rai] Calling tool: shell → curl -s 'wttr.in/Shanghai?format=j1' (auto-approved, low risk)
[rai] Calling tool: shell → curl -s 'wttr.in/Tokyo?format=j1' (auto-approved, low risk)

Weather Comparison:

| City     | Temp | Humidity | Condition |
|----------|------|----------|-----------|
| Shanghai | 28°C | 65%      | Sunny     |
| Tokyo    | 24°C | 72%      | Cloudy    |

Shanghai is warmer and drier. Tokyo has cloud cover with
a chance of rain later today. Pack an umbrella for Tokyo.
```

```
$ rai "find the largest file in this project"

[rai] Calling tool: shell → find . -type f -exec du -b {} + | sort -rn | head -5
      (auto-approved, low risk)

The largest files in your project:
1. target/release/rai — 12.4 MB (release binary)
2. target/debug/rai — 48.2 MB (debug binary)
3. Cargo.lock — 42 KB
```

```
$ rai "delete all .tmp files in /home"

[rai] AI wants to execute:
  shell: find /home -name "*.tmp" -delete
  Risk: HIGH (destructive file operation outside working directory)

  [A]pprove  [R]eject  [E]dit  [A]pprove all → r

[rai] Tool call rejected.

I understand the command was rejected for safety. Here's what I was going to do:
- Find all .tmp files under /home and delete them.

If you'd like to proceed, you can run it manually:
  find /home -name "*.tmp" -delete

Or run a dry-run first to see what would be deleted:
  find /home -name "*.tmp" -print
```

## 11. Implementation Phases

| Phase | Scope | Depends On |
|-------|-------|------------|
| C.1 | Provider trait extension (`chat_with_tools`), `ProviderResponse` types | — |
| C.2 | Agent loop core: iteration, tool dispatch, conversation management | C.1 |
| C.3 | Built-in tools: `shell`, `read_file`, `list_dir`, `http_get` | C.2 |
| C.4 | Safeguard system: risk classification, approval prompt, blocklist | C.3 |
| C.5 | User-defined tools: `tools.toml`, task-file `tools:` frontmatter | C.4 |
| C.6 | Audit logging, iteration limits, sandboxing options | C.4 |
| C.7 | CI/CD mode, `--yes`, `--no-tools`, `--read-only` flags | C.6 |

## 12. Open Questions

1. **Output truncation**: When a tool returns a very large output (e.g., `cat` on a big file), how aggressively should we truncate? Fixed byte limit? Let the model ask for specific ranges?

2. **Streaming**: Should tool results be streamed to the terminal as they come in, or only shown after the AI processes them?

3. **Cost control**: Agent loops can consume many tokens (each round-trip sends the full conversation). Should rai track token usage and warn/stop at a threshold?

4. **Concurrent tool calls**: Some providers return multiple tool calls at once. Should rai execute them in parallel or sequentially?

5. **Stateful tools**: Should tools be able to maintain state across calls? (e.g., a database connection tool that opens once and queries multiple times)
