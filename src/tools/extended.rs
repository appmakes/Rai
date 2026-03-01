use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use regex::Regex;
use reqwest::blocking::Client;
use reqwest::header::{HeaderName, HeaderValue};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::io::Write as _;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

const MAX_OUTPUT_BYTES: usize = 64 * 1024;

// ─── file_read (nullclaw compatibility) ──────────────────────────────────────

pub struct FileReadTool;

impl Tool for FileReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_read".to_string(),
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

// ─── file_write (nullclaw compatibility) ─────────────────────────────────────

pub struct FileWriteTool;

impl Tool for FileWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_write".to_string(),
            description:
                "Write content to a file. Creates the file if it doesn't exist, overwrites if it does."
                    .to_string(),
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

// ─── file_append ──────────────────────────────────────────────────────────────

pub struct FileAppendTool;

impl Tool for FileAppendTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_append".to_string(),
            description: "Append content to the end of a file (creates it if it doesn't exist)."
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
                        "description": "The content to append"
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

// ─── file_edit ────────────────────────────────────────────────────────────────

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

// ─── http_request ─────────────────────────────────────────────────────────────

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

// ─── web_fetch ────────────────────────────────────────────────────────────────

pub struct WebFetchTool;

impl Tool for WebFetchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_fetch".to_string(),
            description: "Fetch a web page and extract readable text content (HTML to text)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "HTTP or HTTPS URL to fetch"
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": "Maximum characters to return (default 50000)"
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

        let max_chars = parse_max_chars(args.get("max_chars"), 50_000);
        let headers = vec![
            (
                "User-Agent".to_string(),
                "rai/0.1 (web_fetch tool)".to_string(),
            ),
            (
                "Accept".to_string(),
                "text/html,application/json,text/plain,*/*".to_string(),
            ),
        ];

        let (_, _, body) = run_blocking_request("GET", url, headers, None, "Web fetch failed")?;
        let extracted = if looks_like_html(&body) {
            html_to_text(&body)
        } else {
            body
        };

        if extracted.len() > max_chars {
            return Ok(format!(
                "{}\n\n[Content truncated at {} chars, total {} chars]",
                &extracted[..max_chars],
                max_chars,
                extracted.len()
            ));
        }

        Ok(extracted)
    }

    fn match_target(&self, args: &Value) -> String {
        args["url"].as_str().unwrap_or("").to_string()
    }
}

// ─── web_search ───────────────────────────────────────────────────────────────

pub struct WebSearchTool;

impl Tool for WebSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web (DuckDuckGo-backed) and return concise result summaries."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of results (1-10, default 5)"
                    },
                    "provider": {
                        "type": "string",
                        "description": "Provider override (supports auto, duckduckgo, ddg)"
                    }
                },
                "required": ["query"]
            }),
            permission: Permission::Allow,
        }
    }

    fn execute(&self, args: &Value) -> Result<String> {
        let query = args["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' argument"))?
            .trim();
        if query.is_empty() {
            anyhow::bail!("'query' must not be empty");
        }

        let provider = args["provider"]
            .as_str()
            .unwrap_or("auto")
            .to_ascii_lowercase();
        if provider != "auto" && provider != "duckduckgo" && provider != "ddg" {
            anyhow::bail!("Invalid web_search provider. Supported: auto, duckduckgo, ddg.");
        }

        let count = parse_count(args.get("count"));
        let encoded_query = url_encode_component(query);
        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
            encoded_query
        );

        let (_, _, body) = run_blocking_request("GET", &url, vec![], None, "Web search failed")?;
        let response_json: Value = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("Invalid response JSON: {}", e))?;

        Ok(format_duckduckgo_results(&response_json, query, count))
    }

    fn match_target(&self, args: &Value) -> String {
        args["query"].as_str().unwrap_or("").to_string()
    }
}

// ─── git_operations ───────────────────────────────────────────────────────────

pub struct GitOperationsTool;

