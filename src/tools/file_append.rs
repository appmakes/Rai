use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write as _;

use crate::tools::path_security::ensure_safe_write_path;

pub struct FileAppendTool;

impl Tool for FileAppendTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_append".to_string(),
            description:
                "Append content to the end of a file (creates the file if it doesn't exist)."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to append to"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to append to the file"
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
        ensure_safe_write_path(path)?;

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| anyhow::anyhow!("Failed to open file '{}': {}", path, e))?;
        file.write_all(content.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to append file '{}': {}", path, e))?;

        Ok(format!("Appended {} bytes to {}", content.len(), path))
    }

    fn match_target(&self, args: &Value) -> String {
        args["path"].as_str().unwrap_or("").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{FileAppendTool, Tool};
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_file_path(filename: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        // Use project target dir instead of system temp to avoid /private/var blocking on macOS.
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
        fs::create_dir_all(&dir).expect("create test-tmp dir");
        dir.join(format!("rai-file-append-{}-{}", nonce, filename))
    }

    #[test]
    fn file_append_creates_and_appends() {
        let path = tmp_file_path("append.txt");
        let tool = FileAppendTool;
        let args1 = json!({
            "path": path.to_string_lossy(),
            "content": "A"
        });
        tool.execute(&args1).expect("first append should succeed");
        let args2 = json!({
            "path": path.to_string_lossy(),
            "content": "B"
        });
        tool.execute(&args2).expect("second append should succeed");
        let actual = fs::read_to_string(&path).expect("read temp file should succeed");
        assert_eq!(actual, "AB");
        let _ = fs::remove_file(&path);
    }
}
