use anyhow::Context;
use clap::{CommandFactory, Parser, Subcommand};
use config::Config;
use dialoguer::{Confirm, Input, Select};
use std::io::{Read, Write};
use std::path::Path;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

mod agent;
mod config;
mod key_store;
mod permission;
mod providers;
mod task_parser;
mod template;
mod tools;

use providers::{get_billing_stats, reset_billing_stats, BillingStats, Provider};
use tools::Tool;

fn set_api_key_helper(provider: &str, api_key: &str) -> anyhow::Result<()> {
    key_store::set_api_key(provider, api_key)
}

#[cfg(not(test))]
fn get_api_key_helper(provider: &str) -> anyhow::Result<String> {
    key_store::get_api_key(provider)
}

fn is_interactive() -> bool {
    atty::is(atty::Stream::Stdin) && std::env::var("CI").is_err()
}

fn extract_subtask_from_args(
    explicit_subtask: Option<&str>,
    args: &[String],
) -> (Option<String>, Vec<String>) {
    if explicit_subtask.is_some() {
        return (explicit_subtask.map(|s| s.to_string()), args.to_vec());
    }
    let mut subtask = None;
    let mut clean_args = Vec::new();
    for arg in args {
        if subtask.is_none() && arg.starts_with('#') && arg.len() > 1 {
            subtask = Some(arg[1..].to_string());
        } else {
            clean_args.push(arg.clone());
        }
    }
    (subtask, clean_args)
}

#[derive(Parser)]
#[command(name = "rai")]
#[command(version)]
#[command(about = "A CLI tool to run AI tasks in terminal or CI/CD", long_about = None)]
#[command(args_conflicts_with_subcommands = true)]
#[command(subcommand_precedence_over_arg = true)]
struct Cli {
    /// Turn debugging information on
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Override the AI model to use (e.g., gpt-4o, kimi-k2)
    #[arg(short, long, global = true)]
    model: Option<String>,

    /// Auto-approve all tool calls (global blocklist still enforced)
    #[arg(short, long, global = true)]
    yes: bool,

    /// Disable tool calling (single-turn mode only)
    #[arg(long, global = true)]
    no_tools: bool,

    /// Print API-call and token usage summary for this command
    #[arg(long, global = true)]
    bill: bool,

    /// Show detailed runtime logs (tool calls, provider notices)
    #[arg(long, global = true)]
    log: bool,

    /// Select configuration profile
    #[arg(long, global = true)]
    profile: Option<String>,

    /// Task description or file path (shorthand for `rai run`)
    task: Option<String>,

