use anyhow::Result;
use regex::Regex;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: String,
    pub required: bool,
}

pub fn find_variables(template: &str) -> Vec<String> {
    let re = Regex::new(r"\{\{\s*([a-zA-Z_][a-zA-Z0-9_]*|\d+)\s*\}\}").unwrap();
    let mut seen = HashSet::new();
    let mut vars = Vec::new();
    for cap in re.captures_iter(template) {
        let name = cap[1].to_string();
        if seen.insert(name.clone()) {
            vars.push(name);
        }
    }
    vars
}

pub fn render(template: &str, variables: &HashMap<String, String>) -> Result<String> {
    let re = Regex::new(r"\{\{\s*([a-zA-Z_][a-zA-Z0-9_]*|\d+)\s*\}\}").unwrap();
    let mut missing: Vec<String> = Vec::new();

    let result = re.replace_all(template, |caps: &regex::Captures| {
        let var_name = caps[1].to_string();
        match variables.get(&var_name) {
            Some(value) => value.clone(),
            None => {
                missing.push(var_name.clone());
                format!("{{{{ {} }}}}", var_name)
            }
        }
    });

    if !missing.is_empty() {
        anyhow::bail!("Missing template variable(s): {}", missing.join(", "));
    }

    Ok(result.to_string())
}

pub fn parse_arg_spec(raw: &str) -> Result<ArgSpec> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Invalid empty argument declaration in task frontmatter.");
    }

    let (candidate_name, required) = if let Some(without_optional) = trimmed.strip_suffix('?') {
        (without_optional.trim(), false)
    } else {
        (trimmed, true)
    };

    if candidate_name.is_empty() {
        anyhow::bail!(
            "Invalid argument declaration '{}'. Use <name> for required or <name>? for optional.",
            raw
        );
    }

    let valid_name = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$|^\d+$").unwrap();
    if !valid_name.is_match(candidate_name) {
        anyhow::bail!(
            "Invalid argument declaration '{}'. Names must match [a-zA-Z_][a-zA-Z0-9_]* or numeric index.",
            raw
        );
    }

    Ok(ArgSpec {
        name: candidate_name.to_string(),
        required,
    })
}

pub fn collect_all_arg_specs(
    global_args: &[String],
    section_args: &[String],
) -> Result<Vec<ArgSpec>> {
    let mut merged: Vec<ArgSpec> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();

    for raw in global_args.iter().chain(section_args.iter()) {
        let parsed = parse_arg_spec(raw)?;
        if let Some(existing_index) = positions.get(&parsed.name).copied() {
            if parsed.required {
                merged[existing_index].required = true;
            }
        } else {
            positions.insert(parsed.name.clone(), merged.len());
            merged.push(parsed);
        }
    }

    Ok(merged)
}

pub fn arg_names(arg_specs: &[ArgSpec]) -> Vec<String> {
    arg_specs.iter().map(|spec| spec.name.clone()).collect()
}

pub fn required_arg_names(arg_specs: &[ArgSpec]) -> Vec<String> {
    arg_specs
        .iter()
        .filter(|spec| spec.required)
        .map(|spec| spec.name.clone())
        .collect()
}

pub fn map_args_to_variables(
    declared_args: &[String],
    positional_args: &[String],
) -> Result<HashMap<String, String>> {
    let mut variables = HashMap::new();

    for (i, value) in positional_args.iter().enumerate() {
        // Map by position index (1-based) for {{ 1 }}, {{ 2 }} style
        variables.insert((i + 1).to_string(), value.clone());

        // Map by declared name if available
        if i < declared_args.len() {
            variables.insert(declared_args[i].clone(), value.clone());
        }
    }

    Ok(variables)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_variables() {
        let template = "Review {{ filename }} with {{ language }} focus on {{ filename }}";
        let vars = find_variables(template);
        assert_eq!(vars, vec!["filename", "language"]);
    }

    #[test]
    fn test_find_numeric_variables() {
        let template = "Process {{ 1 }} and {{ 2 }}";
        let vars = find_variables(template);
        assert_eq!(vars, vec!["1", "2"]);
    }

    #[test]
    fn test_render_success() {
        let template = "Hello {{ name }}, welcome to {{ place }}!";
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        vars.insert("place".to_string(), "Wonderland".to_string());
        let result = render(template, &vars).unwrap();
        assert_eq!(result, "Hello Alice, welcome to Wonderland!");
    }

    #[test]
    fn test_render_missing_variable() {
        let template = "Hello {{ name }}!";
        let vars = HashMap::new();
        let result = render(template, &vars);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("name"));
    }

    #[test]
    fn test_map_args_with_names() {
        let declared = vec!["filename".to_string(), "language".to_string()];
        let positional = vec!["main.rs".to_string(), "rust".to_string()];
        let mapped = map_args_to_variables(&declared, &positional).unwrap();
        assert_eq!(mapped.get("filename").unwrap(), "main.rs");
        assert_eq!(mapped.get("language").unwrap(), "rust");
        assert_eq!(mapped.get("1").unwrap(), "main.rs");
        assert_eq!(mapped.get("2").unwrap(), "rust");
    }

    #[test]
    fn test_map_args_without_names() {
        let declared: Vec<String> = vec![];
        let positional = vec!["file.txt".to_string()];
        let mapped = map_args_to_variables(&declared, &positional).unwrap();
        assert_eq!(mapped.get("1").unwrap(), "file.txt");
        assert!(!mapped.contains_key("filename"));
    }

    #[test]
    fn test_parse_arg_spec_required() {
        let spec = parse_arg_spec("input_file").expect("required arg spec");
        assert_eq!(
            spec,
            ArgSpec {
                name: "input_file".to_string(),
                required: true
            }
        );
    }

    #[test]
    fn test_parse_arg_spec_optional() {
        let spec = parse_arg_spec("output_format?").expect("optional arg spec");
        assert_eq!(
            spec,
            ArgSpec {
                name: "output_format".to_string(),
                required: false
            }
        );
    }

    #[test]
    fn test_parse_arg_spec_rejects_invalid_name() {
        let error = parse_arg_spec("input-format").unwrap_err();
        assert!(
            error.to_string().contains("Invalid argument declaration"),
            "error: {}",
            error
        );
    }

    #[test]
    fn test_collect_all_arg_specs_merges_optional_with_required() {
        let global = vec!["input".to_string(), "output_format?".to_string()];
        let section = vec!["output_format".to_string(), "input?".to_string()];
        let specs = collect_all_arg_specs(&global, &section).expect("merged specs");
        assert_eq!(
            specs,
            vec![
                ArgSpec {
                    name: "input".to_string(),
                    required: true
                },
                ArgSpec {
                    name: "output_format".to_string(),
                    required: true
                },
            ]
        );
    }

    #[test]
    fn test_required_arg_names_returns_only_required_args() {
        let specs = vec![
            ArgSpec {
                name: "input".to_string(),
                required: true,
            },
            ArgSpec {
                name: "output".to_string(),
                required: true,
            },
            ArgSpec {
                name: "input_format".to_string(),
                required: false,
            },
        ];
        assert_eq!(required_arg_names(&specs), vec!["input", "output"]);
        assert_eq!(
            arg_names(&specs),
            vec![
                "input".to_string(),
                "output".to_string(),
                "input_format".to_string()
            ]
        );
    }
}
