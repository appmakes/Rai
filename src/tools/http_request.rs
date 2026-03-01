use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use reqwest::blocking::Client;
use reqwest::header::{HeaderName, HeaderValue};
use serde_json::{json, Value};
use std::time::Duration;

use crate::tools::utils::truncate_output;

pub struct HttpRequestTool;

impl Tool for HttpRequestTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "http_request".to_string(),
            description: "Make HTTP requests (GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "HTTP or HTTPS URL to request"
                    },
                    "method": {
                        "type": "string",
                        "description": "HTTP method (default GET)"
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional request headers"
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional request body"
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
        if !url.starts_with("http://") && !url.starts_with("https://") {
            anyhow::bail!("Only http:// and https:// URLs are allowed");
        }

        let method = args["method"]
            .as_str()
            .unwrap_or("GET")
            .to_ascii_uppercase();
        ensure_http_method_allowed(&method)?;

        let headers = parse_header_pairs(args.get("headers"));
        let body = args.get("body").and_then(Value::as_str).map(str::to_string);

        let (status_code, reason, response_body) =
            run_blocking_request(&method, url, headers.clone(), body, "HTTP request failed")?;

        let redacted_headers = render_headers_for_display(&headers);
        let body_display = truncate_output(&response_body);
        let output = if redacted_headers.is_empty() {
            format!(
                "Status: {}\n\nResponse Body:\n{}",
                status_code, body_display
            )
        } else {
            format!(
                "Status: {}\nRequest Headers: {}\n\nResponse Body:\n{}",
                status_code, redacted_headers, body_display
            )
        };

        if status_code >= 400 {
            return Ok(format!("HTTP {} {}\n{}", status_code, reason, output));
        }
        Ok(output)
    }

    fn match_target(&self, args: &Value) -> String {
        args["url"].as_str().unwrap_or("").to_string()
    }
}

fn ensure_http_method_allowed(method: &str) -> Result<()> {
    match method {
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" => Ok(()),
        _ => anyhow::bail!("Unsupported HTTP method: {}", method),
    }
}

fn parse_header_pairs(headers_value: Option<&Value>) -> Vec<(String, String)> {
    let Some(Value::Object(headers)) = headers_value else {
        return Vec::new();
    };
    headers
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|v| (key.to_string(), v.to_string())))
        .collect()
}

fn is_sensitive_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "authorization",
        "api-key",
        "apikey",
        "token",
        "secret",
        "password",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn render_headers_for_display(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .map(|(key, value)| {
            if is_sensitive_header(key) {
                format!("{}: ***REDACTED***", key)
            } else {
                format!("{}: {}", key, value)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn run_blocking_request(
    method: &str,
    url: &str,
    headers: Vec<(String, String)>,
    body: Option<String>,
    error_prefix: &str,
) -> Result<(u16, String, String)> {
    let method_owned = method.to_string();
    let url_owned = url.to_string();
    let error_prefix_owned = error_prefix.to_string();
    std::thread::spawn(move || -> Result<(u16, String, String)> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow::anyhow!("{}: {}", error_prefix_owned, e))?;

        let method = reqwest::Method::from_bytes(method_owned.as_bytes())
            .map_err(|e| anyhow::anyhow!("Unsupported HTTP method: {}", e))?;
        let mut request = client.request(method, &url_owned);

        if !headers.is_empty() {
            let mut header_map = reqwest::header::HeaderMap::new();
            for (key, value) in headers {
                let Ok(header_name) = HeaderName::from_bytes(key.as_bytes()) else {
                    continue;
                };
                let Ok(header_value) = HeaderValue::from_str(&value) else {
                    continue;
                };
                header_map.insert(header_name, header_value);
            }
            request = request.headers(header_map);
        }

        if let Some(body) = body {
            request = request.body(body);
        }

        let response = request
            .send()
            .map_err(|e| anyhow::anyhow!("{}: {}", error_prefix_owned, e))?;
        let status = response.status();
        let status_code = status.as_u16();
        let reason = status.canonical_reason().unwrap_or("").to_string();
        let response_body = response
            .text()
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))?;
        Ok((status_code, reason, response_body))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("HTTP worker thread panicked"))?
}

#[cfg(test)]
mod tests {
    use super::{HttpRequestTool, Tool};
    use serde_json::json;

    #[test]
    fn http_request_rejects_unsupported_method() {
        let tool = HttpRequestTool;
        let args = json!({
            "url": "https://example.com",
            "method": "CONNECT"
        });
        let err = tool
            .execute(&args)
            .expect_err("unsupported method should fail");
        assert!(err.to_string().contains("Unsupported HTTP method"));
    }
}
