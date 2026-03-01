use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use regex::Regex;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

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
        let body = run_blocking_get(url, "Web fetch failed")?;
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

fn run_blocking_get(url: &str, error_prefix: &str) -> Result<String> {
    let url_owned = url.to_string();
    let error_prefix_owned = error_prefix.to_string();
    std::thread::spawn(move || -> Result<String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow::anyhow!("{}: {}", error_prefix_owned, e))?;
        let response = client
            .get(&url_owned)
            .header("User-Agent", "rai/0.1 (web_fetch tool)")
            .header("Accept", "text/html,application/json,text/plain,*/*")
            .send()
            .map_err(|e| anyhow::anyhow!("{}: {}", error_prefix_owned, e))?;
        response
            .text()
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))
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

#[cfg(test)]
mod tests {
    use super::html_to_text;

    #[test]
    fn html_to_text_strips_scripts_and_tags() {
        let html = "<html><body><script>alert(1)</script><h1>Title</h1><p>Hello&nbsp;World</p></body></html>";
        let text = html_to_text(html);
        assert!(!text.contains("alert"));
        assert!(text.contains("Title"));
        assert!(text.contains("Hello World"));
    }
}