    /// Arguments for the task (including #subtask selector)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// First-time setup wizard
    Start,
    /// Configure AI model provider and other settings
    Config,
    /// Manage profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },
    /// Run a task directly
    Run {
        /// The task description or file path
        task: String,

        /// Optional sub-task name (e.g., #summary)
        #[arg(short, long)]
        subtask: Option<String>,

        /// Arguments for the task (including #subtask selector)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Create a new task file
    Create {
        /// The filename to create
        filename: String,
    },
    /// Plan and preview a task execution
    Plan {
        /// The task file to plan
        task_file: String,

        /// Optional sub-task name (e.g., #summary)
        #[arg(short, long)]
        subtask: Option<String>,

        /// Arguments pre-filled for the task
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ProfileCommands {
    /// List all profiles
    List,
    /// Show profile details
    Show {
        /// Profile name
        name: Option<String>,
    },
    /// Create a new profile
    Create {
        /// New profile name
        name: String,
        /// Optional source profile to copy from
        #[arg(long)]
        copy_from: Option<String>,
    },
    /// Delete a profile
    Delete {
        /// Profile name
        name: String,
    },
    /// Rename a profile
    Rename {
        /// Existing profile name
        old: String,
        /// New profile name
        new: String,
    },
    /// Set active profile
    Switch {
        /// Profile name
        name: String,
    },
    /// Set default profile
    Default {
        /// Profile name
        name: String,
    },
}

fn resolve_provider(config: &Config) -> anyhow::Result<Box<dyn Provider>> {
    let provider = config.provider.trim().to_lowercase();
    if provider.is_empty() {
        anyhow::bail!(
            "No provider configured for profile '{}'. Run `rai start` or `rai config`.",
            config.profile
        );
    }

    match provider.as_str() {
        "poe" => Ok(Box::new(providers::poe::PoeProvider::new(&config.api_key))),
        other => anyhow::bail!("Provider '{}' is not yet supported. Supported: poe", other),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PipedStdin {
    NotPiped,
    Empty,
    Content(String),
}

fn read_piped_stdin() -> anyhow::Result<PipedStdin> {
    if atty::is(atty::Stream::Stdin) {
        return Ok(PipedStdin::NotPiped);
    }

    let mut stdin_content = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin_content)
        .context("Failed to read piped stdin")?;

    if stdin_content.trim().is_empty() {
        Ok(PipedStdin::Empty)
    } else {
        Ok(PipedStdin::Content(stdin_content))
    }
}

fn ensure_non_empty_piped_stdin(piped_stdin: &PipedStdin) -> anyhow::Result<()> {
    if matches!(piped_stdin, PipedStdin::Empty) {
        anyhow::bail!(
            "Piped content is empty.\n\
No stdin text was received from the previous command.\n\
Suggestions:\n\
  1. Quote or escape special URL characters such as '&'.\n\
  2. Follow redirects when fetching web pages (`curl -L` or `curl -Ls`).\n\
  3. Verify stdin size before piping (`... | wc -c`)."
        );
    }

    Ok(())
}

fn compose_adhoc_prompt(task: &str, piped_stdin: Option<&str>) -> String {
    match piped_stdin {
        Some(stdin) if !stdin.trim().is_empty() => {
            format!("{}\n\n{}", task, stdin.trim_end())
        }
        _ => task.to_string(),
    }
}

fn append_direct_tool_failure_note(base_prompt: String, direct_error: Option<&str>) -> String {
    let Some(error) = direct_error else {
        return base_prompt;
    };
    format!(
        "{}\n\n[Execution note]\nA direct built-in attempt failed with: {}\nPlease decide what to do next and still fulfill the original user request. Prefer alternative tools/sources when needed.",
        base_prompt, error
    )
}

#[cfg(test)]
fn parse_shorthand_args(raw_args: &[String]) -> (Option<String>, Vec<String>) {
    let mut subtask: Option<String> = None;
    let mut args: Vec<String> = Vec::new();

    if let Some(first) = raw_args.first() {
        if let Some(stripped) = first.strip_prefix('#') {
            if !stripped.is_empty() {
                subtask = Some(stripped.to_string());
            } else {
                args.push(first.clone());
            }
        } else {
            args.push(first.clone());
        }
    }

    args.extend(raw_args.iter().skip(1).cloned());
    (subtask, args)
}

fn print_billing_summary(stats: BillingStats) {
    println!();
    println!(
        "{}=== Billing Summary ==={}",
        style_code(Style::Billing),
        style_code(Style::Reset)
    );
    println!(
        "{}API calls:{} {}",
        style_code(Style::Info),
        style_code(Style::Reset),
        stats.api_calls
    );
    println!(
        "{}Input tokens:{} {}",
        style_code(Style::Info),
        style_code(Style::Reset),
        stats.input_tokens
    );
    println!(
        "{}Output tokens:{} {}",
        style_code(Style::Info),
        style_code(Style::Reset),
        stats.output_tokens
    );
}

#[derive(Clone, Copy)]
struct ExecutionOptions<'a> {
    model_override: Option<&'a str>,
    profile_override: Option<&'a str>,
    cli_no_tools: bool,
    cli_auto_approve: bool,
    log_enabled: bool,
}

fn execution_options_from_cli(cli: &Cli) -> ExecutionOptions<'_> {
    ExecutionOptions {
        model_override: cli.model.as_deref(),
        profile_override: cli.profile.as_deref(),
        cli_no_tools: cli.no_tools,
        cli_auto_approve: cli.yes,
        log_enabled: cli.log,
    }
}

async fn handle_run(
    task: &str,
    subtask: Option<&str>,
    args: &[String],
    opts: ExecutionOptions<'_>,
) -> anyhow::Result<()> {
    let task_path = Path::new(task);
    let is_file = task_path.exists() && task_path.is_file();
    let mut direct_tool_failure: Option<String> = None;
    if !is_file {
        match try_handle_direct_prompt(task) {
            Ok(Some(direct_output)) => {
                print_result(&direct_output);
                return Ok(());
            }
            Ok(None) => {}
            Err(err) => {
                direct_tool_failure = Some(err.to_string());
                if opts.log_enabled {
                    print_info("Direct tool attempt failed; asking AI for fallback.");
                }
            }
        }
    }
    let piped_stdin = if is_file {
        PipedStdin::NotPiped
    } else {
        read_piped_stdin()?
    };

    if !is_file && std::env::var("CI").is_err() {
        ensure_non_empty_piped_stdin(&piped_stdin)?;
    }

    let mut config = Config::load(opts.profile_override)?;
    config.resolve_api_key()?;

    if config.api_key.is_empty() {
        anyhow::bail!(
            "No API key found. Please run `rai config` or set RAI_API_KEY environment variable."
        );
    }

    let (prompt, model) = if is_file {
        let parsed = task_parser::parse_task_file(task_path)?;
        let section = parsed.get_section(subtask)?;

        let declared_args =
            template::collect_all_args(&parsed.global_frontmatter.args, &section.frontmatter.args);

        let vars_in_template = template::find_variables(&section.content);

        let effective_args = if declared_args.is_empty() && !vars_in_template.is_empty() {
            vars_in_template.clone()
        } else {
            declared_args
        };

        let variables = {
            if args.len() < vars_in_template.len() && !is_interactive() {
                anyhow::bail!(
                    "Missing arguments. Expected {} ({}) but got {}. \
                     Provide all arguments in non-interactive mode.",
                    vars_in_template.len(),
                    vars_in_template.join(", "),
                    args.len()
                );
            }

            let mut mapped = template::map_args_to_variables(&effective_args, args)?;

            if is_interactive() {
                for var in &vars_in_template {
                    if !mapped.contains_key(var) {
                        let value: String = Input::new()
                            .with_prompt(format!("Enter value for '{}'", var))
                            .interact_text()?;
                        mapped.insert(var.clone(), value);
                    }
                }
            }

            mapped
        };

        let rendered = template::render(&section.content, &variables)?;

        let effective_model = opts
            .model_override
            .map(|s| s.to_string())
            .or_else(|| parsed.effective_model(subtask))
            .unwrap_or(config.default_model.clone());

        info!("Task: {} (section: {})", task, section.name);
        (rendered, effective_model)
    } else {
        let model = opts
            .model_override
            .map(|s| s.to_string())
            .unwrap_or(config.default_model.clone());
        let piped_content = match &piped_stdin {
            PipedStdin::Content(content) => Some(content.as_str()),
            PipedStdin::NotPiped | PipedStdin::Empty => None,
        };
        let base_prompt = compose_adhoc_prompt(task, piped_content);
        let prompt = append_direct_tool_failure_note(base_prompt, direct_tool_failure.as_deref());
        (prompt, model)
    };

    let provider_impl = resolve_provider(&config)?;
    info!("Using provider: {}, model: {}", config.provider, model);

    let mut use_agent = if opts.cli_no_tools {
        false
    } else {
        !config.no_tools
    };
    let mut auto_approve = opts.cli_auto_approve || config.auto_approve;
    match config.tool_mode.as_str() {
        "allow" => auto_approve = true,
        "deny" => use_agent = false,
        _ => {}
    }

    if use_agent {
        let builtin = tools::builtin_tools();
        let agent_config = agent::AgentConfig {
            auto_approve,
            log_enabled: opts.log_enabled,
            ..Default::default()
        };
        let mut agent_loop = agent::Agent::new(provider_impl, model, builtin, agent_config);

        let response = agent_loop.run(&prompt).await?;
        print_result(&response);
    } else {
        if opts.log_enabled {
            print_info(&format!("Sending request to {}...", config.provider));
        }
        let response = provider_impl.chat(&model, &prompt).await?;
        print_result(&response);
    }

    Ok(())
}

fn handle_create(filename: &str) -> anyhow::Result<()> {
    let path = Path::new(filename);
    if path.exists() {
        anyhow::bail!(
            "File '{}' already exists. Choose a different name.",
            filename
        );
    }

    if !is_interactive() {
        anyhow::bail!("Cannot run `rai create` in non-interactive mode (CI/CD). Create the task file manually.");
    }

    let task_name: String = Input::new()
        .with_prompt("Task name (H1 heading)")
        .interact_text()?;

    let task_description: String = Input::new()
        .with_prompt("Task description / prompt")
        .interact_text()?;

    let model: String = Input::new()
        .with_prompt("Model (leave empty for default)")
        .default(String::new())
        .interact_text()?;

    let args_input: String = Input::new()
        .with_prompt("Variables (comma-separated, e.g. filename,language)")
        .default(String::new())
        .interact_text()?;

    let args: Vec<String> = args_input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let wants_subtask = Confirm::new()
        .with_prompt("Add a sub-task?")
        .default(false)
        .interact()?;

    let mut subtasks: Vec<(String, String)> = Vec::new();
    if wants_subtask {
        loop {
            let sub_name: String = Input::new()
                .with_prompt("Sub-task name (H2 heading)")
                .interact_text()?;
            let sub_desc: String = Input::new()
                .with_prompt("Sub-task description / prompt")
                .interact_text()?;
            subtasks.push((sub_name, sub_desc));

            let add_more = Confirm::new()
                .with_prompt("Add another sub-task?")
                .default(false)
                .interact()?;
            if !add_more {
                break;
            }
        }
    }

    let mut content = String::new();
    content.push_str("---\n");
    if !model.is_empty() {
        content.push_str(&format!("model: {}\n", model));
    }
    if !args.is_empty() {
        content.push_str("args:\n");
        for arg in &args {
            content.push_str(&format!("  - {}\n", arg));
        }
    }
    content.push_str("---\n\n");

    content.push_str(&format!("# {}\n", task_name));
    content.push_str(&format!("{}\n", task_description));

    for (name, desc) in &subtasks {
        content.push_str(&format!("\n## {}\n", name));
        content.push_str(&format!("{}\n", desc));
    }

    std::fs::write(path, &content)
        .with_context(|| format!("Failed to write task file: {}", filename))?;

    println!("Created task file: {}", filename);
    println!("\nGenerated content:");
    println!("{}", content);

    Ok(())
}

fn print_section(title: &str) {
    println!("\n=== {} ===", title);
}

#[derive(Clone, Copy)]
enum Style {
    Reset,
    Info,
    Billing,
}

fn color_output_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && atty::is(atty::Stream::Stdout)
}

fn style_code(style: Style) -> &'static str {
    if !color_output_enabled() {
        return "";
    }
    match style {
        Style::Reset => "\x1b[0m",
        Style::Info => "\x1b[36m",
        Style::Billing => "\x1b[35m",
    }
}

