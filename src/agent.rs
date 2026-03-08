use crate::permission::{check_permission, check_user_blocklist, Permission, PermissionDecision};
use crate::providers::{Message, Provider, ProviderResponse};
use crate::tools::{Tool, ToolCall, ToolDefinition};
use anyhow::Result;
use dialoguer::{Input, Select};
use std::collections::HashMap;
use std::process::Command;
use tracing::info;

const DEFAULT_MAX_ITERATIONS: usize = 30;
const DEFAULT_MAX_RECOVERABLE_FAIL_RETRIES: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssistantStatus {
    Success,
    SuccessWithWarnings,
    SuccessButCanGoDeeper,
    FailedAndEndTheLoop,
    FailedButNeedFurtherSteps,
}

pub struct AgentConfig {
    pub auto_approve: bool,
    pub max_iterations: usize,
    pub max_recoverable_fail_retries: usize,
    pub blocked_patterns: Vec<String>,
    pub detail_enabled: bool,
    pub think_enabled: bool,
    pub silent_enabled: bool,
    pub plan_enabled: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            auto_approve: false,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            max_recoverable_fail_retries: DEFAULT_MAX_RECOVERABLE_FAIL_RETRIES,
            blocked_patterns: Vec::new(),
            detail_enabled: false,
            think_enabled: false,
            silent_enabled: false,
            plan_enabled: false,
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
        let ask_enabled = self.config.plan_enabled && !self.config.auto_approve && !self.config.silent_enabled;
        let system_prompt = build_system_prompt(self.config.think_enabled, ask_enabled);
        let tool_defs: Vec<ToolDefinition> = self.tools.iter().map(|t| t.definition()).collect();

        let active_tools: Vec<ToolDefinition> = tool_defs
            .iter()
            .filter(|t| !matches!(t.permission, Permission::Deny))
            .filter(|t| ask_enabled || t.name != "ask")
            .cloned()
            .collect();

        let mut messages: Vec<Message> =
            vec![Message::system(&system_prompt), Message::user(prompt)];
        let mut request_number: usize = 0;
        let mut pending_retry_after_failure = false;
        let mut recoverable_fail_retries_used = 0usize;
        let mut saw_any_tool_calls = false;

