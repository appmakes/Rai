pub mod builtin;
pub mod extended;

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
        Box::new(builtin::ShellTool),
        Box::new(builtin::ReadFileTool),
        Box::new(builtin::WriteFileTool),
        Box::new(builtin::ListDirTool),
        Box::new(builtin::HttpGetTool),
        Box::new(builtin::WhoisTool),
        Box::new(extended::FileReadTool),
        Box::new(extended::FileWriteTool),
        Box::new(extended::FileAppendTool),
        Box::new(extended::FileEditTool),
        Box::new(extended::HttpRequestTool),
        Box::new(extended::WebFetchTool),
        Box::new(extended::WebSearchTool),
        Box::new(extended::GitOperationsTool),
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
    }
}
