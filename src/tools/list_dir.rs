use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use serde_json::{json, Value};

pub struct ListDirTool;

impl Tool for ListDirTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_dir".to_string(),
            description: "List files and directories in a given path.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The directory path to list (defaults to '.')"
                    }
                },
                "required": []
            }),
            permission: Permission::Allow,
        }
    }

    fn execute(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or(".");
        let entries = std::fs::read_dir(path)
            .map_err(|e| anyhow::anyhow!("Failed to list directory '{}': {}", path, e))?;

        let mut items: Vec<String> = Vec::new();
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let name = entry.file_name().to_string_lossy().to_string();
            let prefix = if file_type.is_dir() { "d " } else { "  " };
            items.push(format!("{}{}", prefix, name));
        }
        items.sort();

        if items.is_empty() {
            Ok("(empty directory)".to_string())
        } else {
            Ok(items.join("\n"))
        }
    }

    fn match_target(&self, args: &Value) -> String {
        args["path"].as_str().unwrap_or(".").to_string()
    }
}