fn print_info(message: &str) {
    println!(
        "{}{}{}",
        style_code(Style::Info),
        message,
        style_code(Style::Reset)
    );
}

fn print_result(message: &str) {
    println!("{}", message);
}

fn try_handle_direct_prompt(task: &str) -> anyhow::Result<Option<String>> {
    let trimmed = task.trim();
    let lowercase = trimmed.to_ascii_lowercase();

    if lowercase.starts_with("weather in ") {
        let location = trimmed["weather in ".len()..].trim();
        if location.is_empty() {
            return Ok(None);
        }
        let encoded_location = location.split_whitespace().collect::<Vec<_>>().join("%20");
        let url = format!("https://wttr.in/{}?format=3", encoded_location);
        let tool = tools::builtin::HttpGetTool;
        let output = tool.execute(&serde_json::json!({ "url": url }))?;
        return Ok(Some(output.trim().to_string()));
    }

    if lowercase.starts_with("whois ") {
        let domain = trimmed["whois ".len()..].trim();
        if domain.is_empty() {
            return Ok(None);
        }
        let tool = tools::builtin::WhoisTool;
        let output = tool.execute(&serde_json::json!({ "domain": domain }))?;
        return Ok(Some(output.trim().to_string()));
    }

    Ok(None)
}

fn set_profile_api_key(profile: &str, provider: &str, api_key: &str) -> anyhow::Result<()> {
    if api_key.trim().is_empty() {
        return Ok(());
    }
    let scoped_provider = format!("{}:{}", profile, provider);
    set_api_key_helper(&scoped_provider, api_key)
        .context("Failed to save profile-scoped API key to keyring")?;
    // Backward-compatible fallback key.
    let _ = set_api_key_helper(provider, api_key);
    Ok(())
}

