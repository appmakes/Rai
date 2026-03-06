use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn integration_test_home() -> &'static PathBuf {
    static TEST_HOME: OnceLock<PathBuf> = OnceLock::new();
    TEST_HOME.get_or_init(|| {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let home = std::env::temp_dir().join(format!(
            "rai-integration-home-{}-{}",
            std::process::id(),
            nonce
        ));
        let config_dir = home.join(".config").join("rai");

        fs::create_dir_all(&config_dir)
            .expect("failed to create integration-test config directory");
        fs::write(
            config_dir.join("config.toml"),
            "default_profile = \"default\"\nactive_profile = \"default\"\nproviders = [\"poe\"]\ndefault_provider = \"poe\"\ndefault_model = \"gpt-4o\"\ntool_mode = \"ask\"\nno_tools = false\nauto_approve = false\n",
        )
        .expect("failed to write integration-test global config file");

        home
    })
}

fn rai_bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rai"));
    cmd.env("CI", "1");
    cmd.env("HOME", integration_test_home());
    cmd.env("XDG_CONFIG_HOME", integration_test_home().join(".config"));
    cmd
}

fn fresh_home() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let home = std::env::temp_dir().join(format!(
        "rai-integration-home-fresh-{}-{}",
        std::process::id(),
        nonce
    ));
    fs::create_dir_all(home.join(".config").join("rai"))
        .expect("failed to create fresh integration-test config directory");
    home
}

fn rai_bin_with_home(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rai"));
    cmd.env("CI", "1");
    cmd.env("HOME", home);
    cmd.env("XDG_CONFIG_HOME", home.join(".config"));
    cmd
}

// --- Basic CLI tests ---

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
    assert!(stdout.contains("[TASK]"), "Should show implicit TASK arg");
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
fn test_global_bill_before_run_executes_run_semantics() {
    let output = rai_bin()
        .args(["--bill", "run", "demo/task.md"])
        .env("POE_API_KEY", "test-dummy")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Missing arguments"),
        "Expected run semantics with missing template args, stderr: {}",
        stderr
    );
}

#[test]
fn test_no_subcommand_shows_help() {
    let output = rai_bin().output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
}

// --- Config, Create guards ---

#[test]
fn test_config_rejects_ci() {
    let output = rai_bin().arg("config").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("non-interactive"));
}

#[test]
fn test_start_rejects_ci() {
    let output = rai_bin().arg("start").output().unwrap();
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

// --- Run command tests ---

#[test]
fn test_run_adhoc_no_api_key() {
    let output = rai_bin()
        .args(["run", "Hello world"])
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
fn test_run_weather_prompt_no_shortcut_without_api_key() {
    let output = rai_bin()
        .args(["run", "weather in Shanghai"])
        .env_remove("POE_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No API key found"),
        "Expected no direct weather shortcut. stderr: {}",
        stderr
    );
}

#[test]
fn test_run_whois_prompt_no_shortcut_without_api_key() {
    let output = rai_bin()
        .args(["run", "whois google.com"])
        .env_remove("POE_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No API key found"),
        "Expected no direct whois shortcut. stderr: {}",
        stderr
    );
}

#[test]
fn test_bill_flag_reports_zero_usage_when_no_api_call_is_made() {
    let output = rai_bin()
        .args(["run", "Hello world", "--bill"])
        .env_remove("POE_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== Billing Summary ==="));
    assert!(stdout.contains("API calls: 0"));
    assert!(stdout.contains("Input tokens: 0"));
    assert!(stdout.contains("Output tokens: 0"));
}

#[test]
fn test_run_adhoc_reaches_provider() {
    let output = rai_bin()
        .args(["run", "Hello"])
        .env("POE_API_KEY", "test-dummy")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "Expected failure with dummy key");
    assert!(
        stderr.contains("API") || stderr.contains("error") || stderr.contains("not yet supported"),
        "Expected API or provider error, stderr: {}",
        stderr
    );
}

#[test]
fn test_run_task_file_missing_args_ci() {
    let output = rai_bin()
        .args(["run", "demo/task.md"])
        .env("POE_API_KEY", "test")
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
        .env("POE_API_KEY", "test")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

// --- #subtask shorthand tests ---

#[test]
fn test_run_hash_subtask_shorthand() {
    let output = rai_bin()
        .args(["run", "demo/task.md", "#security", "src/main.rs"])
        .env("POE_API_KEY", "test-dummy")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("API") || stderr.contains("error"),
        "Should reach API with #subtask shorthand. stderr: {}",
        stderr
    );
}

#[test]
fn test_run_hash_subtask_nonexistent() {
    let output = rai_bin()
        .args(["run", "demo/task.md", "#nonexistent", "arg1"])
        .env("POE_API_KEY", "test")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "Expected 'not found' error for bad #subtask: {}",
        stderr
    );
}

// --- Implicit run/plan (no subcommand) tests ---

