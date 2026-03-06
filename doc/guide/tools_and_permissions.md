# Tools and Permissions

Rai gives the AI agent access to built-in tools for file operations, web access, and shell commands. A layered permission system keeps actions safe and scoped.

## Built-in tools

| Tool | Description | Default Permission |
|------|-------------|-------------------|
| `shell` | Execute shell commands | Ask |
| `file_read` | Read file contents | Allow |
| `file_write` | Write to files | Ask |
| `file_append` | Append to files | Ask |
| `file_edit` | Edit file content | Ask |
| `list_dir` | List directory contents | Allow |
| `ls_tools` | List available tools | Allow |
| `http_get` | HTTP GET requests | Allow |
| `http_request` | HTTP requests (any method) | Allow |
| `web_fetch` | Fetch and extract web content | Allow |
| `web_search` | Search the web | Allow |
| `git_operations` | Git commands | Ask |
| `ask` | Request user input | Allow |

Tools marked **Allow** run automatically. Tools marked **Ask** prompt you for approval before executing.

You can ask the agent to call `ls_tools` to see the full list with descriptions.

## Permission modes

Each tool can be set to one of these permission levels:

- **Allow** — auto-approve, no prompting.
- **Ask** — always ask the user before executing (default for write operations).
- **AskOnce** — ask once per session, remember the choice.
- **Deny** — tool is completely disabled.
- **Blacklist(patterns)** — block execution if the command matches any regex pattern.
- **Whitelist(patterns)** — allow only if the command matches a regex pattern.

## Global tool settings

Configure via `rai config` or directly in `~/.config/rai/config.toml`:

```toml
tool_mode = "ask"       # Global default: ask, ask_once, allow, deny
no_tools = false        # Disable all tools (single-turn mode)
auto_approve = false    # Auto-approve all tool calls
```

### `tool_mode`

| Value | Behavior |
|-------|----------|
| `"ask"` | Prompt before each tool call (default) |
| `"ask_once"` | Prompt once per tool per session |
| `"allow"` | Auto-approve everything |
| `"deny"` | Disable all tools |

### CLI overrides

- `--yes` / `-y` — auto-approve all tool calls for this run.
- `--no-tools` — disable tools entirely (single-turn mode).

## Safety guardrails

Rai enforces two layers of protection that cannot be overridden.

### Hardcoded command blocklist

These dangerous patterns are always blocked, regardless of permission settings:

- `rm -rf /` (recursive root deletion)
- `mkfs.*` (filesystem creation)
- `dd if=... of=/dev/...` (raw device writes)
- Fork bombs
- `chmod -R 777 /` (recursive permission changes)
- `shutdown` / `reboot`
- Redirects to `/dev/sd*`

### System path protection (Unix)

File operations (`file_read`, `file_write`, `file_append`, `file_edit`, `list_dir`, and git path operations) are blocked on critical system paths:

- `/System`, `/Library` (macOS)
- `/bin`, `/sbin`, `/usr/bin`, `/usr/sbin`, `/usr/lib`, `/usr/libexec`
- `/etc`, `/private/etc`, `/private/var`
- `/dev`, `/boot`, `/proc`, `/sys`

These prefixes are hard-blocked and cannot be overridden by any configuration.
