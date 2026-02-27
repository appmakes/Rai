use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct TaskFrontmatter {
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TaskSection {
    pub name: String,
    pub content: String,
    pub frontmatter: TaskFrontmatter,
}

#[derive(Debug)]
pub struct ParsedTask {
    pub global_frontmatter: TaskFrontmatter,
    pub main_task: Option<TaskSection>,
    pub subtasks: HashMap<String, TaskSection>,
}

impl ParsedTask {
    pub fn get_section(&self, subtask: Option<&str>) -> Result<&TaskSection> {
        match subtask {
            Some(tag) => {
                let normalized = tag.trim_start_matches('#').to_lowercase();
                self.subtasks.get(&normalized).with_context(|| {
                    let available: Vec<&str> =
                        self.subtasks.keys().map(|k| k.as_str()).collect();
                    format!(
                        "Sub-task '{}' not found. Available: {}",
                        normalized,
                        if available.is_empty() {
                            "(none)".to_string()
                        } else {
                            available.join(", ")
                        }
                    )
                })
            }
            None => self
                .main_task
                .as_ref()
                .context("No main task (H1 section) found in task file"),
        }
    }

    pub fn effective_model(&self, subtask: Option<&str>) -> Option<String> {
        if let Ok(section) = self.get_section(subtask) {
            if section.frontmatter.model.is_some() {
                return section.frontmatter.model.clone();
            }
        }
        self.global_frontmatter.model.clone()
    }

    pub fn list_subtasks(&self) -> Vec<&str> {
        self.subtasks.keys().map(|k| k.as_str()).collect()
    }
}

fn parse_frontmatter_yaml(yaml_str: &str) -> TaskFrontmatter {
    let value: serde_yaml::Value = match serde_yaml::from_str(yaml_str) {
        Ok(v) => v,
        Err(_) => return TaskFrontmatter::default(),
    };

    let model = value
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let temperature = value.get("temperature").and_then(|v| v.as_f64());
    let args = value
        .get("args")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    TaskFrontmatter {
        model,
        temperature,
        args,
    }
}

fn extract_frontmatter(content: &str) -> (TaskFrontmatter, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (TaskFrontmatter::default(), content.to_string());
    }

    let after_opening = &trimmed[3..];
    let after_opening = after_opening.strip_prefix('\n').unwrap_or(after_opening);
    if let Some(end_pos) = after_opening.find("\n---") {
        let yaml_content = &after_opening[..end_pos];
        let rest_start = end_pos + 4; // skip "\n---"
        let rest = &after_opening[rest_start..];
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        (parse_frontmatter_yaml(yaml_content), rest.to_string())
    } else {
        (TaskFrontmatter::default(), content.to_string())
    }
}

struct RawSection {
    level: u8,
    name: String,
    body: String,
}

fn split_into_sections(body: &str) -> Vec<RawSection> {
    let header_re = Regex::new(r"(?m)^(#{1,2})\s+(.+)$").unwrap();

    let mut sections = Vec::new();
    let mut matches: Vec<(usize, u8, String)> = Vec::new();

    for cap in header_re.captures_iter(body) {
        let full_match = cap.get(0).unwrap();
        let level = cap[1].len() as u8;
        let name = cap[2].trim().to_string();
        matches.push((full_match.start(), level, name));
    }

    for (i, (start, level, name)) in matches.iter().enumerate() {
        let content_start = body[*start..]
            .find('\n')
            .map(|p| start + p + 1)
            .unwrap_or(body.len());

        let content_end = if i + 1 < matches.len() {
            matches[i + 1].0
        } else {
            body.len()
        };

        let section_body = if content_start <= content_end {
            body[content_start..content_end].to_string()
        } else {
            String::new()
        };

        sections.push(RawSection {
            level: *level,
            name: name.clone(),
            body: section_body,
        });
    }

    sections
}

pub fn parse_task_file(path: &Path) -> Result<ParsedTask> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read task file: {:?}", path))?;
    parse_task_string(&content)
}

