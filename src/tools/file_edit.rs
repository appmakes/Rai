use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use serde_json::{json, Value};

pub struct FileEditTool;

impl Tool for FileEditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_edit".to_string(),
            description: "Find and replace text in a file.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to edit"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Text to find in the file"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "Replacement text"
                    }
                },
                "required": ["path", "old_text", "new_text"]
            }),
            permission: Permission::Ask,
        }
    }

    fn execute(&self, args: &Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let old_text = args["old_text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'old_text' argument"))?;
        let new_text = args["new_text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_text' argument"))?;

        if old_text.is_empty() {
            anyhow::bail!("old_text must not be empty");
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", path, e))?;
        if !content.contains(old_text) {
            anyhow::bail!("old_text not found in file");
        }

        let updated = content.replacen(old_text, new_text, 1);
        std::fs::write(path, updated)
            .map_err(|e| anyhow::anyhow!("Failed to write file '{}': {}", path, e))?;

        Ok(format!(
            "Replaced {} bytes with {} bytes in {}",
            old_text.len(),
            new_text.len(),
            path
        ))
    }

    fn match_target(&self, args: &Value) -> String {
        args["path"].as_str().unwrap_or("").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{FileEditTool, Tool};
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_file_path(filename: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("rai-file-edit-{}-{}", nonce, filename))
    }

    #[test]
    fn file_edit_replaces_first_occurrence() {
        let path = tmp_file_path("edit.txt");
        fs::write(&path, "aaa bbb aaa").expect("write temp file should succeed");
        let tool = FileEditTool;
        let args = json!({
            "path": path.to_string_lossy(),
            "old_text": "aaa",
            "new_text": "ccc"
        });
        let output = tool.execute(&args).expect("file_edit should succeed");
        assert!(output.contains("Replaced"));
        let actual = fs::read_to_string(&path).expect("read temp file should succeed");
        assert_eq!(actual, "ccc bbb aaa");
        let _ = fs::remove_file(&path);
    }
}
