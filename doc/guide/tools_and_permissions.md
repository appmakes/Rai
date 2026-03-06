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

## Configuring tool permissions

Tool permissions are configured in `~/.config/rai/config.toml` (or per-profile in `~/.config/rai/config.<profile>.toml`). You can also use the interactive wizard via `rai config` → **Tools**.

### Global tool mode

The `tool_mode` setting controls how all tools behave by default:

```toml
tool_mode = "ask"       # Global default: ask, ask_once, allow, deny
no_tools = false        # Disable all tools (single-turn mode)
auto_approve = false    # Auto-approve all tool calls
```

| Value | Behavior |
|-------|----------|
| `"ask"` | Prompt before each tool call (default) |
| `"ask_once"` | Prompt once per tool per session, remember the choice |
| `"allow"` | Auto-approve everything |
| `"deny"` | Disable all tools entirely |

### Per-tool permission overrides

Override the default permission for individual tools using the `[tool_permissions]` table. Two formats are supported:

**Simple string** — set the permission mode directly:

```toml
[tool_permissions]
shell = "deny"                          # Completely disable shell access
file_write = "allow"                    # Auto-approve file writes
git_operations = "ask_once"             # Ask once per session for git
```

**Table with rules** — combine blacklist, whitelist, and fallback mode:

```toml
[tool_permissions]
shell = { blacklist = ["rm\\s+-rf", "sudo", "shutdown"], mode = "ask" }
```

Table fields (all optional):

| Field | Type | Description |
|-------|------|-------------|
| `mode` | String | Fallback permission when no pattern matches (`"allow"`, `"ask"`, `"ask_once"`, `"deny"`). If omitted, uses the tool's default. |
| `blacklist` | Array of regex | If any pattern matches the command, the call is **denied**. |
| `whitelist` | Array of regex | If present, **only** matching commands are allowed. |

**Evaluation order:** blacklist (deny) → whitelist (allow) → mode (fallback) → tool default.

### Examples

```toml
# Block dangerous commands, allow everything else
shell = { blacklist = ["rm\\s+-rf", "sudo", "shutdown"] }

# Only allow cargo and npm, deny everything else
shell = { whitelist = ["^cargo ", "^npm "] }

# Combine: block force-push, allow only git commands, ask for the rest
shell = { blacklist = ["push --force"], whitelist = ["^git "] }

# Block dangerous commands but allow whitelisted ones, ask for anything else
shell = { blacklist = ["sudo"], whitelist = ["^cargo ", "^npm "], mode = "ask" }

# Block writing to sensitive files
file_write = { blacklist = ["\\.env", "credentials"] }

# Block dangerous git operations
git_operations = { blacklist = ["push --force", "reset --hard"], mode = "ask" }
```

### Example: restrictive CI/CD profile

Create a locked-down profile for automated pipelines:

```toml
# ~/.config/rai/config.ci.toml
tool_mode = "allow"
auto_approve = true

[tool_permissions]
shell          = { whitelist = ["^cargo ", "^npm ", "^git "] }
file_write     = { blacklist = ["/etc", "/usr", "\\.env", "credentials"] }
git_operations = { blacklist = ["push --force", "reset --hard"] }
http_request   = "deny"
```

### Example: permissive local development

Trust the agent for local work while still blocking dangerous commands:

```toml
# ~/.config/rai/config.toml
tool_mode = "allow"
auto_approve = true

[tool_permissions]
shell = { blacklist = ["rm\\s+-rf", "sudo", "shutdown", "reboot"] }
```

### CLI overrides

CLI flags override config settings for the current run:

- `--yes` / `-y` — auto-approve all tool calls.
- `--no-tools` — disable tools entirely (single-turn mode).

```bash
# One-off auto-approve
rai -y "clean up temp files"

# Disable tools for a pure text response
rai --no-tools "what is the capital of France"
```

### Precedence

Permission checks are applied in this order:

1. **System path protection** — blocks critical paths and sensitive files, cannot be overridden.
2. **Per-tool permission** (`[tool_permissions]`) — if set, overrides the tool's default.
3. **Global tool_mode** — applies to tools without a per-tool override.
4. **CLI flags** (`--yes`, `--no-tools`) — override config for this run.

## Safety guardrails

Rai enforces path-level protections that cannot be overridden by any configuration.

### System path protection

All file tools (`file_read`, `file_write`, `file_append`, `file_edit`, `list_dir`, and git path operations) block access to critical system paths:

**Unix / macOS:**

- `/System`, `/Library` (macOS)
- `/bin`, `/sbin`, `/usr/bin`, `/usr/sbin`, `/usr/lib`, `/usr/libexec`
- `/etc`, `/private/etc`, `/private/var`
- `/dev`, `/boot`, `/proc`, `/sys`

**Windows** (case-insensitive):

- `C:\Windows`
- `C:\Program Files`, `C:\Program Files (x86)`
- `C:\ProgramData`, `C:\Recovery`, `C:\$Recycle.Bin`

### Sensitive file protection (write operations)

Write tools (`file_write`, `file_append`, `file_edit`) additionally block writes to sensitive dotfiles and credential stores:

- `.ssh/`, `.gnupg/`, `.gpg/`
- `.aws/`, `.docker/`, `.kube/`
- `.npmrc`, `.netrc`, `.env`
- `.git/config`, `.gitconfig`

Read access to these files is not blocked.

### Path traversal protection

All path-based tools reject:

- **Null bytes** in paths (CWE-158 bypass prevention)
- **URL-encoded traversal** patterns (`..%2f`, `%2f..`, `..%5c`, `%5c..`)
- Paths are **resolved to absolute form** before checking, so `../../etc/passwd` is caught even when the current directory is deep in a project tree.
