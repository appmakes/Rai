use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use serde_json::{json, Value};

pub struct WriteFileTool;

impl Tool for WriteFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".to_string(),
            description:
                "Write content to a file. Creates the file if it doesn't exist, overwrites if it does."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to write to"
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to write"
                    }
                },
                "required": ["path", "content"]
            }),
            permission: Permission::Ask,
        }
    }

    fn execute(&self, args: &Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

        std::fs::write(path, content)
            .map_err(|e| anyhow::anyhow!("Failed to write file '{}': {}", path, e))?;

        Ok(format!("Written {} bytes to {}", content.len(), path))
    }

    fn match_target(&self, args: &Value) -> String {
        args["path"].as_str().unwrap_or("").to_string()
    }
}