impl Tool for GitOperationsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "git_operations".to_string(),
            description: "Perform structured Git operations (status, diff, log, branch, commit, add, checkout, stash).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "operation": {
                        "type": "string",
                        "enum": ["status", "diff", "log", "branch", "commit", "add", "checkout", "stash"],
                        "description": "Git operation to perform"
                    },
                    "message": {
                        "type": "string",
                        "description": "Commit message (for commit)"
                    },
                    "paths": {
                        "type": "string",
                        "description": "File paths (for add)"
                    },
                    "branch": {
                        "type": "string",
                        "description": "Branch name (for checkout)"
                    },
                    "files": {
                        "type": "string",
                        "description": "File path filter for diff"
                    },
                    "cached": {
                        "type": "boolean",
                        "description": "Show staged changes (diff)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Log entry count (default: 10)"
                    },
                    "action": {
                        "type": "string",
                        "description": "Stash action: push, pop, list"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Absolute repo directory (optional)"
                    }
                },
                "required": ["operation"]
            }),
            permission: Permission::Ask,
        }
    }

    fn execute(&self, args: &Value) -> Result<String> {
        let operation = args["operation"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'operation' argument"))?;

        for field in ["message", "paths", "branch", "files", "action"] {
            if let Some(value) = args.get(field).and_then(Value::as_str) {
                if !sanitize_git_arg(value) {
                    anyhow::bail!("Unsafe git arguments detected");
                }
            }
        }

        let cwd = args.get("cwd").and_then(Value::as_str);
        if let Some(path) = cwd {
            if path.is_empty() || !std::path::Path::new(path).is_absolute() {
                anyhow::bail!("cwd must be an absolute path");
            }
        }

        match operation {
            "status" => run_git_operation(cwd, &["status", "--porcelain=2", "--branch"]),
            "diff" => git_diff(cwd, args),
            "log" => git_log(cwd, args),
            "branch" => run_git_operation(cwd, &["branch", "--format=%(refname:short)|%(HEAD)"]),
            "commit" => git_commit(cwd, args),
            "add" => git_add(cwd, args),
            "checkout" => git_checkout(cwd, args),
            "stash" => git_stash(cwd, args),
            other => anyhow::bail!("Unknown operation: {}", other),
        }
    }

    fn match_target(&self, args: &Value) -> String {
        args["operation"].as_str().unwrap_or("").to_string()
    }
}

// ─── shared helpers ───────────────────────────────────────────────────────────

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

fn parse_max_chars(value: Option<&Value>, default: usize) -> usize {
    let Some(raw) = value.and_then(Value::as_i64) else {
        return default;
    };
    if raw < 100 {
        return 100;
    }
    if raw > 200_000 {
        return 200_000;
    }
    raw as usize
}

fn looks_like_html(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("<html") || lower.contains("<!doctype html") || lower.contains("<body")
}

