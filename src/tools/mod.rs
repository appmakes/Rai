pub mod ask;
pub mod file_append;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod git_operations;
pub mod http_get;
pub mod http_request;
pub mod ls_tools;
pub mod list_dir;
pub mod path_security;
pub mod shell;
pub mod utils;
pub mod web_fetch;
pub mod web_search;

use crate::permission::Permission;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(skip)]
    pub permission: Permission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub output: String,
    pub success: bool,
}

pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    fn execute(&self, args: &Value) -> Result<String>;

    /// The string to match permission patterns against.
    fn match_target(&self, args: &Value) -> String;
}

/// Wraps a tool to override its permission level from config.
struct PermissionOverride {
    inner: Box<dyn Tool>,
    permission: Permission,
}

impl Tool for PermissionOverride {
    fn definition(&self) -> ToolDefinition {
        let mut def = self.inner.definition();
        def.permission = self.permission.clone();
        def
    }

    fn execute(&self, args: &Value) -> Result<String> {
        self.inner.execute(args)
    }

    fn match_target(&self, args: &Value) -> String {
        self.inner.match_target(args)
    }
}

/// Parse a TOML value into a Permission.
///
/// Accepts two formats:
///   String  → "allow", "ask", "deny", "ask_once"
///   Table   → { mode = "ask", blacklist = [".."], whitelist = [".."] }
///             All three keys are optional.
fn parse_permission_value(value: &toml::Value, tool_name: &str) -> Option<Permission> {
    match value {
        toml::Value::String(s) => match Permission::parse(s) {
            Ok(perm) => Some(perm),
            Err(e) => {
                eprintln!(
                    "[rai] Warning: invalid permission '{}' for tool '{}': {}. Using default.",
                    s, tool_name, e
                );
                None
            }
        },
        toml::Value::Table(tbl) => {
            let blacklist: Vec<String> = tbl
                .get("blacklist")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let whitelist: Vec<String> = tbl
                .get("whitelist")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let mode = tbl
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // If only mode is set (no blacklist/whitelist), use simple permission.
            if blacklist.is_empty() && whitelist.is_empty() {
                if mode.is_empty() {
                    eprintln!(
                        "[rai] Warning: empty table for tool '{}'. Expected {{ mode, blacklist, whitelist }}. Using default.",
                        tool_name
                    );
                    return None;
                }
                return match Permission::parse(&mode) {
                    Ok(perm) => Some(perm),
                    Err(e) => {
                        eprintln!(
                            "[rai] Warning: invalid mode '{}' for tool '{}': {}. Using default.",
                            mode, tool_name, e
                        );
                        None
                    }
                };
            }

            Some(Permission::Rules {
                blacklist,
                whitelist,
                mode,
            })
        }
        _ => {
            eprintln!(
                "[rai] Warning: unsupported value type for tool '{}'. Expected string or table. Using default.",
                tool_name
            );
            None
        }
    }
}

/// Apply per-tool permission overrides from config to the tool list.
pub fn apply_tool_permissions(
    tools: &mut Vec<Box<dyn Tool>>,
    overrides: &std::collections::HashMap<String, toml::Value>,
) {
    if overrides.is_empty() {
        return;
    }
    let mut new_tools: Vec<Box<dyn Tool>> = Vec::with_capacity(tools.len());
    for tool in tools.drain(..) {
        let name = tool.definition().name;
        if let Some(value) = overrides.get(&name) {
            if let Some(perm) = parse_permission_value(value, &name) {
                new_tools.push(Box::new(PermissionOverride {
                    inner: tool,
                    permission: perm,
                }));
            } else {
                new_tools.push(tool);
            }
        } else {
            new_tools.push(tool);
        }
    }
    *tools = new_tools;
}

pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(shell::ShellTool),
        Box::new(ls_tools::LsToolsTool),
        Box::new(list_dir::ListDirTool),
        Box::new(http_get::HttpGetTool),
        Box::new(file_read::FileReadTool),
        Box::new(file_write::FileWriteTool),
        Box::new(file_append::FileAppendTool),
        Box::new(file_edit::FileEditTool),
        Box::new(http_request::HttpRequestTool),
        Box::new(web_fetch::WebFetchTool),
        Box::new(web_search::WebSearchTool),
        Box::new(git_operations::GitOperationsTool),
        Box::new(ask::AskTool),
    ]
}

/// Convert tool definitions to the OpenAI-compatible JSON format for the API.
pub fn tools_to_api_json(tools: &[ToolDefinition]) -> Value {
    let tool_list: Vec<Value> = tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            })
        })
        .collect();
    Value::Array(tool_list)
}

#[cfg(test)]
mod tests {
    use super::builtin_tools;

    #[test]
    fn builtin_toolset_includes_nullclaw_compat_tools() {
        let names = builtin_tools()
            .into_iter()
            .map(|tool| tool.definition().name)
            .collect::<Vec<_>>();

        for expected in [
            "shell",
            "ls_tools",
            "list_dir",
            "http_get",
            "file_read",
            "file_write",
            "file_append",
            "file_edit",
            "http_request",
            "web_fetch",
            "web_search",
            "git_operations",
            "ask",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "missing tool '{}'",
                expected
            );
        }
        assert!(
            !names.iter().any(|name| name == "whois"),
            "whois tool should not be in common-purpose registry"
        );
    }
}