        for iteration in 0..self.config.max_iterations {
            info!(
                "Agent iteration {}/{}",
                iteration + 1,
                self.config.max_iterations
            );
            request_number += 1;
            if self.config.detail_enabled {
                print_detail_prompt(request_number, &format_messages_for_detail(&messages));
            }

            let response = self
                .provider
                .chat_with_tools(&self.model, &messages, &active_tools)
                .await?;

            match response {
                ProviderResponse::Text(text) => {
                    if self.config.detail_enabled {
                        print_detail_response(request_number, &text);
                    }
                    let status = parse_assistant_status(&text);
                    if is_success_status(status) {
                        pending_retry_after_failure = false;
                        recoverable_fail_retries_used = 0;
                    }
                    if matches!(status, Some(AssistantStatus::FailedButNeedFurtherSteps)) {
                        if self.config.silent_enabled {
                            return Ok(text);
                        }
                        pending_retry_after_failure = true;
                        messages.push(Message::user(
                            "[Retry required]\nYou reported `failed_but_need_further_steps`.\n\
Continue executing additional steps/tools to fulfill the original request. \
Do not stop yet.",
                        ));
                        continue;
                    }
                    if matches!(status, Some(AssistantStatus::FailedAndEndTheLoop))
                        && should_retry_after_failed_terminal_response(
                            saw_any_tool_calls,
                            recoverable_fail_retries_used,
                            self.config.max_recoverable_fail_retries,
                        )
                    {
                        pending_retry_after_failure = true;
                        recoverable_fail_retries_used += 1;
                        messages.push(Message::user(&build_retry_after_failed_terminal_response(
                            &text,
                            recoverable_fail_retries_used,
                            self.config.max_recoverable_fail_retries,
                        )));
                        continue;
                    }
                    if pending_retry_after_failure && status.is_none() {
                        messages.push(Message::user(&build_retry_after_failed_text_response(
                            &text,
                        )));
                        continue;
                    }
                    return Ok(text);
                }
                ProviderResponse::ToolCalls(tool_calls) => {
                    if self.config.detail_enabled {
                        print_detail_response(
                            request_number,
                            &format_tool_calls_for_detail(&tool_calls),
                        );
                    }
                    if !tool_calls.is_empty() {
                        saw_any_tool_calls = true;
                    }
                    messages.push(Message::assistant_tool_calls(&tool_calls));
                    let mut failed_calls: Vec<(String, String)> = Vec::new();
                    let mut success_count = 0usize;

                    for tc in &tool_calls {
                        let result = self.handle_tool_call(tc)?;
                        if result.success {
                            success_count += 1;
                        } else {
                            failed_calls.push((tc.name.clone(), result.output.clone()));
                        }

                        let output = if result.success {
                            result.output.clone()
                        } else {
                            format!("[Tool call failed] {}", result.output)
                        };

                        messages.push(Message::tool_result(&result.tool_call_id, &output));
                    }

                    if !failed_calls.is_empty() {
                        pending_retry_after_failure = true;
                        messages.push(Message::user(&build_retry_after_failed_tool_calls(
                            &failed_calls,
                        )));
                    } else if success_count > 0 {
                        pending_retry_after_failure = false;
                        recoverable_fail_retries_used = 0;
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

        if tc.name == "shell" {
            if let Some(missing_cmd) = find_missing_shell_executable(&tc.arguments) {
                if self.config.detail_enabled {
                    eprintln!(
                        "[rai] {}: {}  ✗ (command not found: {})",
                        tc.name, match_target, missing_cmd
                    );
                }
                return Ok(crate::tools::ToolResult {
                    tool_call_id: tc.id.clone(),
                    output: format!("[stderr] sh: 1: {}: not found", missing_cmd),
                    success: false,
                });
            }
        }

        // Layer 0: Block interactive-only tools unless invoked via `rai plan`
        if tc.name == "ask" && (!self.config.plan_enabled || self.config.auto_approve || self.config.silent_enabled) {
            return Ok(crate::tools::ToolResult {
                tool_call_id: tc.id.clone(),
                output: "The ask tool is only available via `rai plan`. \
                    Make your best judgment and proceed without user input."
                    .to_string(),
                success: false,
            });
        }

        // Layer 1: Global blocklist
        if let Some(reason) = check_user_blocklist(&match_target, &self.config.blocked_patterns) {
            if self.config.detail_enabled {
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
                if self.config.detail_enabled {
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
                if self.config.detail_enabled {
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
            if self.config.detail_enabled {
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
                if self.config.detail_enabled {
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
                if self.config.detail_enabled {
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
                    "list_dir" | "file_read" | "file_write" | "file_append" | "file_edit" => {
                        edited_args["path"] = serde_json::Value::String(edited.clone())
                    }
                    "http_get" | "http_request" | "web_fetch" => {
                        edited_args["url"] = serde_json::Value::String(edited.clone())
                    }
                    "web_search" => {
                        edited_args["query"] = serde_json::Value::String(edited.clone())
                    }
                    "git_operations" => {
                        edited_args["operation"] = serde_json::Value::String(edited.clone())
                    }
                    _ => edited_args["command"] = serde_json::Value::String(edited.clone()),
                }

                if let Some(reason) = check_user_blocklist(&edited, &self.config.blocked_patterns)
                {
                    if self.config.detail_enabled {
                        eprintln!("[rai] Edited command also blocked: {}", reason);
                    }
                    return Ok(crate::tools::ToolResult {
                        tool_call_id: tc.id.clone(),
                        output: format!("Blocked: {}", reason),
                        success: false,
                    });
                }

                if self.config.detail_enabled {
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
                if self.config.detail_enabled {
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

const SYSTEM_PROMPT_TEMPLATE: &str = include_str!("rules/system_prompt.md");

fn build_system_prompt(think_enabled: bool, ask_enabled: bool) -> String {
    let os = std::env::consts::OS;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let think_rule = if think_enabled {
        "- Think mode is enabled. Include your full reasoning chain inside <think>...</think> before the final answer.\n"
    } else {
        ""
    };
    let ask_rule = if ask_enabled {
        "- Use `ask` when you want to clarify the user's intent, confirm a plan, or let them choose between options."
    } else {
        ""
    };
    let (proceeding_state, arguments_field, proceeding_rule) = if ask_enabled {
        (
            " | \"proceeding\"",
            "\n    \"arguments\": {\"prompt\":\"...\", \"options\":[\"...\"]} | \"prompt text\" | null,",
            "- If additional input is still needed, use `\"state\":\"proceeding\"` and provide `arguments`.",
        )
    } else {
        ("", "", "")
    };

    SYSTEM_PROMPT_TEMPLATE
        .replace("{{ask_rule}}", ask_rule)
        .replace("{{think_rule}}", think_rule)
        .replace("{{proceeding_state}}", proceeding_state)
        .replace("{{arguments_field}}", arguments_field)
        .replace("{{proceeding_rule}}", proceeding_rule)
        .replace("{{os}}", os)
        .replace("{{cwd}}", &cwd)
}

fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && atty::is(atty::Stream::Stdout)
}

fn print_detail_prompt(request_number: usize, message: &str) {
    if color_enabled() {
        println!(
            "\x1b[34m[detail][request #{}]\x1b[0m {}",
            request_number, message
        );
    } else {
        println!("[detail][request #{}] {}", request_number, message);
    }
}

fn print_detail_response(response_number: usize, message: &str) {
    if color_enabled() {
        println!(
            "\x1b[33m[detail][response #{}]\x1b[0m {}",
            response_number, message
        );
    } else {
        println!("[detail][response #{}] {}", response_number, message);
    }
}

fn format_messages_for_detail(messages: &[Message]) -> String {
    let mut output = String::new();
    for message in messages {
        match message {
            Message::System { content } => {
                output.push_str("[system]\n");
                output.push_str(content);
                output.push('\n');
            }
            Message::User { content } => {
                output.push_str("[user]\n");
                output.push_str(content);
                output.push('\n');
            }
            Message::AssistantToolCalls {
                content,
                tool_calls,
            } => {
                output.push_str("[assistant_tool_calls]\n");
                if let Some(value) = content {
                    output.push_str(value);
                    output.push('\n');
                }
                for call in tool_calls {
                    output.push_str(&format!(
                        "- {}({}) id={}\n",
                        call.function.name, call.function.arguments, call.id
                    ));
                }
            }
            Message::ToolResult {
                tool_call_id,
                content,
            } => {
                output.push_str("[tool_result]\n");
                output.push_str(&format!("id={}\n{}\n", tool_call_id, content));
            }
        }
    }
    output.trim_end().to_string()
}

fn format_tool_calls_for_detail(tool_calls: &[ToolCall]) -> String {
    if tool_calls.is_empty() {
        return "Tool calls requested: none".to_string();
    }
    let mut output = String::from("Tool calls requested:\n");
    for call in tool_calls {
        output.push_str(&format!(
            "- {}({}) id={}\n",
            call.name, call.arguments, call.id
        ));
    }
    output.trim_end().to_string()
}

fn build_retry_after_failed_tool_calls(failed_calls: &[(String, String)]) -> String {
    let mut note = String::from(
        "[Tool execution update]\nOne or more tool calls failed. Do not stop yet.\n\
Try alternative tools or approaches and continue until the original request is fulfilled \
or the iteration limit is reached.\nFailed calls:",
    );
    for (index, (tool_name, error)) in failed_calls.iter().enumerate() {
        note.push_str(&format!(
            "\n{}. {}: {}",
            index + 1,
            tool_name,
            truncate_for_retry_note(error, 280)
        ));
    }
    let has_missing_command = failed_calls
        .iter()
        .any(|(_, error)| error.to_ascii_lowercase().contains("not found"));
    if has_missing_command {
        note.push_str(
            "\nHint: if a shell command is missing, do NOT end the loop. Use `web_search` for discovery and `web_fetch` to verify details, then continue.",
        );
    }
    note
}

fn build_retry_after_failed_text_response(text: &str) -> String {
    format!(
        "[Retry required]\nSome previous tool calls failed and your last reply did not \
complete the request. Continue trying with alternative tools/sources.\nLast reply:\n{}",
        truncate_for_retry_note(text, 500)
    )
}

fn build_retry_after_failed_terminal_response(
    text: &str,
    retries_used: usize,
    max_retries: usize,
) -> String {
    format!(
        "[Retry required]\nYour last reply used `state: \"fail\"`, but this task may still be recoverable.\n\
Try another tool/source strategy now (for web tasks: choose a different result URL or reduce `web_fetch.max_chars`). \
Use `state: \"proceeding\"` while continuing.\n\
Only return `state: \"fail\"` after alternative attempts are exhausted.\n\
Retry budget: {}/{}.\nLast reply:\n{}",
        retries_used,
        max_retries,
        truncate_for_retry_note(text, 500)
    )
}

fn should_retry_after_failed_terminal_response(
    saw_any_tool_calls: bool,
    retries_used: usize,
    max_retries: usize,
) -> bool {
    saw_any_tool_calls && max_retries > 0 && retries_used < max_retries
}

fn truncate_for_retry_note(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input.chars().take(max_chars).collect::<String>() + "..."
}

fn parse_assistant_status(text: &str) -> Option<AssistantStatus> {
    if let Some(status) = parse_status_from_json_payload(text) {
        return Some(status);
    }

    for line in text.lines().take(12) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((label, raw_value)) = trimmed.split_once(':') else {
            continue;
        };
        let label_normalized = label.trim().to_ascii_lowercase();
        if label_normalized != "status" && label_normalized != "state" {
            continue;
        }
        let normalized = raw_value
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-'], "_");
        let status = match normalized.as_str() {
            "success" => AssistantStatus::Success,
            "fail" => AssistantStatus::FailedAndEndTheLoop,
            "proceeding" => AssistantStatus::FailedButNeedFurtherSteps,
            "success_with_warnings" => AssistantStatus::SuccessWithWarnings,
            "success_but_can_go_deeper" => AssistantStatus::SuccessButCanGoDeeper,
            "failed_and_end_the_loop" => AssistantStatus::FailedAndEndTheLoop,
            "failed_but_need_further_steps" => AssistantStatus::FailedButNeedFurtherSteps,
            _ => continue,
        };
        return Some(status);
    }
    None
}

fn parse_status_from_json_payload(text: &str) -> Option<AssistantStatus> {
    let value = parse_json_like_object(text)?;
    let state = value.get("state")?.as_str()?.trim().to_ascii_lowercase();
    match state.as_str() {
        "success" => Some(AssistantStatus::Success),
        "fail" => Some(AssistantStatus::FailedAndEndTheLoop),
        "proceeding" => Some(AssistantStatus::FailedButNeedFurtherSteps),
        _ => None,
    }
}

fn parse_json_like_object(text: &str) -> Option<serde_json::Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if value.is_object() {
            return Some(value);
        }
    }
    let (first, last) = (trimmed.find('{')?, trimmed.rfind('}')?);
    if last <= first {
        return None;
    }
    let candidate = &trimmed[first..=last];
    let value = serde_json::from_str::<serde_json::Value>(candidate).ok()?;
    if value.is_object() {
        Some(value)
    } else {
        None
    }
}

fn is_success_status(status: Option<AssistantStatus>) -> bool {
    matches!(
        status,
        Some(AssistantStatus::Success)
            | Some(AssistantStatus::SuccessWithWarnings)
            | Some(AssistantStatus::SuccessButCanGoDeeper)
    )
}

fn find_missing_shell_executable(arguments: &serde_json::Value) -> Option<String> {
    let command = arguments.get("command")?.as_str()?;
    let executable = extract_shell_executable(command)?;
    if executable.is_empty() {
        return None;
    }

    let status = Command::new("sh")
        .arg("-c")
        .arg("command -v \"$1\" >/dev/null 2>&1")
        .arg("sh")
        .arg(&executable)
        .status()
        .ok()?;
    if status.success() {
        None
    } else {
        Some(executable)
    }
}

fn extract_shell_executable(command: &str) -> Option<String> {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return None;
    }
    let mut index = 0usize;

    while index < tokens.len() && looks_like_env_assignment(tokens[index]) {
        index += 1;
    }
    if index >= tokens.len() {
        return None;
    }

    match tokens[index] {
        "sudo" => {
            index += 1;
            while index < tokens.len() && tokens[index].starts_with('-') {
                index += 1;
            }
        }
        "env" => {
            index += 1;
            while index < tokens.len()
                && (tokens[index].starts_with('-') || looks_like_env_assignment(tokens[index]))
            {
                index += 1;
            }
        }
        "command" => {
            index += 1;
            while index < tokens.len() && tokens[index].starts_with('-') {
                index += 1;
            }
        }
        "nohup" => {
            index += 1;
        }
        "time" => {
            index += 1;
            while index < tokens.len() && tokens[index].starts_with('-') {
                index += 1;
            }
        }
        _ => {}
    }
    let candidate = tokens.get(index)?;
    if !is_simple_executable_token(candidate) {
        return None;
    }
    Some((*candidate).to_string())
}

fn looks_like_env_assignment(token: &str) -> bool {
    let Some((key, _value)) = token.split_once('=') else {
        return false;
    };
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_simple_executable_token(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/' || c == '+'
        })
}

#[cfg(test)]
mod tests {
    use super::{
        extract_shell_executable, find_missing_shell_executable, parse_assistant_status, Agent,
        AgentConfig, AssistantStatus,
    };
    use crate::permission::Permission;
    use crate::providers::{Message, Provider, ProviderResponse};
    use crate::tools::shell::ShellTool;
    use crate::tools::{Tool, ToolCall, ToolDefinition};
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Copy)]
    enum MockMode {
        EventualSuccess,
        NeverRecovers,
        StatusDrivenRetry,
        RecoverableFailThenSuccess,
        RecoverableFailRetryBudgetExceeded,
    }

    struct MockProvider {
        mode: MockMode,
        call_count: Arc<Mutex<usize>>,
        snapshots: Arc<Mutex<Vec<Vec<Message>>>>,
    }

    impl MockProvider {
        fn new(mode: MockMode) -> (Self, Arc<Mutex<usize>>, Arc<Mutex<Vec<Vec<Message>>>>) {
            let call_count = Arc::new(Mutex::new(0usize));
            let snapshots = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    mode,
                    call_count: Arc::clone(&call_count),
                    snapshots: Arc::clone(&snapshots),
                },
                call_count,
                snapshots,
            )
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn chat(&self, _model: &str, _message: &str) -> Result<String> {
            anyhow::bail!("chat() is not used in these tests")
        }

        async fn chat_with_tools(
            &self,
            _model: &str,
            messages: &[Message],
            _tools: &[ToolDefinition],
        ) -> Result<ProviderResponse> {
            self.snapshots
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push(messages.to_vec());

            let current_call = {
                let mut guard = self
                    .call_count
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                *guard += 1;
                *guard
            };

            let response = match self.mode {
                MockMode::EventualSuccess => match current_call {
                    1 => ProviderResponse::ToolCalls(vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "shell".to_string(),
                        arguments: json!({ "command": "whois typora.io" }),
                    }]),
                    2 => ProviderResponse::Text("Could not finish yet.".to_string()),
                    3 => ProviderResponse::ToolCalls(vec![ToolCall {
                        id: "call-2".to_string(),
                        name: "web_search".to_string(),
                        arguments: json!({ "query": "whois typora.io" }),
                    }]),
                    _ => ProviderResponse::Text("Resolved after fallback.".to_string()),
                },
                MockMode::NeverRecovers => match current_call {
                    1 => ProviderResponse::ToolCalls(vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "shell".to_string(),
                        arguments: json!({ "command": "whois typora.io" }),
                    }]),
                    _ => ProviderResponse::Text("Still unable to complete.".to_string()),
                },
                MockMode::StatusDrivenRetry => match current_call {
                    1 => ProviderResponse::Text(
                        r#"{"state":"proceeding","output":"","description":"Need one more try."}"#
                            .to_string(),
                    ),
                    _ => ProviderResponse::Text(
                        r#"{"state":"success","output":"Done.","description":"Completed."}"#
                            .to_string(),
                    ),
                },
                MockMode::RecoverableFailThenSuccess => match current_call {
                    1 => ProviderResponse::ToolCalls(vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "web_search".to_string(),
                        arguments: json!({ "query": "weather shanghai current" }),
                    }]),
                    2 => ProviderResponse::Text(
                        r#"{"state":"fail","output":"","description":"Unable to extract concise weather information from fetched content. The content was too detailed and not specific to the current weather."}"#
                            .to_string(),
                    ),
                    3 => ProviderResponse::ToolCalls(vec![ToolCall {
                        id: "call-2".to_string(),
                        name: "web_search".to_string(),
                        arguments: json!({ "query": "shanghai weather live now official source" }),
                    }]),
                    _ => ProviderResponse::Text(
                        r#"{"state":"success","output":"Shanghai: 14°C, cloudy.","description":"Completed with alternate source."}"#
                            .to_string(),
                    ),
                },
                MockMode::RecoverableFailRetryBudgetExceeded => match current_call {
                    1 => ProviderResponse::ToolCalls(vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "web_search".to_string(),
                        arguments: json!({ "query": "weather shanghai current" }),
                    }]),
                    _ => ProviderResponse::Text(
                        r#"{"state":"fail","output":"","description":"Unable to extract concise weather information from fetched content. The content was too detailed and not specific to the current weather."}"#
                            .to_string(),
                    ),
                },
            };
            Ok(response)
        }
    }

    struct MockShellTool;

    impl Tool for MockShellTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "shell".to_string(),
                description: "mock shell".to_string(),
                parameters: json!({ "type": "object" }),
                permission: Permission::Allow,
            }
        }

        fn execute(&self, _args: &Value) -> Result<String> {
            anyhow::bail!("[stderr] sh: 1: whois: not found")
        }

        fn match_target(&self, args: &Value) -> String {
            args["command"].as_str().unwrap_or("").to_string()
        }
    }

    struct MockWebSearchTool;

    impl Tool for MockWebSearchTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "web_search".to_string(),
                description: "mock web search".to_string(),
                parameters: json!({ "type": "object" }),
                permission: Permission::Allow,
            }
        }

        fn execute(&self, _args: &Value) -> Result<String> {
            Ok("Mock web result".to_string())
        }

        fn match_target(&self, args: &Value) -> String {
            args["query"].as_str().unwrap_or("").to_string()
        }
    }

    #[tokio::test]
    async fn retries_after_tool_failure_until_success() {
        let (provider, call_count, snapshots) = MockProvider::new(MockMode::EventualSuccess);
        let mut agent = Agent::new(
            Box::new(provider),
            "mock-model".to_string(),
            vec![Box::new(MockShellTool), Box::new(MockWebSearchTool)],
            AgentConfig {
                auto_approve: true,
                max_iterations: 6,
                ..Default::default()
            },
        );

        let output = agent
            .run("whois typora.io")
            .await
            .expect("agent should eventually recover");

        assert_eq!(output, "Resolved after fallback.");
        let calls = *call_count
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(calls, 4, "agent should continue after failed tool calls");

        let recorded = snapshots
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let second_request_messages = &recorded[1];
        let has_failure_followup = second_request_messages.iter().any(|message| {
            matches!(
                message,
                Message::User { content } if content.contains("[Tool execution update]")
            )
        });
        assert!(
            has_failure_followup,
            "agent should append failure follow-up before retrying"
        );
    }

    #[tokio::test]
    async fn fails_with_iteration_limit_when_tools_never_recover() {
        let (provider, call_count, _snapshots) = MockProvider::new(MockMode::NeverRecovers);
        let mut agent = Agent::new(
            Box::new(provider),
            "mock-model".to_string(),
            vec![Box::new(MockShellTool)],
            AgentConfig {
                auto_approve: true,
                max_iterations: 3,
                ..Default::default()
            },
        );

        let error = agent
            .run("whois typora.io")
            .await
            .expect_err("agent should stop at iteration limit");
        assert!(
            error
                .to_string()
                .contains("Agent loop reached maximum iterations"),
            "unexpected error: {}",
            error
        );
        let calls = *call_count
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(calls, 3);
    }

    #[tokio::test]
    async fn retries_when_model_reports_failed_but_need_further_steps() {
        let (provider, call_count, _snapshots) = MockProvider::new(MockMode::StatusDrivenRetry);
        let mut agent = Agent::new(
            Box::new(provider),
            "mock-model".to_string(),
            vec![Box::new(MockWebSearchTool)],
            AgentConfig {
                auto_approve: true,
                max_iterations: 4,
                ..Default::default()
            },
        );

        let output = agent
            .run("weather in Shanghai")
            .await
            .expect("agent should continue and eventually succeed");
        assert!(output.contains(r#""state":"success""#));
        let calls = *call_count
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn retries_when_model_returns_recoverable_failed_status_after_tool_usage() {
        let (provider, call_count, snapshots) =
            MockProvider::new(MockMode::RecoverableFailThenSuccess);
        let mut agent = Agent::new(
            Box::new(provider),
            "mock-model".to_string(),
            vec![Box::new(MockWebSearchTool)],
            AgentConfig {
                auto_approve: true,
                max_iterations: 6,
                max_recoverable_fail_retries: 2,
                ..Default::default()
            },
        );

        let output = agent
            .run("weather in Shanghai")
            .await
            .expect("agent should retry recoverable fail and succeed");
        assert!(output.contains(r#""state":"success""#));
        let calls = *call_count
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(calls, 4);

        let recorded = snapshots
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let third_request_messages = &recorded[2];
        let has_recoverable_fail_followup = third_request_messages.iter().any(|message| {
            matches!(
                message,
                Message::User { content } if content.contains("Your last reply used `state: \"fail\"`")
            )
        });
        assert!(
            has_recoverable_fail_followup,
            "agent should append recoverable-fail follow-up before retrying"
        );
    }

    #[tokio::test]
    async fn stops_retrying_after_recoverable_fail_budget_is_exhausted() {
        let (provider, call_count, _snapshots) =
            MockProvider::new(MockMode::RecoverableFailRetryBudgetExceeded);
        let mut agent = Agent::new(
            Box::new(provider),
            "mock-model".to_string(),
            vec![Box::new(MockWebSearchTool)],
            AgentConfig {
                auto_approve: true,
                max_iterations: 8,
                max_recoverable_fail_retries: 2,
                ..Default::default()
            },
        );

        let output = agent
            .run("weather in Shanghai")
            .await
            .expect("agent should return fail payload once retry budget is exhausted");
        assert!(output.contains(r#""state":"fail""#));
        let calls = *call_count
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(calls, 4);
    }

    #[test]
    fn parses_status_tokens_from_response_lines() {
        assert_eq!(
            parse_assistant_status("STATUS: success_with_warnings\nDone"),
            Some(AssistantStatus::SuccessWithWarnings)
        );
        assert_eq!(
            parse_assistant_status("status: failed_but_need_further_steps\nretry"),
            Some(AssistantStatus::FailedButNeedFurtherSteps)
        );
        assert_eq!(
            parse_assistant_status("state: proceeding\nneed more input"),
            Some(AssistantStatus::FailedButNeedFurtherSteps)
        );
        assert_eq!(
            parse_assistant_status(r#"{"state":"fail","output":"","description":"nope"}"#),
            Some(AssistantStatus::FailedAndEndTheLoop)
        );
        assert_eq!(parse_assistant_status("No status here"), None);
    }

    #[test]
    fn extracts_shell_executable_with_env_and_wrappers() {
        assert_eq!(
            extract_shell_executable("FOO=bar env BAR=baz whois google.com").as_deref(),
            Some("whois")
        );
        assert_eq!(
            extract_shell_executable("sudo -n whois google.com").as_deref(),
            Some("whois")
        );
    }

    #[test]
    fn missing_shell_command_detection_flags_nonexistent_command() {
        let missing = find_missing_shell_executable(
            &json!({ "command": "__rai_cmd_does_not_exist_123456__ --help" }),
        );
        assert_eq!(
            missing.as_deref(),
            Some("__rai_cmd_does_not_exist_123456__")
        );
    }

    #[test]
    fn shell_missing_command_skips_noninteractive_allow_prompt() {
        let (provider, _call_count, _snapshots) = MockProvider::new(MockMode::NeverRecovers);
        let mut agent = Agent::new(
            Box::new(provider),
            "mock-model".to_string(),
            vec![Box::new(ShellTool)],
            AgentConfig {
                auto_approve: false,
                ..Default::default()
            },
        );

        let call = ToolCall {
            id: "shell-missing".to_string(),
            name: "shell".to_string(),
            arguments: json!({ "command": "__rai_cmd_does_not_exist_123456__ --help" }),
        };

        let result = agent
            .handle_tool_call(&call)
            .expect("tool call should return result");
        assert!(!result.success);
        assert!(result.output.contains("not found"));
        assert!(!result.output.contains("non-interactive mode"));
    }
}
