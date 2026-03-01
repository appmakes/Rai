use super::{builtin_tools, Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use serde_json::{json, Value};

pub struct LsToolsTool;

impl Tool for LsToolsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ls_tools".to_string(),
            description: "List available built-in tools with permissions and descriptions."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            permission: Permission::Allow,
        }
    }

    fn execute(&self, _args: &Value) -> Result<String> {
        let mut definitions = builtin_tools()
            .into_iter()
            .map(|tool| tool.definition())
            .collect::<Vec<_>>();
        definitions.sort_by(|left, right| left.name.cmp(&right.name));

        let mut output = format!("Available tools ({}):", definitions.len());
        for (index, tool) in definitions.into_iter().enumerate() {
            output.push_str(&format!(
                "\n{}. {} [{}] - {}",
                index + 1,
                tool.name,
                tool.permission,
                tool.description
            ));
        }
        Ok(output)
    }

    fn match_target(&self, _args: &Value) -> String {
        "available tools".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{LsToolsTool, Tool};
    use serde_json::json;

    #[test]
    fn lists_builtin_tools_including_itself() {
        let output = LsToolsTool
            .execute(&json!({}))
            .expect("ls_tools should execute");
        assert!(output.contains("ls_tools"));
        assert!(output.contains("file_read"));
        assert!(output.contains("web_search"));
    }
}