pub fn parse_task_string(content: &str) -> Result<ParsedTask> {
    let (global_frontmatter, body) = extract_frontmatter(content);
    let raw_sections = split_into_sections(&body);

    let mut main_task: Option<TaskSection> = None;
    let mut subtasks: HashMap<String, TaskSection> = HashMap::new();

    for section in raw_sections {
        let (section_fm, clean_content) = extract_frontmatter(&section.body);
        let task_section = TaskSection {
            name: section.name.clone(),
            content: clean_content.trim().to_string(),
            frontmatter: section_fm,
        };

        match section.level {
            1 => {
                if main_task.is_none() {
                    main_task = Some(task_section);
                }
            }
            2 => {
                subtasks.insert(section.name.to_lowercase(), task_section);
            }
            _ => {}
        }
    }

    Ok(ParsedTask {
        global_frontmatter,
        main_task,
        subtasks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_task() {
        let content = r#"---
model: gpt-4o
temperature: 0.7
args:
  - filename
---

# Code Review
Review the following code file: {{ filename }}.
"#;
        let parsed = parse_task_string(content).unwrap();
        assert_eq!(parsed.global_frontmatter.model, Some("gpt-4o".to_string()));
        assert_eq!(parsed.global_frontmatter.temperature, Some(0.7));
        assert_eq!(parsed.global_frontmatter.args, vec!["filename"]);
        assert!(parsed.main_task.is_some());
        let main = parsed.main_task.unwrap();
        assert_eq!(main.name, "Code Review");
        assert!(main.content.contains("{{ filename }}"));
    }

    #[test]
    fn test_parse_with_subtasks() {
        let content = r#"---
model: gpt-4o
args:
  - filename
---

# Code Review
Review {{ filename }}.

## security
Focus on security in {{ filename }}.

## refactor
Suggest refactoring for {{ filename }}.
"#;
        let parsed = parse_task_string(content).unwrap();
        assert!(parsed.main_task.is_some());
        assert_eq!(parsed.subtasks.len(), 2);
        assert!(parsed.subtasks.contains_key("security"));
        assert!(parsed.subtasks.contains_key("refactor"));
    }

    #[test]
    fn test_parse_subtask_frontmatter() {
        let content = r#"---
model: gpt-4o
args:
  - filename
  - language
---

# Code Generation
Generate a {{ language }} code snippet that reads {{ filename }}.

## test
---
args:
  - test_framework
---
Write a unit test using {{ test_framework }} for {{ filename }}.
"#;
        let parsed = parse_task_string(content).unwrap();
        let test_section = parsed.subtasks.get("test").unwrap();
        assert_eq!(test_section.frontmatter.args, vec!["test_framework"]);
        assert!(test_section.content.contains("{{ test_framework }}"));
    }

    #[test]
    fn test_no_frontmatter() {
        let content = "# Simple Task\nJust do the thing.\n";
        let parsed = parse_task_string(content).unwrap();
        assert!(parsed.global_frontmatter.model.is_none());
        assert!(parsed.main_task.is_some());
        assert_eq!(
            parsed.main_task.as_ref().unwrap().content,
            "Just do the thing."
        );
    }

    #[test]
    fn test_get_section() {
        let content = r#"# Main
Main content.

## sub1
Sub1 content.
"#;
        let parsed = parse_task_string(content).unwrap();
        assert!(parsed.get_section(None).is_ok());
        assert!(parsed.get_section(Some("#sub1")).is_ok());
        assert!(parsed.get_section(Some("sub1")).is_ok());
        assert!(parsed.get_section(Some("#nonexistent")).is_err());
    }

    #[test]
    fn test_effective_model() {
        let content = r#"---
model: gpt-4o
---

# Main
Main content.

## sub1
Sub1 content.
"#;
        let parsed = parse_task_string(content).unwrap();
        assert_eq!(parsed.effective_model(None), Some("gpt-4o".to_string()));
        assert_eq!(
            parsed.effective_model(Some("sub1")),
            Some("gpt-4o".to_string())
        );
    }

    #[test]
    fn test_list_subtasks() {
        let content = r#"# Main
Main.

## alpha
Alpha.

## beta
Beta.
"#;
        let parsed = parse_task_string(content).unwrap();
        let mut subs = parsed.list_subtasks();
        subs.sort();
        assert_eq!(subs, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_subtask_model_override() {
        let content = r#"---
model: gpt-4o
---

# Main
Main.

## special
---
model: claude-3-opus
---
Special content.
"#;
        let parsed = parse_task_string(content).unwrap();
        assert_eq!(parsed.effective_model(None), Some("gpt-4o".to_string()));
        assert_eq!(
            parsed.effective_model(Some("special")),
            Some("claude-3-opus".to_string())
        );
    }

    #[test]
    fn test_parse_demo_task() {
        let content = r#"---
model: gpt-4o
temperature: 0.5
---

# Code Review
I need you to act as a senior software engineer. Please review the following code file: `{{ filename }}`.

## security
I need you to act as a security expert. Please analyze the file `{{ filename }}` specifically for security vulnerabilities.

## refactor
I want to improve the quality of the code in `{{ filename }}`.

## docs
Please generate comprehensive documentation for the code in `{{ filename }}`.
"#;
        let parsed = parse_task_string(content).unwrap();
        assert_eq!(parsed.global_frontmatter.model, Some("gpt-4o".to_string()));
        assert!(parsed.main_task.is_some());
        assert_eq!(parsed.subtasks.len(), 3);
        assert!(parsed.subtasks.contains_key("security"));
        assert!(parsed.subtasks.contains_key("refactor"));
        assert!(parsed.subtasks.contains_key("docs"));
    }
}
