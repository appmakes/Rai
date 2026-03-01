use super::{Tool, ToolDefinition};
use crate::permission::Permission;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

use crate::tools::path_security::{
    ensure_not_system_critical_path, ensure_not_system_critical_path_with_base,
};
use crate::tools::utils::truncate_output;

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
            ensure_not_system_critical_path(path)?;
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
    enforce_git_path_tokens(files, cwd)?;

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
    for path in &split_paths {
        ensure_not_system_critical_path_with_base(path, cwd.map(Path::new))?;
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

fn enforce_git_path_tokens(raw_paths: &str, cwd: Option<&str>) -> Result<()> {
    let tokens = raw_paths
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        anyhow::bail!("No paths provided");
    }
    for token in tokens {
        ensure_not_system_critical_path_with_base(token, cwd.map(Path::new))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{enforce_git_path_tokens, sanitize_git_arg};

    #[test]
    fn git_arg_sanitizer_blocks_dangerous_patterns() {
        assert!(!sanitize_git_arg("$(evil)"));
        assert!(!sanitize_git_arg("--exec=rm -rf /"));
        assert!(!sanitize_git_arg("arg; rm -rf /"));
        assert!(sanitize_git_arg("--cached"));
        assert!(sanitize_git_arg("feature/test"));
    }

    #[test]
    fn git_path_tokens_block_system_critical_prefixes() {
        if !cfg!(unix) {
            return;
        }
        assert!(enforce_git_path_tokens("/etc/passwd", None).is_err());
    }

    #[test]
    fn git_path_tokens_allow_non_system_relative_paths() {
        assert!(enforce_git_path_tokens("src", Some("/workspace")).is_ok());
    }
}