fn available_providers() -> Vec<&'static str> {
    vec!["poe", "openai", "anthropic", "google", "xai"]
}

fn configure_provider_and_key(config: &mut Config) -> anyhow::Result<()> {
    print_section("Provider");
    let providers = available_providers();
    let default_idx = providers
        .iter()
        .position(|provider| *provider == config.provider)
        .unwrap_or(0);
    let selection = Select::new()
        .with_prompt("Select provider")
        .items(&providers)
        .default(default_idx)
        .interact_opt()?;
    let provider = match selection {
        Some(index) => providers[index].to_string(),
        None => {
            println!("No changes made.");
            return Ok(());
        }
    };

    config.provider = provider.clone();
    config.providers = vec![provider.clone()];
    config.default_provider = Some(provider.clone());

    let api_key: String = Input::new()
        .with_prompt("API key (leave empty to keep current)")
        .allow_empty(true)
        .interact_text()?;
    if !api_key.trim().is_empty() {
        set_profile_api_key(&config.profile, &provider, &api_key)?;
    }
    Ok(())
}

fn configure_model_defaults(config: &mut Config) -> anyhow::Result<()> {
    print_section("Model");
    let default_model: String = Input::new()
        .with_prompt("Default model")
        .default(config.default_model.clone())
        .interact_text()?;
    config.default_model = default_model;
    Ok(())
}

fn configure_tools(config: &mut Config) -> anyhow::Result<()> {
    print_section("Tools");
    let tool_modes = vec!["ask", "ask_once", "allow", "deny"];
    let mode_index = tool_modes
        .iter()
        .position(|mode| *mode == config.tool_mode)
        .unwrap_or(0);
    if let Some(selection) = Select::new()
        .with_prompt("Default tool mode")
        .items(&tool_modes)
        .default(mode_index)
        .interact_opt()?
    {
        config.tool_mode = tool_modes[selection].to_string();
    }

    config.no_tools = Confirm::new()
        .with_prompt("Disable tool calling by default?")
        .default(config.no_tools)
        .interact()?;
    config.auto_approve = Confirm::new()
        .with_prompt("Auto-approve tool calls by default?")
        .default(config.auto_approve)
        .interact()?;
    Ok(())
}

