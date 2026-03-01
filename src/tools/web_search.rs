use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use regex::Regex;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::sync::OnceLock;
use std::time::Duration;

pub struct WebSearchTool;
const NO_WEB_RESULTS: &str = "No web results found.";

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

        let body = run_blocking_get(&url, "Web search failed")?;
        let response_json: Value = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("Invalid response JSON: {}", e))?;
        let primary = format_duckduckgo_results(&response_json, query, count);
        if primary != NO_WEB_RESULTS {
            return Ok(primary);
        }

        // Fallback: DDG instant-answer API is often sparse for generic/fresh queries.
        // Use DDG HTML search and parse top web links.
        let fallback_html =
            run_blocking_duckduckgo_html_search(query, "Web search fallback failed");
        if let Ok(html) = fallback_html {
            let fallback = format_duckduckgo_html_results(&html, query, count);
            if fallback != NO_WEB_RESULTS {
                return Ok(fallback);
            }
        }

        Ok(primary)
    }

    fn match_target(&self, args: &Value) -> String {
        args["query"].as_str().unwrap_or("").to_string()
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
            .header("User-Agent", "Rai/0.1 (+https://github.com/appmakes/Rai)")
            .send()
            .map_err(|e| anyhow::anyhow!("{}: {}", error_prefix_owned, e))?;
        if !response.status().is_success() {
            anyhow::bail!("{}: HTTP {}", error_prefix_owned, response.status());
        }
        response
            .text()
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("HTTP worker thread panicked"))?
}

fn run_blocking_duckduckgo_html_search(query: &str, error_prefix: &str) -> Result<String> {
    let query_owned = query.to_string();
    let error_prefix_owned = error_prefix.to_string();
    std::thread::spawn(move || -> Result<String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow::anyhow!("{}: {}", error_prefix_owned, e))?;
        let form_body = format!("q={}", url_encode_component(&query_owned));
        let response = client
            .post("https://html.duckduckgo.com/html/")
            .header("User-Agent", "Rai/0.1 (+https://github.com/appmakes/Rai)")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form_body)
            .send()
            .map_err(|e| anyhow::anyhow!("{}: {}", error_prefix_owned, e))?;
        if !response.status().is_success() {
            anyhow::bail!("{}: HTTP {}", error_prefix_owned, response.status());
        }
        response
            .text()
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("HTTP worker thread panicked"))?
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
        return NO_WEB_RESULTS.to_string();
    }

    format_search_results(query, results, count)
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

fn format_duckduckgo_html_results(html: &str, query: &str, count: usize) -> String {
    let results = parse_duckduckgo_html_results(html, count);
    if results.is_empty() {
        return NO_WEB_RESULTS.to_string();
    }
    format_search_results(query, results, count)
}

fn parse_duckduckgo_html_results(html: &str, count: usize) -> Vec<SearchResult> {
    static RESULT_LINK_RE: OnceLock<Regex> = OnceLock::new();
    static RESULT_SNIPPET_RE: OnceLock<Regex> = OnceLock::new();
    static STRIP_TAG_RE: OnceLock<Regex> = OnceLock::new();
    let link_re = RESULT_LINK_RE.get_or_init(|| {
        Regex::new(
            r#"(?is)<a[^>]*class="[^"]*\bresult__a\b[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#,
        )
        .expect("valid result link regex")
    });
    let snippet_re = RESULT_SNIPPET_RE.get_or_init(|| {
        Regex::new(r#"(?is)<a[^>]*class="[^"]*\bresult__snippet\b[^"]*"[^>]*>(.*?)</a>"#)
            .expect("valid snippet regex")
    });
    let strip_tag_re =
        STRIP_TAG_RE.get_or_init(|| Regex::new(r"(?is)<[^>]+>").expect("valid strip regex"));

    let snippets = snippet_re
        .captures_iter(html)
        .filter_map(|capture| {
            capture
                .get(1)
                .map(|m| clean_html_text(m.as_str(), strip_tag_re))
        })
        .collect::<Vec<_>>();

    let mut results = Vec::new();
    for (index, capture) in link_re.captures_iter(html).enumerate() {
        if results.len() >= count {
            break;
        }
        let Some(url_match) = capture.get(1) else {
            continue;
        };
        let Some(title_match) = capture.get(2) else {
            continue;
        };
        let url = normalize_duckduckgo_result_url(url_match.as_str());
        let title = clean_html_text(title_match.as_str(), strip_tag_re);
        if title.is_empty() || url.is_empty() {
            continue;
        }
        let snippet = snippets
            .get(index)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| title.clone());
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }
    results
}

fn clean_html_text(input: &str, strip_tag_re: &Regex) -> String {
    let stripped = strip_tag_re.replace_all(input, "");
    decode_html_entities(stripped.as_ref())
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_duckduckgo_result_url(raw_url: &str) -> String {
    let maybe_url = if let Some(uddg) = extract_query_param(raw_url, "uddg") {
        percent_decode(uddg)
    } else if raw_url.starts_with("//") {
        format!("https:{}", raw_url)
    } else {
        raw_url.to_string()
    };
    maybe_url.trim().to_string()
}

fn extract_query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let (_, query) = url.split_once('?')?;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(v);
        }
    }
    None
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let h1 = bytes[i + 1] as char;
                let h2 = bytes[i + 2] as char;
                let hex = format!("{}{}", h1, h2);
                if let Ok(value) = u8::from_str_radix(&hex, 16) {
                    decoded.push(value);
                    i += 3;
                    continue;
                }
                decoded.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                decoded.push(b' ');
                i += 1;
            }
            byte => {
                decoded.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).to_string()
}

fn format_search_results(query: &str, results: Vec<SearchResult>, count: usize) -> String {
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

#[cfg(test)]
mod tests {
    use super::{
        format_duckduckgo_results, normalize_duckduckgo_result_url, parse_count,
        parse_duckduckgo_html_results, url_encode_component,
    };
    use serde_json::{json, Value};

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
    fn parse_count_clamps_range() {
        assert_eq!(parse_count(Some(&Value::from(0))), 1);
        assert_eq!(parse_count(Some(&Value::from(99))), 10);
        assert_eq!(parse_count(None), 5);
    }

    #[test]
    fn url_encoder_handles_spaces_and_symbols() {
        assert_eq!(url_encode_component("hello world"), "hello+world");
        assert_eq!(url_encode_component("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn html_parser_extracts_result_links_and_snippets() {
        let html = r#"
        <html><body>
          <a class="result__a" href="https://example.com/weather">Weather in Shanghai</a>
          <a class="result__snippet" href="https://example.com/weather">Current weather details</a>
        </body></html>
        "#;
        let results = parse_duckduckgo_html_results(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Weather in Shanghai");
        assert_eq!(results[0].url, "https://example.com/weather");
        assert!(results[0].snippet.contains("Current weather"));
    }

    #[test]
    fn normalize_ddg_redirect_url_uses_uddg_param() {
        let redirected = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Ffoo%3Fa%3D1&rut=123";
        let normalized = normalize_duckduckgo_result_url(redirected);
        assert_eq!(normalized, "https://example.com/foo?a=1");
    }
}
