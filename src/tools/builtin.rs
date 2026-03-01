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

// ─── whois ───────────────────────────────────────────────────────────────────

pub struct WhoisTool;

impl Tool for WhoisTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "whois".to_string(),
            description: "Lookup domain registration data using RDAP (WHOIS-like output)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "Domain name to lookup (e.g., google.com)"
                    }
                },
                "required": ["domain"]
            }),
            permission: Permission::Allow,
        }
    }

    fn execute(&self, args: &Value) -> Result<String> {
        let raw_domain = args["domain"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'domain' argument"))?;
        let domain = normalize_domain_input(raw_domain)?;

        let mut last_http_error: Option<(u16, String, String)> = None;
        for url in candidate_rdap_urls(&domain) {
            let (status_code, reason, body) = run_blocking_get(&url, "WHOIS request failed")?;
            if status_code >= 400 {
                last_http_error = Some((status_code, reason, body));
                continue;
            }

            let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            if parsed.is_null() {
                return Ok(truncate_output(&body));
            }

            return Ok(build_whois_summary(&parsed, &domain));
        }

        if let Some((status_code, reason, body)) = last_http_error {
            return Ok(format!(
                "HTTP {} {}\n{}",
                status_code,
                reason,
                truncate_output(&body)
            ));
        }

        anyhow::bail!("WHOIS lookup failed for '{}'", domain)
    }

    fn match_target(&self, args: &Value) -> String {
        args["domain"].as_str().unwrap_or("").to_string()
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

fn normalize_domain_input(input: &str) -> Result<String> {
    let mut domain = input.trim().to_lowercase();
    if let Some(stripped) = domain.strip_prefix("https://") {
        domain = stripped.to_string();
    } else if let Some(stripped) = domain.strip_prefix("http://") {
        domain = stripped.to_string();
    }

    if let Some((head, _)) = domain.split_once('/') {
        domain = head.to_string();
    }
    if let Some((head, _)) = domain.split_once(':') {
        domain = head.to_string();
    }
    if let Some(stripped) = domain.strip_suffix('.') {
        domain = stripped.to_string();
    }

    if domain.is_empty() || domain.contains(char::is_whitespace) {
        anyhow::bail!("Invalid domain: '{}'", input);
    }

    Ok(domain)
}

fn candidate_rdap_urls(domain: &str) -> Vec<String> {
    let tld = domain.rsplit('.').next().unwrap_or_default();
    match tld {
        "com" => vec![
            format!("https://rdap.verisign.com/com/v1/domain/{}", domain),
            format!("https://rdap.org/domain/{}", domain),
        ],
        "net" => vec![
            format!("https://rdap.verisign.com/net/v1/domain/{}", domain),
            format!("https://rdap.org/domain/{}", domain),
        ],
        _ => vec![format!("https://rdap.org/domain/{}", domain)],
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

fn build_whois_summary(rdap: &Value, requested_domain: &str) -> String {
    let domain = rdap["ldhName"].as_str().unwrap_or(requested_domain);
    let mut lines = vec![format!("Domain: {}", domain)];

    if let Some(registrar) = extract_registrar(rdap) {
        lines.push(format!("Registrar: {}", registrar));
    }

    if let Some(statuses) = rdap["status"].as_array() {
        let status_values: Vec<&str> = statuses.iter().filter_map(Value::as_str).take(6).collect();
        if !status_values.is_empty() {
            lines.push(format!("Status: {}", status_values.join(", ")));
        }
    }

    if let Some(created) = extract_event_date(rdap, "registration") {
        lines.push(format!("Created: {}", created));
    }
    if let Some(updated) = extract_event_date(rdap, "last changed") {
        lines.push(format!("Updated: {}", updated));
    }
    if let Some(expires) = extract_event_date(rdap, "expiration") {
        lines.push(format!("Expires: {}", expires));
    }

    if let Some(nameservers) = rdap["nameservers"].as_array() {
        let ns_values: Vec<&str> = nameservers
            .iter()
            .filter_map(|ns| ns["ldhName"].as_str())
            .take(6)
            .collect();
        if !ns_values.is_empty() {
            lines.push(format!("Nameservers: {}", ns_values.join(", ")));
        }
    }

    lines.join("\n")
}

fn extract_event_date(rdap: &Value, action: &str) -> Option<String> {
    rdap["events"].as_array().and_then(|events| {
        events
            .iter()
            .find(|event| event["eventAction"].as_str() == Some(action))
            .and_then(|event| event["eventDate"].as_str())
            .map(str::to_string)
    })
}

fn extract_registrar(rdap: &Value) -> Option<String> {
    rdap["entities"].as_array().and_then(|entities| {
        entities
            .iter()
            .find(|entity| {
                entity["roles"]
                    .as_array()
                    .map(|roles| roles.iter().any(|role| role.as_str() == Some("registrar")))
                    .unwrap_or(false)
            })
            .and_then(|entity| extract_vcard_field(entity, "fn"))
            .or_else(|| {
                entities
                    .iter()
                    .find(|entity| {
                        entity["roles"]
                            .as_array()
                            .map(|roles| {
                                roles.iter().any(|role| role.as_str() == Some("registrar"))
                            })
                            .unwrap_or(false)
                    })
                    .and_then(|entity| extract_vcard_field(entity, "org"))
            })
    })
}

fn extract_vcard_field(entity: &Value, field_name: &str) -> Option<String> {
    let fields = entity["vcardArray"][1].as_array()?;
    fields.iter().find_map(|field| {
        let array = field.as_array()?;
        if array.first()?.as_str()? != field_name {
            return None;
        }
        array.get(3)?.as_str().map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::{candidate_rdap_urls, normalize_domain_input, HttpGetTool, Tool};
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

    #[test]
    fn normalize_domain_strips_scheme_path_and_port() {
        let domain = normalize_domain_input("https://Example.COM:443/path?q=1")
            .expect("domain should normalize");
        assert_eq!(domain, "example.com");
    }

    #[test]
    fn normalize_domain_rejects_empty_values() {
        let result = normalize_domain_input("   ");
        assert!(result.is_err());
    }

    #[test]
    fn candidate_urls_prioritize_verisign_for_com_domains() {
        let urls = candidate_rdap_urls("google.com");
        assert_eq!(
            urls[0],
            "https://rdap.verisign.com/com/v1/domain/google.com"
        );
    }
}
