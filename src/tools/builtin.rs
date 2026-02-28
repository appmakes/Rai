use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use serde_json::{json, Value};
use std::process::Command;

// ─── shell ───────────────────────────────────────────────────────────────────

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

// ─── read_file ───────────────────────────────────────────────────────────────

pub struct ReadFileTool;

impl Tool for ReadFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read the contents of a file and return them as text.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to read"
                    }
                },
                "required": ["path"]
            }),
            permission: Permission::Allow,
        }
    }

    fn execute(&self, args: &Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", path, e))?;

        Ok(truncate_output(&content))
    }

    fn match_target(&self, args: &Value) -> String {
        args["path"].as_str().unwrap_or("").to_string()
    }
}

// ─── write_file ──────────────────────────────────────────────────────────────

pub struct WriteFileTool;

impl Tool for WriteFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write content to a file. Creates the file if it doesn't exist, overwrites if it does.".to_string(),
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

// ─── list_dir ────────────────────────────────────────────────────────────────

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

// ─── http_get ────────────────────────────────────────────────────────────────

pub struct HttpGetTool;

impl Tool for HttpGetTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "http_get".to_string(),
            description: "Fetch a URL via HTTP GET and return the response body.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    }
                },
                "required": ["url"]
            }),
            permission: Permission::Allow,
        }
    }

    fn execute(&self, args: &Value) -> Result<String> {
        let url = args["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' argument"))?;

        let response = reqwest::blocking::get(url)
            .map_err(|e| anyhow::anyhow!("HTTP request failed: {}", e))?;

        let status = response.status();
        let body = response
            .text()
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))?;

        if !status.is_success() {
            return Ok(format!("HTTP {} {}\n{}", status.as_u16(), status.canonical_reason().unwrap_or(""), truncate_output(&body)));
        }

        Ok(truncate_output(&body))
    }

    fn match_target(&self, args: &Value) -> String {
        args["url"].as_str().unwrap_or("").to_string()
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

const MAX_OUTPUT_BYTES: usize = 64 * 1024;

fn truncate_output(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        s.to_string()
    } else {
        let truncated = &s[..MAX_OUTPUT_BYTES];
        format!(
            "{}\n\n[truncated — {} bytes total, showing first {}]",
            truncated,
            s.len(),
            MAX_OUTPUT_BYTES
        )
    }
}