fn html_to_text(html: &str) -> String {
    static SCRIPT_RE: OnceLock<Regex> = OnceLock::new();
    static STYLE_RE: OnceLock<Regex> = OnceLock::new();
    static BLOCK_RE: OnceLock<Regex> = OnceLock::new();
    static TAG_RE: OnceLock<Regex> = OnceLock::new();

    let script_re = SCRIPT_RE
        .get_or_init(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").expect("valid script regex"));
    let style_re = STYLE_RE
        .get_or_init(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").expect("valid style regex"));
    let block_re = BLOCK_RE.get_or_init(|| {
        Regex::new(r"(?is)</?(p|div|section|article|main|header|footer|nav|aside|li|ul|ol|tr|table|h[1-6]|br|hr)[^>]*>")
            .expect("valid block regex")
    });
    let tag_re = TAG_RE.get_or_init(|| Regex::new(r"(?is)<[^>]+>").expect("valid tag regex"));

    let mut text = script_re.replace_all(html, " ").to_string();
    text = style_re.replace_all(&text, " ").to_string();
    text = block_re.replace_all(&text, "\n").to_string();
    text = tag_re.replace_all(&text, " ").to_string();
    text = decode_common_entities(&text);

    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_common_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

fn parse_count(value: Option<&Value>) -> usize {
    let Some(raw) = value.and_then(Value::as_i64) else {
        return 5;
    };
    if raw < 1 {
        return 1;
    }
    if raw > 10 {
        return 10;
    }
    raw as usize
}

fn url_encode_component(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            _ => {
                let _ = write!(&mut encoded, "%{:02X}", byte);
            }
        }
    }
    encoded
}

#[derive(Debug, Clone)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

fn format_duckduckgo_results(response_json: &Value, query: &str, count: usize) -> String {
    let mut results = Vec::new();

    let abstract_text = response_json["AbstractText"].as_str().unwrap_or("").trim();
    let abstract_url = response_json["AbstractURL"].as_str().unwrap_or("").trim();
    if !abstract_text.is_empty() && !abstract_url.is_empty() {
        let heading = response_json["Heading"].as_str().unwrap_or(query).trim();
        results.push(SearchResult {
            title: heading.to_string(),
            url: abstract_url.to_string(),
            snippet: abstract_text.to_string(),
        });
    }

    collect_related_topics(response_json.get("RelatedTopics"), &mut results, count);

    if results.is_empty() {
        return "No web results found.".to_string();
    }

    let mut output = format!("Results for: {}", query);
    for (index, result) in results.into_iter().take(count).enumerate() {
        output.push_str(&format!(
            "\n{}. {}\n   {}\n   {}",
            index + 1,
            result.title,
            result.url,
            result.snippet
        ));
    }
    output
}

fn collect_related_topics(
    value: Option<&Value>,
    results: &mut Vec<SearchResult>,
    max_count: usize,
) {
    if results.len() >= max_count {
        return;
    }

    let Some(Value::Array(topics)) = value else {
        return;
    };

    for topic in topics {
        if results.len() >= max_count {
            break;
        }

        if let (Some(text), Some(url)) = (
            topic.get("Text").and_then(Value::as_str),
            topic.get("FirstURL").and_then(Value::as_str),
        ) {
            let title = text
                .split(" - ")
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("Result")
                .to_string();
            results.push(SearchResult {
                title,
                url: url.to_string(),
                snippet: text.to_string(),
            });
            continue;
        }

        collect_related_topics(topic.get("Topics"), results, max_count);
    }
}

fn sanitize_git_arg(arg: &str) -> bool {
    let dangerous_prefixes = [
        "--exec=",
        "--upload-pack=",
        "--receive-pack=",
        "--pager=",
        "--editor=",
    ];
    let dangerous_exact = ["--no-verify"];
    let dangerous_substrings = ["$(", "`"];
    let dangerous_chars = ['|', ';', '>'];

    for token in arg.split_whitespace() {
        if dangerous_prefixes
            .iter()
            .any(|prefix| token.to_ascii_lowercase().starts_with(prefix))
        {
            return false;
        }
        if dangerous_exact
            .iter()
            .any(|exact| token.eq_ignore_ascii_case(exact))
        {
            return false;
        }
        if dangerous_substrings
            .iter()
            .any(|needle| token.contains(needle))
        {
            return false;
        }
        if token.chars().any(|ch| dangerous_chars.contains(&ch)) {
            return false;
        }
        if token.eq_ignore_ascii_case("-c") || token.to_ascii_lowercase().starts_with("-c=") {
            return false;
        }
    }

    true
}

fn run_git(cwd: Option<&str>, args: &[String]) -> Result<(bool, String, String)> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute git command: {}", e))?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

fn run_git_operation(cwd: Option<&str>, args: &[&str]) -> Result<String> {
    let string_args = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let (success, stdout, stderr) = run_git(cwd, &string_args)?;
    if !success {
        anyhow::bail!(
            "{}",
            if stderr.trim().is_empty() {
                "Git operation failed".to_string()
            } else {
                stderr
            }
        );
    }
    let output = if stdout.trim().is_empty() {
        "(no output)"
    } else {
        &stdout
    };
    Ok(truncate_output(output))
}

fn git_diff(cwd: Option<&str>, args: &Value) -> Result<String> {
    let cached = args["cached"].as_bool().unwrap_or(false);
    let files = args["files"].as_str().unwrap_or(".");

    let mut cmd_args = vec!["diff".to_string(), "--unified=3".to_string()];
    if cached {
        cmd_args.push("--cached".to_string());
    }
    cmd_args.push("--".to_string());
    cmd_args.push(files.to_string());

    let (success, stdout, stderr) = run_git(cwd, &cmd_args)?;
    if !success {
        anyhow::bail!(
            "{}",
            if stderr.trim().is_empty() {
                "Git diff failed".to_string()
            } else {
                stderr
            }
        );
    }
    let output = if stdout.trim().is_empty() {
        "(no diff output)"
    } else {
        &stdout
    };
    Ok(truncate_output(output))
}

fn git_log(cwd: Option<&str>, args: &Value) -> Result<String> {
    let limit = args["limit"].as_i64().unwrap_or(10).clamp(1, 1000);
    let cmd_args = vec![
        "log".to_string(),
        format!("-{}", limit),
        "--pretty=format:%H|%an|%ae|%ad|%s".to_string(),
        "--date=iso".to_string(),
    ];
    let (success, stdout, stderr) = run_git(cwd, &cmd_args)?;
    if !success {
        anyhow::bail!(
            "{}",
            if stderr.trim().is_empty() {
                "Git log failed".to_string()
            } else {
                stderr
            }
        );
    }
    Ok(truncate_output(if stdout.trim().is_empty() {
        "(no log output)"
    } else {
        &stdout
    }))
}

fn git_commit(cwd: Option<&str>, args: &Value) -> Result<String> {
    let raw_message = args["message"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'message' argument for commit"))?;
    if raw_message.is_empty() {
        anyhow::bail!("Commit message cannot be empty");
    }
    let message = truncate_utf8(raw_message, 2000);
    let cmd_args = vec!["commit".to_string(), "-m".to_string(), message.to_string()];
    let (success, stdout, stderr) = run_git(cwd, &cmd_args)?;
    if !success {
        let msg = if stderr.trim().is_empty() {
            if stdout.trim().is_empty() {
                "Git commit failed".to_string()
            } else {
                stdout
            }
        } else {
            stderr
        };
        anyhow::bail!("{}", msg);
    }
    Ok(format!("Committed: {}", message))
}

fn git_add(cwd: Option<&str>, args: &Value) -> Result<String> {
    let paths = args["paths"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'paths' argument for add"))?;
    let split_paths = paths.split_whitespace().collect::<Vec<_>>();
    if split_paths.is_empty() {
        anyhow::bail!("No paths provided for add");
    }

    let mut cmd_args = vec!["add".to_string(), "--".to_string()];
    cmd_args.extend(split_paths.iter().map(|p| p.to_string()));
    let (success, stdout, stderr) = run_git(cwd, &cmd_args)?;
    if !success {
        let msg = if stderr.trim().is_empty() {
            if stdout.trim().is_empty() {
                "Git add failed".to_string()
            } else {
                stdout
            }
        } else {
            stderr
        };
        anyhow::bail!("{}", msg);
    }
    Ok(format!("Staged: {}", paths))
}

fn git_checkout(cwd: Option<&str>, args: &Value) -> Result<String> {
    let branch = args["branch"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'branch' argument for checkout"))?;
    if branch.contains(';') || branch.contains('|') || branch.contains('`') || branch.contains("$(")
    {
        anyhow::bail!("Branch name contains invalid characters");
    }

    let cmd_args = vec!["checkout".to_string(), branch.to_string()];
    let (success, stdout, stderr) = run_git(cwd, &cmd_args)?;
    if !success {
        let msg = if stderr.trim().is_empty() {
            if stdout.trim().is_empty() {
                "Git checkout failed".to_string()
            } else {
                stdout
            }
        } else {
            stderr
        };
        anyhow::bail!("{}", msg);
    }
    Ok(format!("Switched to branch: {}", branch))
}

fn git_stash(cwd: Option<&str>, args: &Value) -> Result<String> {
    let action = args["action"].as_str().unwrap_or("push");
    let cmd_args = match action {
        "push" | "save" => vec![
            "stash".to_string(),
            "push".to_string(),
            "-m".to_string(),
            "auto-stash".to_string(),
        ],
        "pop" => vec!["stash".to_string(), "pop".to_string()],
        "list" => vec!["stash".to_string(), "list".to_string()],
        _ => anyhow::bail!("Unknown stash action: {}", action),
    };
    let (success, stdout, stderr) = run_git(cwd, &cmd_args)?;
    if !success {
        let msg = if stderr.trim().is_empty() {
            if stdout.trim().is_empty() {
                "Git stash failed".to_string()
            } else {
                stdout
            }
        } else {
            stderr
        };
        anyhow::bail!("{}", msg);
    }
    Ok(truncate_output(if stdout.trim().is_empty() {
        "(no output)"
    } else {
        &stdout
    }))
}

fn truncate_utf8(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }
    let mut idx = max_bytes;
    while idx > 0 && !input.is_char_boundary(idx) {
        idx -= 1;
    }
    &input[..idx]
}

#[cfg(test)]
mod tests {
    use super::{
        format_duckduckgo_results, html_to_text, parse_count, parse_max_chars, sanitize_git_arg,
        url_encode_component, FileAppendTool, FileEditTool, HttpRequestTool, Tool,
    };
    use serde_json::{json, Value};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_file_path(filename: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("rai-extended-tool-{}-{}", nonce, filename))
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

    #[test]
    fn html_to_text_strips_scripts_and_tags() {
        let html = "<html><body><script>alert(1)</script><h1>Title</h1><p>Hello&nbsp;World</p></body></html>";
        let text = html_to_text(html);
        assert!(!text.contains("alert"));
        assert!(text.contains("Title"));
        assert!(text.contains("Hello World"));
    }

    #[test]
    fn duckduckgo_formatter_handles_related_topics() {
        let sample = json!({
            "Heading": "Zig",
            "AbstractText": "",
            "AbstractURL": "",
            "RelatedTopics": [
                { "Text": "Zig - Programming language", "FirstURL": "https://ziglang.org" },
                { "Topics": [
                    { "Text": "Zig docs - Documentation", "FirstURL": "https://ziglang.org/documentation/master/" }
                ]}
            ]
        });
        let formatted = format_duckduckgo_results(&sample, "zig", 5);
        assert!(formatted.contains("Results for: zig"));
        assert!(formatted.contains("https://ziglang.org"));
    }

    #[test]
    fn parse_limits_are_clamped() {
        assert_eq!(parse_max_chars(Some(&Value::from(10)), 50_000), 100);
        assert_eq!(
            parse_max_chars(Some(&Value::from(999_999)), 50_000),
            200_000
        );
        assert_eq!(parse_count(Some(&Value::from(0))), 1);
        assert_eq!(parse_count(Some(&Value::from(99))), 10);
    }

    #[test]
    fn url_encoder_handles_spaces_and_symbols() {
        assert_eq!(url_encode_component("hello world"), "hello+world");
        assert_eq!(url_encode_component("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn git_arg_sanitizer_blocks_dangerous_patterns() {
        assert!(!sanitize_git_arg("$(evil)"));
        assert!(!sanitize_git_arg("--exec=rm -rf /"));
        assert!(!sanitize_git_arg("arg; rm -rf /"));
        assert!(sanitize_git_arg("--cached"));
        assert!(sanitize_git_arg("feature/test"));
    }
}
