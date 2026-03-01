use crate::permission::{check_global_blocklist, check_permission, Permission, PermissionDecision};
use crate::providers::{Message, Provider, ProviderResponse};
use crate::tools::{Tool, ToolCall, ToolDefinition};
use anyhow::Result;
use dialoguer::{Input, Select};
use std::collections::HashMap;
use tracing::info;

const DEFAULT_MAX_ITERATIONS: usize = 10;

pub struct AgentConfig {
    pub auto_approve: bool,
    pub max_iterations: usize,
    pub blocked_patterns: Vec<String>,
    pub log_enabled: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            auto_approve: false,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            blocked_patterns: Vec::new(),
            log_enabled: false,
        }
    }
}

pub struct Agent {
    provider: Box<dyn Provider>,
    model: String,
    tools: Vec<Box<dyn Tool>>,
    config: AgentConfig,
    ask_once_memory: HashMap<String, bool>,
}

impl Agent {
    pub fn new(
        provider: Box<dyn Provider>,
        model: String,
        tools: Vec<Box<dyn Tool>>,
        config: AgentConfig,
    ) -> Self {
        Self {
            provider,
            model,
            tools,
            config,
            ask_once_memory: HashMap::new(),
        }
    }

    pub async fn run(&mut self, prompt: &str) -> Result<String> {
        let system_prompt = build_system_prompt();
        let tool_defs: Vec<ToolDefinition> = self.tools.iter().map(|t| t.definition()).collect();

        let active_tools: Vec<ToolDefinition> = tool_defs
            .iter()
            .filter(|t| !matches!(t.permission, Permission::Deny))
            .cloned()
            .collect();

        let mut messages: Vec<Message> =
            vec![Message::system(&system_prompt), Message::user(prompt)];

        for iteration in 0..self.config.max_iterations {
            info!(
                "Agent iteration {}/{}",
                iteration + 1,
                self.config.max_iterations
            );

            let response = self
                .provider
                .chat_with_tools(&self.model, &messages, &active_tools)
                .await?;

            match response {
                ProviderResponse::Text(text) => {
                    return Ok(text);
                }
                ProviderResponse::ToolCalls(tool_calls) => {
                    messages.push(Message::assistant_tool_calls(&tool_calls));

                    for tc in &tool_calls {
                        let result = self.handle_tool_call(tc)?;

                        let output = if result.success {
                            result.output.clone()
                        } else {
                            format!("[Tool call failed] {}", result.output)
                        };

                        messages.push(Message::tool_result(&result.tool_call_id, &output));
                    }
                }
            }
        }

        anyhow::bail!(
            "Agent loop reached maximum iterations ({}). Stopping.",
            self.config.max_iterations
        );
    }

    fn handle_tool_call(&mut self, tc: &ToolCall) -> Result<crate::tools::ToolResult> {
        let tool_idx = self
            .tools
            .iter()
            .position(|t| t.definition().name == tc.name);

        let tool_idx = match tool_idx {
            Some(idx) => idx,
            None => {
                return Ok(crate::tools::ToolResult {
                    tool_call_id: tc.id.clone(),
                    output: format!("Unknown tool: {}", tc.name),
                    success: false,
                });
            }
        };

        let def = self.tools[tool_idx].definition();
        let match_target = self.tools[tool_idx].match_target(&tc.arguments);

        // Layer 1: Global blocklist
        if let Some(reason) = check_global_blocklist(&match_target, &self.config.blocked_patterns) {
            if self.config.log_enabled {
                eprintln!("[rai] {} → {}  ✗ ({})", tc.name, match_target, reason);
            }
            return Ok(crate::tools::ToolResult {
                tool_call_id: tc.id.clone(),
                output: format!("Blocked: {}", reason),
                success: false,
            });
        }

        // Layer 2: Per-tool permission
        let decision = if self.config.auto_approve {
            match &def.permission {
                Permission::Deny => PermissionDecision::Deny("tool is disabled".to_string()),
                _ => PermissionDecision::Allow,
            }
        } else {
            match &def.permission {
                Permission::AskOnce => {
                    if let Some(&remembered) = self.ask_once_memory.get(&tc.name) {
                        if remembered {
                            PermissionDecision::Allow
                        } else {
                            PermissionDecision::Deny("previously denied".to_string())
                        }
                    } else {
                        PermissionDecision::NeedAsk
                    }
                }
                other => check_permission(other, &match_target),
            }
        };

        match decision {
            PermissionDecision::Allow => {
                if self.config.log_enabled {
                    eprintln!("[rai] {}: {}  ✓", tc.name, match_target);
                }
                match self.tools[tool_idx].execute(&tc.arguments) {
                    Ok(output) => Ok(crate::tools::ToolResult {
                        tool_call_id: tc.id.clone(),
                        output,
                        success: true,
                    }),
                    Err(e) => Ok(crate::tools::ToolResult {
                        tool_call_id: tc.id.clone(),
                        output: format!("Error: {}", e),
                        success: false,
                    }),
                }
            }
            PermissionDecision::Deny(reason) => {
                if self.config.log_enabled {
                    eprintln!("[rai] {}: {}  ✗ ({})", tc.name, match_target, reason);
                }
                Ok(crate::tools::ToolResult {
                    tool_call_id: tc.id.clone(),
                    output: format!("Denied: {}", reason),
                    success: false,
                })
            }
            PermissionDecision::NeedAsk => self.interactive_approve(tc, tool_idx),
        }
    }

