use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::time::Duration;

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

        let body = run_blocking_get(&url, "Web search failed")?;
        let response_json: Value = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("Invalid response JSON: {}", e))?;

        Ok(format_duckduckgo_results(&response_json, query, count))
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
            .send()
            .map_err(|e| anyhow::anyhow!("{}: {}", error_prefix_owned, e))?;
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

#[cfg(test)]
mod tests {
    use super::{format_duckduckgo_results, parse_count, url_encode_component};
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
}