#[test]
fn test_implicit_run_adhoc() {
    let output = rai_bin()
        .args(["Hello from implicit"])
        .env("POE_API_KEY", "test-dummy")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("API") || stderr.contains("error"),
        "Implicit run should reach API. stderr: {}",
        stderr
    );
}

#[test]
fn test_implicit_run_with_task_file_and_args() {
    let output = rai_bin()
        .args(["demo/task.md", "src/main.rs"])
        .env("POE_API_KEY", "test-dummy")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("API") || stderr.contains("error"),
        "Implicit run with file+args should reach API. stderr: {}",
        stderr
    );
}

#[test]
fn test_implicit_run_with_hash_subtask() {
    let output = rai_bin()
        .args(["demo/task.md", "#security", "src/main.rs"])
        .env("POE_API_KEY", "test-dummy")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("API") || stderr.contains("error"),
        "Implicit run with #subtask should reach API. stderr: {}",
        stderr
    );
}

#[test]
fn test_implicit_task_file_missing_args_ci() {
    let output = rai_bin()
        .args(["demo/task.md"])
        .env("POE_API_KEY", "test")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Missing arguments"),
        "Implicit mode with missing args in CI should fail: {}",
        stderr
    );
}

#[test]
fn test_implicit_task_file_named_flags_reach_provider() {
    let output = rai_bin()
        .args([
            "demo/convert-format.md",
            "--input",
            "demo/source.md",
            "--output",
            "target/xxx.rtf",
        ])
        .env("POE_API_KEY", "test-dummy")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("API") || stderr.contains("error"),
        "Named flags should pass argument parsing and reach provider. stderr: {}",
        stderr
    );
    assert!(
        !stderr.contains("Missing arguments"),
        "Should not fail argument validation when required named args are provided. stderr: {}",
        stderr
    );
}

#[test]
fn test_implicit_task_file_named_flags_missing_required_arg() {
    let output = rai_bin()
        .args(["demo/convert-format.md", "--input", "demo/source.md"])
        .env("POE_API_KEY", "test")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Missing arguments"),
        "Expected missing argument error for required output flag. stderr: {}",
        stderr
    );
    assert!(stderr.contains("output"), "stderr: {}", stderr);
}

#[test]
fn test_implicit_task_file_named_flags_unknown_argument() {
    let output = rai_bin()
        .args([
            "demo/convert-format.md",
            "--input",
            "demo/source.md",
            "--output",
            "target/xxx.rtf",
            "--destination",
            "target/other.rtf",
        ])
        .env("POE_API_KEY", "test")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Unknown argument"),
        "Expected unknown argument validation error. stderr: {}",
        stderr
    );
}

// --- Plan command tests ---

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
fn test_plan_translate_demo_task() {
    let output = rai_bin()
        .args(["plan", "demo/translate.md"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Task Plan"));
    assert!(stdout.contains("Localize Xcode strings into 10 languages"));
    assert!(stdout.contains("zh-Hans"));
    assert!(stdout.contains("ja-JP"));
}

#[test]
fn test_plan_template_task() {
    let output = rai_bin()
        .args(["plan", "doc/development/template_task.md"])
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
fn test_plan_with_hash_subtask() {
    let output = rai_bin()
        .args(["plan", "demo/task.md", "#security"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Task Plan"));
    assert!(stdout.contains("Non-interactive mode"));
}

#[test]
fn test_plan_hint_uses_shorthand() {
    let output = rai_bin().args(["plan", "demo/task.md"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("rai demo/task.md"),
        "Plan hint should use shorthand syntax: {}",
        stdout
    );
}

#[test]
fn test_profile_list() {
    let output = rai_bin().args(["profile", "list"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("default"));
}

#[test]
fn test_profile_show_bootstraps_missing_default_profile() {
    let home = fresh_home();
    let config_dir = home.join(".config").join("rai");
    fs::write(
        config_dir.join("config.toml"),
        "default_profile = \"default\"\nactive_profile = \"default\"\n",
    )
    .expect("failed to write global config");

    let output = rai_bin_with_home(&home)
        .args(["profile", "show"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Profile: default"), "stdout: {}", stdout);
    let config_content = fs::read_to_string(config_dir.join("config.toml"))
        .expect("config.toml should exist after profile bootstrap");
    assert!(
        config_content.contains("default_provider"),
        "default profile should be stored in config.toml: {}",
        config_content
    );
}

#[test]
fn test_bill_flag_on_plan_reports_zero_usage() {
    let output = rai_bin()
        .args(["plan", "demo/task.md", "--bill"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== Billing Summary ==="));
    assert!(stdout.contains("API calls: 0"), "stdout: {}", stdout);
    assert!(stdout.contains("Input tokens: 0"), "stdout: {}", stdout);
    assert!(stdout.contains("Output tokens: 0"), "stdout: {}", stdout);
}