fn print_profiles_list() -> anyhow::Result<()> {
    let profiles = Config::list_profiles()?;
    let (default_profile, active_profile) = Config::read_global_profile_settings()?;
    if profiles.is_empty() {
        println!("No profiles found.");
        return Ok(());
    }
    println!("Profiles:");
    for profile in profiles {
        let mut tags = Vec::new();
        if profile == default_profile {
            tags.push("default");
        }
        if active_profile.as_deref() == Some(profile.as_str()) {
            tags.push("active");
        }
        if tags.is_empty() {
            println!("- {}", profile);
        } else {
            println!("- {} ({})", profile, tags.join(", "));
        }
    }
    Ok(())
}

fn configure_profiles_menu(config: &mut Config) -> anyhow::Result<()> {
    loop {
        print_section("Profiles");
        let options = vec![
            "Switch active profile",
            "Create profile",
            "Delete profile",
            "Rename current profile",
            "Set default profile",
            "List profiles",
            "Back",
        ];
        let selection = Select::new()
            .with_prompt(format!("Current profile: {}", config.profile))
            .items(&options)
            .default(0)
            .interact_opt()?;

        match selection {
            Some(0) => {
                let profiles = Config::list_profiles()?;
                if profiles.is_empty() {
                    println!("No profiles to switch to.");
                    continue;
                }
                let current_idx = profiles
                    .iter()
                    .position(|profile| profile == &config.profile)
                    .unwrap_or(0);
                if let Some(choice) = Select::new()
                    .with_prompt("Switch to profile")
                    .items(&profiles)
                    .default(current_idx)
                    .interact_opt()?
                {
                    let selected = &profiles[choice];
                    Config::set_active_profile(selected)?;
                    *config = Config::load(Some(selected))?;
                }
            }
            Some(1) => {
                let name: String = Input::new()
                    .with_prompt("New profile name")
                    .interact_text()?;
                let copy_current = Confirm::new()
                    .with_prompt("Copy settings from current profile?")
                    .default(true)
                    .interact()?;
                let copy_from = if copy_current {
                    Some(config.profile.as_str())
                } else {
                    None
                };
                Config::create_profile(&name, copy_from)?;
                println!("Profile '{}' created.", name);
            }
            Some(2) => {
                let profiles = Config::list_profiles()?;
                if profiles.is_empty() {
                    println!("No profiles to delete.");
                    continue;
                }
                if let Some(choice) = Select::new()
                    .with_prompt("Delete which profile?")
                    .items(&profiles)
                    .default(0)
                    .interact_opt()?
                {
                    let selected = profiles[choice].clone();
                    Config::delete_profile(&selected)?;
                    println!("Profile '{}' deleted.", selected);
                }
            }
            Some(3) => {
                let new_name: String = Input::new()
                    .with_prompt("Rename current profile to")
                    .interact_text()?;
                let old_name = config.profile.clone();
                Config::rename_profile(&old_name, &new_name)?;
                *config = Config::load(Some(&new_name))?;
                println!("Profile '{}' renamed to '{}'.", old_name, new_name);
            }
            Some(4) => {
                let profiles = Config::list_profiles()?;
                if profiles.is_empty() {
                    println!("No profiles available.");
                    continue;
                }
                if let Some(choice) = Select::new()
                    .with_prompt("Set default profile")
                    .items(&profiles)
                    .default(0)
                    .interact_opt()?
                {
                    let selected = &profiles[choice];
                    Config::set_default_profile(selected)?;
                    println!("Default profile set to '{}'.", selected);
                }
            }
            Some(5) => {
                print_profiles_list()?;
            }
            Some(6) | None => break,
            _ => {}
        }
    }
    Ok(())
}

fn handle_config(profile_override: Option<&str>) -> anyhow::Result<()> {
    if !is_interactive() {
        anyhow::bail!(
            "Cannot run `rai config` in non-interactive mode. Set configuration via files/env vars."
        );
    }

    let mut config = Config::load(profile_override)?;
    loop {
        print_section("Configuration");
        let options = vec![
            "Provider & API key",
            "Model defaults",
            "Tools",
            "Profiles",
            "Save and exit",
            "Exit without saving",
        ];
        let selection = Select::new()
            .with_prompt(format!("Editing profile: {}", config.profile))
            .items(&options)
            .default(0)
            .interact_opt()?;

        match selection {
            Some(0) => configure_provider_and_key(&mut config)?,
            Some(1) => configure_model_defaults(&mut config)?,
            Some(2) => configure_tools(&mut config)?,
            Some(3) => configure_profiles_menu(&mut config)?,
            Some(4) => {
                config.save()?;
                Config::set_active_profile(&config.profile)?;
                println!("Configuration saved.");
                return Ok(());
            }
            Some(5) | None => {
                println!("Exited without saving.");
                return Ok(());
            }
            _ => {}
        }
    }
}

