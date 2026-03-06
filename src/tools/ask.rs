use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use dialoguer::Select;
use serde_json::{json, Value};

pub struct AskTool;

impl Tool for AskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ask".to_string(),
            description: "Ask the user a single-select question when you cannot proceed without \
                their explicit input. Only use this tool when you are truly blocked and need the \
                user to choose between concrete options before you can continue. Do NOT use this \
                tool for confirmations, status updates, or when you can make a reasonable decision \
                yourself."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The question to present to the user"
                    },
                    "options": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 2,
                        "description": "List of options for the user to choose from (single select)"
                    }
                },
                "required": ["question", "options"]
            }),
            permission: Permission::Allow,
        }
    }

    fn execute(&self, args: &Value) -> Result<String> {
        let question = args["question"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'question' argument"))?;

        let options: Vec<String> = args["options"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing 'options' argument"))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        if options.len() < 2 {
            anyhow::bail!("At least 2 options are required");
        }

        eprintln!();
        let selection = Select::new()
            .with_prompt(question)
            .items(&options)
            .default(0)
            .interact_opt()?;

        match selection {
            Some(idx) => Ok(options[idx].clone()),
            None => Ok("User cancelled the selection.".to_string()),
        }
    }

    fn match_target(&self, args: &Value) -> String {
        args["question"].as_str().unwrap_or("").to_string()
    }
}
