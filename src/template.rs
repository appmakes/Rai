use anyhow::Result;
use regex::Regex;
use std::collections::{HashMap, HashSet};

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
        anyhow::bail!(
            "Missing template variable(s): {}",
            missing.join(", ")
        );
    }

    Ok(result.to_string())
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

pub fn collect_all_args(
    global_args: &[String],
    section_args: &[String],
) -> Vec<String> {
    let mut all = global_args.to_vec();
    for arg in section_args {
        if !all.contains(arg) {
            all.push(arg.clone());
        }
    }
    all
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
    fn test_collect_all_args() {
        let global = vec!["filename".to_string()];
        let section = vec!["risk_level".to_string(), "filename".to_string()];
        let all = collect_all_args(&global, &section);
        assert_eq!(all, vec!["filename", "risk_level"]);
    }
}