fn handle_start(profile_override: Option<&str>) -> anyhow::Result<()> {
    if !is_interactive() {
        anyhow::bail!(
            "Cannot run `rai start` in non-interactive mode. Use `rai config` files/env vars instead."
        );
    }

    let target_profile = profile_override.unwrap_or("default");
    if !Config::profile_exists(target_profile)? {
        Config::create_profile(target_profile, None)?;
    }
    Config::set_active_profile(target_profile)?;
    let mut config = Config::load(Some(target_profile))?;

    print_section("Welcome to rai start");
    println!("We'll do a quick setup: provider, API key, and model.");

    configure_provider_and_key(&mut config)?;
    configure_model_defaults(&mut config)?;
    config.save()?;
    Config::set_active_profile(&config.profile)?;

    println!("\nSaved profile '{}'.", config.profile);
    println!("Try: rai run \"Hello world\"");

    let choices = vec!["Continue", "More settings"];
    let selection = Select::new()
        .with_prompt("Next step")
        .items(&choices)
        .default(0)
        .interact_opt()?;
    if matches!(selection, Some(1)) {
        handle_config(Some(config.profile.as_str()))?;
    }

    Ok(())
}

fn handle_profile_command(
    command: &ProfileCommands,
    profile_override: Option<&str>,
) -> anyhow::Result<()> {
    match command {
        ProfileCommands::List => print_profiles_list(),
        ProfileCommands::Show { name } => {
            let target = if let Some(name) = name.as_deref() {
                name.to_string()
            } else if let Some(profile) = profile_override {
                profile.to_string()
            } else {
                let (default_profile, active_profile) = Config::read_global_profile_settings()?;
                active_profile.unwrap_or(default_profile)
            };
            let config = Config::load(Some(&target))?;
            println!("Profile: {}", config.profile);
            println!("Providers: {}", config.providers.join(", "));
            println!(
                "Default provider: {}",
                config
                    .default_provider
                    .as_deref()
                    .unwrap_or("(none configured)")
            );
            println!("Default model: {}", config.default_model);
            println!("Tool mode: {}", config.tool_mode);
            println!("no_tools: {}", config.no_tools);
            println!("auto_approve: {}", config.auto_approve);
            Ok(())
        }
        ProfileCommands::Create { name, copy_from } => {
            Config::create_profile(name, copy_from.as_deref())?;
            println!("Profile '{}' created.", name);
            Ok(())
        }
        ProfileCommands::Delete { name } => {
            Config::delete_profile(name)?;
            println!("Profile '{}' deleted.", name);
            Ok(())
        }
        ProfileCommands::Rename { old, new } => {
            Config::rename_profile(old, new)?;
            println!("Profile '{}' renamed to '{}'.", old, new);
            Ok(())
        }
        ProfileCommands::Switch { name } => {
            Config::set_active_profile(name)?;
            println!("Active profile set to '{}'.", name);
            Ok(())
        }
        ProfileCommands::Default { name } => {
            Config::set_default_profile(name)?;
            println!("Default profile set to '{}'.", name);
            Ok(())
        }
    }
}

async fn handle_plan(
    task_file: &str,
    subtask: Option<&str>,
    prefilled_args: &[String],
    opts: ExecutionOptions<'_>,
) -> anyhow::Result<()> {
    let path = Path::new(task_file);
    if !path.exists() {
        anyhow::bail!("Task file '{}' not found.", task_file);
    }

    let parsed = task_parser::parse_task_file(path)?;

    println!("=== Task Plan: {} ===\n", task_file);

    if let Some(model) = &parsed.global_frontmatter.model {
        println!("Model: {}", model);
    }
    if let Some(temp) = parsed.global_frontmatter.temperature {
        println!("Temperature: {}", temp);
    }
    if !parsed.global_frontmatter.args.is_empty() {
        println!("Global args: {}", parsed.global_frontmatter.args.join(", "));
    }
    println!();

    if let Some(main) = &parsed.main_task {
        println!("--- Main Task: {} ---", main.name);
        let vars = template::find_variables(&main.content);
        if !vars.is_empty() {
            println!("  Variables: {}", vars.join(", "));
        }
        println!("  Preview:");
        for line in main.content.lines().take(5) {
            println!("    {}", line);
        }
        if main.content.lines().count() > 5 {
            println!("    ...");
        }
        println!();
    }

    let subtask_names = parsed.list_subtasks();
    if !subtask_names.is_empty() {
        println!("--- Sub-tasks ---");
        for name in &subtask_names {
            let section = parsed.subtasks.get(*name).unwrap();
            let vars = template::find_variables(&section.content);
            print!("  [{}]", name);
            if !vars.is_empty() {
                print!(" (vars: {})", vars.join(", "));
            }
            if !section.frontmatter.args.is_empty() {
                print!(" (args: {})", section.frontmatter.args.join(", "));
            }
            println!();
        }
        println!();
    }

    if !is_interactive() {
        println!(
            "Non-interactive mode: use `rai {} [#subtask] [args...]` to execute.",
            task_file
        );
        return Ok(());
    }

    let subtask_opt: Option<&str> = if subtask.is_some() {
        subtask
    } else {
        let mut options: Vec<String> = Vec::new();
        if parsed.main_task.is_some() {
            options.push("(main task)".to_string());
        }
        for name in &subtask_names {
            options.push(format!("#{}", name));
        }
        options.push("(cancel)".to_string());

        let selection = Select::new()
            .with_prompt("Select a task to execute")
            .items(&options)
            .default(0)
            .interact_opt()?;

        match selection {
            Some(idx) => {
                let cancel_idx = options.len() - 1;
                if idx == cancel_idx {
                    println!("Cancelled.");
                    return Ok(());
                }

                let has_main = parsed.main_task.is_some();
                if has_main && idx == 0 {
                    None
                } else {
                    let sub_idx = if has_main { idx - 1 } else { idx };
                    Some(subtask_names[sub_idx])
                }
            }
            None => {
                println!("Cancelled.");
                return Ok(());
            }
        }
    };

    let section = parsed.get_section(subtask_opt)?;
    let all_args =
        template::collect_all_args(&parsed.global_frontmatter.args, &section.frontmatter.args);
    let vars = template::find_variables(&section.content);

    let effective_args = if all_args.is_empty() && !vars.is_empty() {
        vars.clone()
    } else {
        all_args
    };

    let mut mapped = template::map_args_to_variables(&effective_args, prefilled_args)?;

    for var in &vars {
        if !mapped.contains_key(var) {
            let value: String = Input::new().with_prompt(var).interact_text()?;
            mapped.insert(var.clone(), value);
        }
    }

    let rendered = template::render(&section.content, &mapped)?;

    println!("\n=== Final Prompt ===");
    println!("{}", rendered);

    let approx_tokens = rendered.split_whitespace().count() * 4 / 3;
    println!("\nEstimated tokens: ~{}", approx_tokens);

    let confirm = Confirm::new()
        .with_prompt("Execute this task?")
        .default(true)
        .interact()?;

    if !confirm {
        println!("Cancelled.");
        return Ok(());
    }

    let final_args: Vec<String> = effective_args
        .iter()
        .filter_map(|name| mapped.get(name).cloned())
        .collect();

    handle_run(task_file, subtask_opt, &final_args, opts).await
}

