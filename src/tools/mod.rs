pub mod file_append;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod git_operations;
pub mod http_get;
pub mod http_request;
pub mod list_dir;
pub mod read_file;
pub mod shell;
pub mod utils;
pub mod web_fetch;
pub mod web_search;
pub mod write_file;

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

pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(shell::ShellTool),
        Box::new(read_file::ReadFileTool),
        Box::new(write_file::WriteFileTool),
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
            "read_file",
            "write_file",
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
