use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use serde_json::{json, Value};
use std::process::Command;

use crate::tools::utils::truncate_output;

pub struct ShellTool;

impl Tool for ShellTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "shell".to_string(),
            description: "Execute a shell command and return its stdout and stderr.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    }
                },
                "required": ["command"]
            }),
            permission: Permission::Ask,
        }
    }

    fn execute(&self, args: &Value) -> Result<String> {
        let cmd = args["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' argument"))?;

        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to execute command: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("[stderr] ");
            result.push_str(&stderr);
        }
        if result.is_empty() {
            result.push_str("(no output)");
        }

        Ok(truncate_output(&result))
    }

    fn match_target(&self, args: &Value) -> String {
        args["command"].as_str().unwrap_or("").to_string()
    }
}
