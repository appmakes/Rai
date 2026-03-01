use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use serde_json::{json, Value};

use crate::tools::utils::truncate_output;

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
        let (status_code, reason, body) = run_blocking_get(url, "HTTP request failed")?;

        if status_code >= 400 {
            return Ok(format!(
                "HTTP {} {}\n{}",
                status_code,
                reason,
                truncate_output(&body)
            ));
        }

        Ok(truncate_output(&body))
    }

    fn match_target(&self, args: &Value) -> String {
        args["url"].as_str().unwrap_or("").to_string()
    }
}

fn run_blocking_get(url: &str, request_error_prefix: &str) -> Result<(u16, String, String)> {
    let url_owned = url.to_string();
    let error_prefix = request_error_prefix.to_string();
    std::thread::spawn(move || -> Result<(u16, String, String)> {
        let response = reqwest::blocking::get(&url_owned)
            .map_err(|e| anyhow::anyhow!("{}: {}", error_prefix, e))?;
        let status = response.status();
        let status_code = status.as_u16();
        let reason = status.canonical_reason().unwrap_or("").to_string();
        let body = response
            .text()
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))?;
        Ok((status_code, reason, body))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("HTTP worker thread panicked"))?
}

#[cfg(test)]
mod tests {
    use super::{HttpGetTool, Tool};
    use serde_json::json;

    #[test]
    fn http_get_does_not_panic_inside_tokio_runtime() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should initialize");
        runtime.block_on(async {
            let tool = HttpGetTool;
            let args = json!({ "url": "http://127.0.0.1:1/" });
            let _ = tool.execute(&args);
        });
    }
}