async fn smart_execute(
    task: &str,
    subtask: Option<&str>,
    args: &[String],
    opts: ExecutionOptions<'_>,
) -> anyhow::Result<()> {
    let task_path = Path::new(task);
    let is_file = task_path.exists() && task_path.is_file();

    if !is_file {
        return handle_run(task, subtask, args, opts).await;
    }

    let parsed = task_parser::parse_task_file(task_path)?;

    let section = match parsed.get_section(subtask) {
        Ok(s) => s,
        Err(_) if subtask.is_none() && !parsed.subtasks.is_empty() => {
            return handle_plan(task, subtask, args, opts).await;
        }
        Err(e) => return Err(e),
    };

    let vars = template::find_variables(&section.content);

    if vars.is_empty() || args.len() >= vars.len() {
        handle_run(task, subtask, args, opts).await
    } else if is_interactive() {
        handle_plan(task, subtask, args, opts).await
    } else {
        handle_run(task, subtask, args, opts).await
    }
}

fn maybe_print_subcommand_help(name: &str) -> anyhow::Result<bool> {
    let mut cmd = Cli::command();
    let Some(sub) = cmd.find_subcommand_mut(name) else {
        return Ok(false);
    };
    sub.write_long_help(&mut std::io::stdout())?;
    std::io::stdout().write_all(b"\n")?;
    Ok(true)
}