    fn interactive_approve(
        &mut self,
        tc: &ToolCall,
        tool_idx: usize,
    ) -> Result<crate::tools::ToolResult> {
        let match_target = self.tools[tool_idx].match_target(&tc.arguments);

        if !atty::is(atty::Stream::Stdin) {
            if self.config.log_enabled {
                eprintln!(
                    "[rai] {}: {}  ✗ (non-interactive, use --yes to auto-approve)",
                    tc.name, match_target
                );
            }
            return Ok(crate::tools::ToolResult {
                tool_call_id: tc.id.clone(),
                output: "Denied: non-interactive mode. Use --yes to auto-approve.".to_string(),
                success: false,
            });
        }

        eprintln!("\n[rai] AI wants to execute:\n");
        eprintln!("  {}: {}\n", tc.name, match_target);

        let options = vec!["Yes", "No", "Edit", "Always (approve all for this session)"];
        let selection = Select::new()
            .with_prompt("Allow?")
            .items(&options)
            .default(0)
            .interact_opt()?;

        match selection {
            Some(0) => {
                // Yes
                if matches!(
                    self.tools[tool_idx].definition().permission,
                    Permission::AskOnce
                ) {
                    self.ask_once_memory.insert(tc.name.clone(), true);
                }
                if self.config.log_enabled {
                    eprintln!("[rai] {}: {}  ✓", tc.name, match_target);
                }
                match self.tools[tool_idx].execute(&tc.arguments) {
                    Ok(output) => Ok(crate::tools::ToolResult {
                        tool_call_id: tc.id.clone(),
                        output,
                        success: true,
                    }),
                    Err(e) => Ok(crate::tools::ToolResult {
                        tool_call_id: tc.id.clone(),
                        output: format!("Error: {}", e),
                        success: false,
                    }),
                }
            }
            Some(1) | None => {
                // No
                if matches!(
                    self.tools[tool_idx].definition().permission,
                    Permission::AskOnce
                ) {
                    self.ask_once_memory.insert(tc.name.clone(), false);
                }
                if self.config.log_enabled {
                    eprintln!("[rai] {}: {}  ✗ (user denied)", tc.name, match_target);
                }
                Ok(crate::tools::ToolResult {
                    tool_call_id: tc.id.clone(),
                    output: "Denied by user.".to_string(),
                    success: false,
                })
            }
            Some(2) => {
                // Edit
                let edited: String = Input::new()
                    .with_prompt("Edit command")
                    .default(match_target.clone())
                    .interact_text()?;

                let mut edited_args = tc.arguments.clone();
                match tc.name.as_str() {
                    "shell" => edited_args["command"] = serde_json::Value::String(edited.clone()),
                    "read_file" | "write_file" | "list_dir" | "file_read" | "file_write"
                    | "file_append" | "file_edit" => {
                        edited_args["path"] = serde_json::Value::String(edited.clone())
                    }
                    "http_get" | "http_request" | "web_fetch" => {
                        edited_args["url"] = serde_json::Value::String(edited.clone())
                    }
                    "whois" => edited_args["domain"] = serde_json::Value::String(edited.clone()),
                    "web_search" => {
                        edited_args["query"] = serde_json::Value::String(edited.clone())
                    }
                    "git_operations" => {
                        edited_args["operation"] = serde_json::Value::String(edited.clone())
                    }
                    _ => edited_args["command"] = serde_json::Value::String(edited.clone()),
                }

                if let Some(reason) = check_global_blocklist(&edited, &self.config.blocked_patterns)
                {
                    if self.config.log_enabled {
                        eprintln!("[rai] Edited command also blocked: {}", reason);
                    }
                    return Ok(crate::tools::ToolResult {
                        tool_call_id: tc.id.clone(),
                        output: format!("Blocked: {}", reason),
                        success: false,
                    });
                }

                if self.config.log_enabled {
                    eprintln!("[rai] {}: {}  ✓ (edited)", tc.name, edited);
                }
                match self.tools[tool_idx].execute(&edited_args) {
                    Ok(output) => Ok(crate::tools::ToolResult {
                        tool_call_id: tc.id.clone(),
                        output,
                        success: true,
                    }),
                    Err(e) => Ok(crate::tools::ToolResult {
                        tool_call_id: tc.id.clone(),
                        output: format!("Error: {}", e),
                        success: false,
                    }),
                }
            }
            Some(3) => {
                // Always
                self.config.auto_approve = true;
                if self.config.log_enabled {
                    eprintln!("[rai] Auto-approving all remaining tool calls this session.");
                    eprintln!("[rai] {}: {}  ✓", tc.name, match_target);
                }
                match self.tools[tool_idx].execute(&tc.arguments) {
                    Ok(output) => Ok(crate::tools::ToolResult {
                        tool_call_id: tc.id.clone(),
                        output,
                        success: true,
                    }),
                    Err(e) => Ok(crate::tools::ToolResult {
                        tool_call_id: tc.id.clone(),
                        output: format!("Error: {}", e),
                        success: false,
                    }),
                }
            }
            _ => Ok(crate::tools::ToolResult {
                tool_call_id: tc.id.clone(),
                output: "Denied by user.".to_string(),
                success: false,
            }),
        }
    }
}

fn build_system_prompt() -> String {
    let os = std::env::consts::OS;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());

    format!(
        r#"You are Rai, a CLI assistant with access to tools.

Rules:
- If you can answer directly from your knowledge, do so without tools.
- If you need real-time data or system information, use the available tools.
- Keep final answers short and clear.
- Prefer `web_search` for discovery and `web_fetch` for page content.
- For domain registration lookups, prefer the `whois` tool.
- Prefer the most specific tool (e.g., `read_file` over `shell cat`).
- For shell commands: use simple, portable commands when possible.
- Never run destructive commands (rm -rf, drop table, etc.).
- If a tool call is rejected, explain what you needed and suggest alternatives.
- Keep tool usage minimal — only call tools when necessary.

Environment:
- OS: {}
- Working directory: {}"#,
        os, cwd
    )
}
