pub mod builtin;

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