async fn dispatch_keyword_task_as_command(cli: &Cli, task_keyword: &str) -> anyhow::Result<bool> {
    let opts = execution_options_from_cli(cli);
    match task_keyword {
        "run" => {
            if cli.args.is_empty() {
                anyhow::bail!("Missing task. Usage: rai run <task>");
            }
            if cli.args.iter().any(|arg| arg == "-h" || arg == "--help") {
                maybe_print_subcommand_help("run")?;
                return Ok(true);
            }
            let task = cli.args[0].clone();
            let trailing_args = cli.args[1..].to_vec();
            let (resolved_subtask, clean_args) = extract_subtask_from_args(None, &trailing_args);
            handle_run(&task, resolved_subtask.as_deref(), &clean_args, opts).await?;
            Ok(true)
        }
        "plan" => {
            if cli.args.is_empty() {
                anyhow::bail!("Missing task file. Usage: rai plan <task_file>");
            }
            if cli.args.iter().any(|arg| arg == "-h" || arg == "--help") {
                maybe_print_subcommand_help("plan")?;
                return Ok(true);
            }
            let task_file = cli.args[0].clone();
            let trailing_args = cli.args[1..].to_vec();
            let (resolved_subtask, clean_args) = extract_subtask_from_args(None, &trailing_args);
            handle_plan(&task_file, resolved_subtask.as_deref(), &clean_args, opts).await?;
            Ok(true)
        }
        "start" if cli.args.is_empty() => {
            handle_start(cli.profile.as_deref())?;
            Ok(true)
        }
        "config" if cli.args.is_empty() => {
            handle_config(cli.profile.as_deref())?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let log_level = match cli.verbose {
        0 => Level::WARN,
        1 => Level::INFO,
        2 => Level::DEBUG,
        _ => Level::TRACE,
    };

    let subscriber = FmtSubscriber::builder().with_max_level(log_level).finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    if let Some(model) = cli.model.as_ref() {
        info!("Overriding model to: {}", model);
    }

    if cli.bill {
        reset_billing_stats();
    }

    let execution_result: anyhow::Result<()> = async {
        match &cli.command {
            Some(Commands::Start) => handle_start(cli.profile.as_deref())?,
            Some(Commands::Config) => handle_config(cli.profile.as_deref())?,
            Some(Commands::Profile { command }) => {
                handle_profile_command(command, cli.profile.as_deref())?
            }
            Some(Commands::Run {
                task,
                subtask,
                args,
            }) => {
                let (resolved_subtask, clean_args) =
                    extract_subtask_from_args(subtask.as_deref(), args);
                handle_run(
                    task,
                    resolved_subtask.as_deref(),
                    &clean_args,
                    execution_options_from_cli(&cli),
                )
                .await?;
            }
            Some(Commands::Create { filename }) => {
                handle_create(filename)?;
            }
            Some(Commands::Plan {
                task_file,
                subtask,
                args,
            }) => {
                let (resolved_subtask, clean_args) =
                    extract_subtask_from_args(subtask.as_deref(), args);
                handle_plan(
                    task_file,
                    resolved_subtask.as_deref(),
                    &clean_args,
                    execution_options_from_cli(&cli),
                )
                .await?;
            }
            None => {
                if let Some(task) = &cli.task {
                    if dispatch_keyword_task_as_command(&cli, task).await? {
                        return Ok(());
                    }
                    let (subtask, clean_args) = extract_subtask_from_args(None, &cli.args);
                    smart_execute(
                        task,
                        subtask.as_deref(),
                        &clean_args,
                        execution_options_from_cli(&cli),
                    )
                    .await?;
                } else {
                    Cli::command().print_help()?;
                }
            }
        }
        Ok(())
    }
    .await;

    if cli.bill {
        print_billing_summary(get_billing_stats());
    }

    execution_result
}

#[cfg(test)]
mod tests {
    use super::{
        append_direct_tool_failure_note, compose_adhoc_prompt, ensure_non_empty_piped_stdin,
        parse_shorthand_args, PipedStdin,
    };

    #[test]
    fn test_compose_adhoc_prompt_without_stdin() {
        let prompt = compose_adhoc_prompt("Summarize this", None);
        assert_eq!(prompt, "Summarize this");
    }

    #[test]
    fn test_compose_adhoc_prompt_with_stdin() {
        let prompt = compose_adhoc_prompt("Summarize this", Some("input text\n"));
        assert_eq!(prompt, "Summarize this\n\ninput text");
    }

    #[test]
    fn test_append_direct_tool_failure_note_when_error_exists() {
        let prompt = append_direct_tool_failure_note(
            "weather in Shanghai".to_string(),
            Some("HTTP request failed"),
        );
        assert!(prompt.contains("weather in Shanghai"));
        assert!(prompt.contains("[Execution note]"));
        assert!(prompt.contains("HTTP request failed"));
    }

    #[test]
    fn test_append_direct_tool_failure_note_without_error() {
        let base = "weather in Shanghai".to_string();
        let prompt = append_direct_tool_failure_note(base.clone(), None);
        assert_eq!(prompt, base);
    }

    #[test]
    fn test_parse_shorthand_args_with_subtask() {
        let raw = vec!["#security".to_string(), "file.rs".to_string()];
        let (subtask, args) = parse_shorthand_args(&raw);

        assert_eq!(subtask.as_deref(), Some("security"));
        assert_eq!(args, vec!["file.rs".to_string()]);
    }

    #[test]
    fn test_parse_shorthand_args_without_subtask() {
        let raw = vec!["file.rs".to_string(), "strict".to_string()];
        let (subtask, args) = parse_shorthand_args(&raw);

        assert!(subtask.is_none());
        assert_eq!(args, raw);
    }

    #[test]
    fn test_empty_piped_stdin_returns_helpful_error() {
        let error = ensure_non_empty_piped_stdin(&PipedStdin::Empty).unwrap_err();
        let message = error.to_string();

        assert!(message.contains("Piped content is empty"));
        assert!(message.contains("Suggestions"));
        assert!(message.contains("curl -L"));
    }

    #[test]
    fn test_non_empty_piped_stdin_passes_validation() {
        let result = ensure_non_empty_piped_stdin(&PipedStdin::Content("text".to_string()));
        assert!(result.is_ok());
    }
}
