use std::process::{Command, Stdio};

fn rai_bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rai"));
    cmd.env("CI", "1");
    cmd
}

#[test]
fn test_help_output() {
    let output = rai_bin().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("A CLI tool to run AI tasks in terminal or CI/CD"));
    assert!(stdout.contains("config"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("create"));
    assert!(stdout.contains("plan"));
}

#[test]
fn test_version_output() {
    let output = rai_bin().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("rai"));
}

#[test]
fn test_run_help() {
    let output = rai_bin().args(["run", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TASK"));
    assert!(stdout.contains("--subtask"));
}

#[test]
fn test_no_subcommand_shows_help() {
    let output = rai_bin().output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
}

#[test]
fn test_run_adhoc_no_api_key() {
    let output = rai_bin()
        .args(["run", "Hello world"])
        .env_remove("RAI_API_KEY")
        .env_remove("POE_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No API key found"),
        "Expected 'No API key found' in stderr: {}",
        stderr
    );
}

#[test]
fn test_shorthand_adhoc_no_api_key() {
    let output = rai_bin()
        .args(["Hello world"])
        .env_remove("RAI_API_KEY")
        .env_remove("POE_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No API key found"),
        "Expected 'No API key found' in stderr: {}",
        stderr
    );
}

#[test]
fn test_shorthand_adhoc_empty_piped_stdin_shows_suggestions() {
    let output = rai_bin()
        .args(["summarize this"])
        .env("RAI_API_KEY", "test")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Piped content is empty"));
    assert!(stderr.contains("Suggestions"));
    assert!(stderr.contains("curl -L"));
}

#[test]
fn test_config_rejects_ci() {
    let output = rai_bin().arg("config").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("non-interactive"));
}

#[test]
fn test_create_rejects_ci() {
    let output = rai_bin()
        .args(["create", "test_output.md"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("non-interactive"));
}

#[test]
fn test_plan_nonexistent_file() {
    let output = rai_bin().args(["plan", "nonexistent.md"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn test_plan_demo_task() {
    let output = rai_bin().args(["plan", "demo/task.md"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Task Plan"));
    assert!(stdout.contains("Code Review"));
    assert!(stdout.contains("security"));
    assert!(stdout.contains("refactor"));
    assert!(stdout.contains("docs"));
    assert!(stdout.contains("gpt-4o"));
}

#[test]
fn test_plan_template_task() {
    let output = rai_bin()
        .args(["plan", "doc/template_task.md"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Code Generation"));
    assert!(stdout.contains("filename"));
    assert!(stdout.contains("language"));
    assert!(stdout.contains("[test]"));
}

#[test]
fn test_run_task_file_missing_args_ci() {
    let output = rai_bin()
        .args(["run", "demo/task.md"])
        .env("RAI_API_KEY", "test")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Missing arguments"));
}

#[test]
fn test_run_task_file_missing_subtask() {
    let output = rai_bin()
        .args(["run", "demo/task.md", "--subtask", "nonexistent", "arg1"])
        .env("RAI_API_KEY", "test")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn test_shorthand_task_file_missing_subtask() {
    let output = rai_bin()
        .args(["demo/task.md", "#nonexistent", "arg1"])
        .env("RAI_API_KEY", "test")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn test_run_adhoc_reaches_provider() {
    let output = rai_bin()
        .args(["run", "Hello"])
        .env("RAI_API_KEY", "test-dummy")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !output.status.success(),
        "Expected failure with dummy key, stdout: {}, stderr: {}",
        stdout,
        stderr
    );
    assert!(
        stderr.contains("API") || stderr.contains("error") || stderr.contains("not yet supported"),
        "Expected API or provider error, stderr: {}",
        stderr
    );
}
