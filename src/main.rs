use anyhow::Context;
use clap::{CommandFactory, Parser, Subcommand};
use config::Config;
use dialoguer::{Confirm, Input, Select};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

mod agent;
mod config;
mod key_store;
mod permission;
mod provider_catalog;
mod providers;
mod task_parser;
mod template;
mod tools;

use providers::{get_billing_stats, reset_billing_stats, BillingStats, Provider};

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

    /// Do not ask for follow-up input when response is proceeding
    #[arg(short = 's', long, global = true)]
    silent: bool,

    /// Print API-call and token usage summary for this command
    #[arg(long, global = true)]
    bill: bool,

    /// Show detailed runtime logs (tool calls, prompts, provider responses)
    #[arg(long, alias = "log", global = true)]
    detail: bool,

    /// Ask provider to include thinking chain and display it
    #[arg(long, global = true)]
    think: bool,

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
        #[arg(long)]
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
        #[arg(long)]
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
    let provider = provider_catalog::normalize_provider_name(&config.provider)
        .unwrap_or_else(|| config.provider.trim().to_ascii_lowercase());
    if provider.is_empty() {
        anyhow::bail!(
            "No provider configured for profile '{}'. Run `rai start` or `rai config`.",
            config.profile
        );
    }

    let base_url = if config.provider_base_url.trim().is_empty() {
        None
    } else {
        Some(config.provider_base_url.trim())
    };

    match provider.as_str() {
        "poe" => Ok(Box::new(providers::poe::PoeProvider::new(&config.api_key))),
        "openai" => Ok(Box::new(providers::openai::OpenAiProvider::new(
            &config.api_key,
            base_url,
        )?)),
        "anthropic" => Ok(Box::new(providers::anthropic::AnthropicProvider::new(
            &config.api_key,
            base_url,
        )?)),
        "google" => Ok(Box::new(providers::google::GoogleProvider::new(
            &config.api_key,
            base_url,
        )?)),
        other if provider_catalog::provider_uses_openai_compatible_api(other) => Ok(Box::new(
            providers::openai_compatible::OpenAiCompatibleProvider::new(
                other,
                &config.api_key,
                base_url,
            )?,
        )),
        other => anyhow::bail!(
            "Provider '{}' is not supported. Supported: {}",
            other,
            provider_catalog::available_providers().join(", ")
        ),
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

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedTaskCliArgs {
    positional: Vec<String>,
    named: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct ResolvedTaskArguments {
    variables: HashMap<String, String>,
    arg_specs: Vec<template::ArgSpec>,
}

fn normalize_named_task_arg(name: &str) -> String {
    name.trim().replace('-', "_")
}

fn parse_task_cli_args(raw_args: &[String]) -> anyhow::Result<ParsedTaskCliArgs> {
    let mut parsed = ParsedTaskCliArgs::default();
    let mut index = 0usize;

    while index < raw_args.len() {
        let token = &raw_args[index];
        if token == "--" {
            parsed
                .positional
                .extend(raw_args.iter().skip(index + 1).cloned());
            break;
        }

        if let Some(body) = token.strip_prefix("--") {
            if body.is_empty() {
                anyhow::bail!("Invalid argument '--'.");
            }

            let (raw_name, raw_value) = if let Some((name, value)) = body.split_once('=') {
                (name, Some(value.to_string()))
            } else {
                (body, None)
            };

            let normalized_name = normalize_named_task_arg(raw_name);
            if normalized_name.is_empty() {
                anyhow::bail!("Invalid named argument '{}'.", token);
            }

            let value = if let Some(value) = raw_value {
                value
            } else {
                let Some(next_value) = raw_args.get(index + 1) else {
                    anyhow::bail!("Missing value for argument '--{}'.", raw_name);
                };
                if next_value == "--" || next_value.starts_with("--") {
                    anyhow::bail!("Missing value for argument '--{}'.", raw_name);
                }
                index += 1;
                next_value.clone()
            };

            if parsed.named.contains_key(&normalized_name) {
                anyhow::bail!("Duplicate named argument '--{}'.", raw_name);
            }
            parsed.named.insert(normalized_name, value);
        } else {
            parsed.positional.push(token.clone());
        }
        index += 1;
    }

    Ok(parsed)
}

fn build_effective_arg_specs(
    global_args: &[String],
    section_args: &[String],
    vars_in_template: &[String],
) -> anyhow::Result<Vec<template::ArgSpec>> {
    let declared_specs = template::collect_all_arg_specs(global_args, section_args)?;
    if declared_specs.is_empty() && !vars_in_template.is_empty() {
        Ok(vars_in_template
            .iter()
            .map(|name| template::ArgSpec {
                name: name.clone(),
                required: true,
            })
            .collect())
    } else {
        Ok(declared_specs)
    }
}

fn resolve_task_arguments(
    global_args: &[String],
    section_args: &[String],
    template_content: &str,
    raw_args: &[String],
    interactive: bool,
) -> anyhow::Result<ResolvedTaskArguments> {
    let vars_in_template = template::find_variables(template_content);
    let arg_specs = build_effective_arg_specs(global_args, section_args, &vars_in_template)?;
    let allowed_names: HashSet<String> = arg_specs.iter().map(|spec| spec.name.clone()).collect();
    let parsed_cli_args = parse_task_cli_args(raw_args)?;

    if !parsed_cli_args.named.is_empty() {
        let mut unknown: Vec<String> = parsed_cli_args
            .named
            .keys()
            .filter(|name| !allowed_names.contains(*name))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            unknown.sort();
            let mut expected = template::arg_names(&arg_specs);
            expected.sort();
            anyhow::bail!(
                "Unknown argument(s): {}. Available task arguments: {}",
                unknown
                    .iter()
                    .map(|name| format!("--{}", name))
                    .collect::<Vec<_>>()
                    .join(", "),
                if expected.is_empty() {
                    "(none)".to_string()
                } else {
                    expected
                        .iter()
                        .map(|name| format!("--{}", name))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
        }
    }

    let arg_names = template::arg_names(&arg_specs);
    let mut variables = template::map_args_to_variables(&arg_names, &parsed_cli_args.positional)?;

    for (name, value) in &parsed_cli_args.named {
        variables.insert(name.clone(), value.clone());
    }

    // Keep numeric placeholders aligned with declared argument order.
    for (index, arg_spec) in arg_specs.iter().enumerate() {
        if let Some(value) = variables.get(&arg_spec.name).cloned() {
            variables.insert((index + 1).to_string(), value);
        }
    }

    // Optional declared arguments default to empty string when omitted.
    for arg_spec in &arg_specs {
        if !arg_spec.required && !variables.contains_key(&arg_spec.name) {
            variables.insert(arg_spec.name.clone(), String::new());
        }
    }

    let mut missing_required = template::required_arg_names(&arg_specs)
        .into_iter()
        .filter(|name| {
            variables
                .get(name)
                .map(|value| value.trim().is_empty())
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    if !missing_required.is_empty() {
        if interactive {
            for name in missing_required.drain(..) {
                let value: String = Input::new()
                    .with_prompt(format!("Enter value for '{}'", name))
                    .interact_text()?;
                variables.insert(name, value);
            }
        } else {
            missing_required.sort();
            anyhow::bail!(
                "Missing arguments. Required: {}. Provide values positionally or via named flags (e.g., {}).",
                missing_required.join(", "),
                missing_required
                    .iter()
                    .map(|name| format!("--{} <value>", name))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }

    let mut missing_template_vars = vars_in_template
        .iter()
        .filter(|name| !variables.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();

    if !missing_template_vars.is_empty() {
        if interactive {
            for name in missing_template_vars.drain(..) {
                let value: String = Input::new()
                    .with_prompt(format!("Enter value for '{}'", name))
                    .interact_text()?;
                variables.insert(name, value);
            }
        } else {
            missing_template_vars.sort();
            anyhow::bail!(
                "Missing arguments. Expected values for: {}. Provide them positionally or with --name <value>.",
                missing_template_vars.join(", ")
            );
        }
    }

    Ok(ResolvedTaskArguments {
        variables,
        arg_specs,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssistantStatus {
    Success,
    SuccessWithWarnings,
    SuccessButCanGoDeeper,
    FailedAndEndTheLoop,
    FailedButNeedFurtherSteps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserFinalState {
    Success,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FollowupRequest {
    prompt: String,
    options: Vec<String>,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResponseDirective {
    Done,
    NeedsInput(FollowupRequest),
}

#[derive(Debug, Clone)]
struct ParsedAssistantPayload {
    state: AssistantStatus,
    output: String,
    description: String,
    arguments: Option<serde_json::Value>,
    thinking: Option<String>,
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
    detail_enabled: bool,
    think_enabled: bool,
    silent_enabled: bool,
}

fn execution_options_from_cli(cli: &Cli) -> ExecutionOptions<'_> {
    ExecutionOptions {
        model_override: cli.model.as_deref(),
        profile_override: cli.profile.as_deref(),
        cli_no_tools: cli.no_tools,
        cli_auto_approve: cli.yes,
        detail_enabled: cli.detail,
        think_enabled: cli.think,
        silent_enabled: cli.silent,
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
    let piped_stdin = if is_file {
        PipedStdin::NotPiped
    } else {
        read_piped_stdin()?
    };

    if !is_file && std::env::var("CI").is_err() {
        ensure_non_empty_piped_stdin(&piped_stdin)?;
    }

    let mut config = Config::load(opts.profile_override)?;
    if let Some(normalized) = provider_catalog::normalize_provider_name(&config.provider) {
        config.provider = normalized;
    }
    config.resolve_api_key()?;

    // If current provider has no key, use the first provider that has credentials (env or keyring).
    if config.api_key.is_empty() && provider_catalog::provider_requires_api_key(&config.provider) {
        for provider in available_providers() {
            if !provider_catalog::provider_requires_api_key(provider) {
                continue;
            }
            if provider_catalog::provider_supports_base_url(provider)
                && provider_catalog::provider_default_base_url(provider).is_none()
            {
                continue;
            }
            if let Some(key) = get_key_for_provider(&config.profile, provider) {
                config.provider = provider.to_string();
                config.providers = vec![provider.to_string()];
                config.default_provider = Some(provider.to_string());
                config.provider_base_url.clear();
                config.api_key = key;
                break;
            }
        }
    }

    if config.api_key.is_empty() && provider_catalog::provider_requires_api_key(&config.provider) {
        anyhow::bail!(
            "No API key found. Set RAI_API_KEY or a provider-specific env var (e.g. POE_API_KEY, OPENAI_API_KEY), add a .env file, or run `rai config` to save a key to keyring."
        );
    }

    let (prompt, model) = if is_file {
        let parsed = task_parser::parse_task_file(task_path)?;
        let section = parsed.get_section(subtask)?;
        let resolved = resolve_task_arguments(
            &parsed.global_frontmatter.args,
            &section.frontmatter.args,
            &section.content,
            args,
            is_interactive(),
        )?;
        let variables = resolved.variables;

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
        let prompt = compose_adhoc_prompt(task, piped_content);
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

    let mut current_prompt = prompt;
    if use_agent {
        let builtin = tools::builtin_tools();
        let agent_config = agent::AgentConfig {
            auto_approve,
            detail_enabled: opts.detail_enabled,
            think_enabled: opts.think_enabled,
            silent_enabled: opts.silent_enabled,
            ..Default::default()
        };
        let mut agent_loop = agent::Agent::new(provider_impl, model, builtin, agent_config);

        loop {
            let agent_prompt =
                apply_status_contract_prompt(current_prompt.clone(), opts.silent_enabled);
            let response = agent_loop.run(&agent_prompt).await?;
            match print_and_validate_response(&response, opts.think_enabled, opts.silent_enabled)? {
                ResponseDirective::Done => break,
                ResponseDirective::NeedsInput(request) => {
                    let input = collect_followup_input(&request, opts.silent_enabled)?;
                    current_prompt
                        .push_str(&format!("\n\n[Additional user input]\n{}\n", input.trim()));
                }
            }
        }
    } else {
        let mut exchange_number = 0usize;
        loop {
            exchange_number += 1;
            let provider_prompt = apply_think_mode_prompt(
                apply_status_contract_prompt(current_prompt.clone(), opts.silent_enabled),
                opts.think_enabled,
            );
            if opts.detail_enabled {
                print_info(&format!("Sending request to {}...", config.provider));
                print_detail_prompt(exchange_number, &provider_prompt);
            }
            let response = provider_impl.chat(&model, &provider_prompt).await?;
            if opts.detail_enabled {
                print_detail_response(exchange_number, &response);
            }
            match print_and_validate_response(&response, opts.think_enabled, opts.silent_enabled)? {
                ResponseDirective::Done => break,
                ResponseDirective::NeedsInput(request) => {
                    let input = collect_followup_input(&request, opts.silent_enabled)?;
                    current_prompt
                        .push_str(&format!("\n\n[Additional user input]\n{}\n", input.trim()));
                }
            }
        }
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
    DetailPrompt,
    DetailResponse,
    Thinking,
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
        Style::DetailPrompt => "\x1b[34m",
        Style::DetailResponse => "\x1b[33m",
        Style::Thinking => "\x1b[90m",
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

fn print_detail_prompt(request_number: usize, message: &str) {
    println!(
        "{}[detail][request #{}]{} {}",
        style_code(Style::DetailPrompt),
        request_number,
        style_code(Style::Reset),
        message
    );
}

fn print_detail_response(response_number: usize, message: &str) {
    println!(
        "{}[detail][response #{}]{} {}",
        style_code(Style::DetailResponse),
        response_number,
        style_code(Style::Reset),
        message
    );
}

fn print_thinking(message: &str) {
    println!(
        "{}[thinking]{} {}",
        style_code(Style::Thinking),
        style_code(Style::Reset),
        message
    );
}

fn apply_think_mode_prompt(base_prompt: String, think_enabled: bool) -> String {
    if !think_enabled {
        return base_prompt;
    }
    format!(
        "{}\n\n[Think mode]\nBefore your final answer, include your reasoning chain inside <think>...</think> and then provide a concise final answer.",
        base_prompt
    )
}

fn apply_status_contract_prompt(base_prompt: String, silent_enabled: bool) -> String {
    let silent_rule = if silent_enabled {
        "Silent mode is enabled. Do not return `state: \"proceeding\"`; choose `success` or `fail`."
    } else {
        "If additional input/options are needed, return `state: \"proceeding\"` with `arguments` guidance."
    };
    format!(
        "{}\n\n[Output contract]\nReturn ONLY valid JSON (no markdown, no extra text):\n{{\n  \"state\": \"success\" | \"fail\" | \"proceeding\",\n  \"output\": \"<cli output string or empty>\",\n  \"description\": \"<human-readable explanation or error reason>\",\n  \"arguments\": {{\"prompt\":\"...\", \"options\":[\"...\",\"...\"]}} | \"prompt text\" | null,\n  \"thinking\": \"<optional, mainly when think mode is enabled>\"\n}}\n{}\nDo not include additional keys unless necessary.",
        base_prompt, silent_rule
    )
}

fn extract_thinking_blocks(response: &str) -> (Vec<String>, String) {
    static THINK_RE: OnceLock<Regex> = OnceLock::new();
    let think_re = THINK_RE
        .get_or_init(|| Regex::new(r"(?is)<think>(.*?)</think>").expect("valid think regex"));

    let thoughts = think_re
        .captures_iter(response)
        .filter_map(|capture| capture.get(1).map(|m| m.as_str().trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    let cleaned = think_re.replace_all(response, "").to_string();
    (thoughts, cleaned)
}

fn print_processed_response(response: &str, think_enabled: bool) {
    let (thoughts, cleaned) = extract_thinking_blocks(response);
    let has_thoughts = !thoughts.is_empty();
    if think_enabled {
        if !has_thoughts {
            print_thinking("No thinking chain returned by provider.");
        } else {
            for thought in &thoughts {
                print_thinking(thought);
            }
        }
    }

    let candidate = if !has_thoughts {
        response.trim().to_string()
    } else {
        cleaned.trim().to_string()
    };
    let visible = strip_internal_status_lines(&candidate);
    let visible = visible.trim();
    if visible.is_empty() {
        print_result(response.trim());
    } else {
        print_result(visible);
    }
}

fn parse_assistant_status(response: &str) -> Option<AssistantStatus> {
    if let Some(parsed) = parse_assistant_payload(response) {
        return Some(parsed.state);
    }

    let (_, cleaned) = extract_thinking_blocks(response);
    for line in cleaned.lines().take(12) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(status) = parse_status_line(trimmed) {
            return Some(status);
        }
    }
    None
}

fn parse_status_line(trimmed_line: &str) -> Option<AssistantStatus> {
    let (label, value) = trimmed_line.split_once(':')?;
    let label_normalized = label.trim().to_ascii_lowercase();
    if label_normalized != "status" && label_normalized != "state" {
        return None;
    }
    let normalized = value.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    match normalized.as_str() {
        "success" => Some(AssistantStatus::Success),
        "fail" => Some(AssistantStatus::FailedAndEndTheLoop),
        "proceeding" => Some(AssistantStatus::FailedButNeedFurtherSteps),
        "success_with_warnings" => Some(AssistantStatus::SuccessWithWarnings),
        "success_but_can_go_deeper" => Some(AssistantStatus::SuccessButCanGoDeeper),
        "failed_and_end_the_loop" => Some(AssistantStatus::FailedAndEndTheLoop),
        "failed_but_need_further_steps" => Some(AssistantStatus::FailedButNeedFurtherSteps),
        _ => None,
    }
}

fn strip_internal_status_lines(text: &str) -> String {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let trimmed = line.trim();
            if index < 12 && parse_status_line(trimmed).is_some() {
                None
            } else {
                Some(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_assistant_payload(response: &str) -> Option<ParsedAssistantPayload> {
    let (_, cleaned) = extract_thinking_blocks(response);
    parse_assistant_payload_from_text(cleaned.trim())
}

fn parse_assistant_payload_from_text(text: &str) -> Option<ParsedAssistantPayload> {
    let value = parse_json_like_object(text)?;
    let state_text = value
        .get("state")
        .and_then(|v| v.as_str())
        .map(str::trim)?
        .to_ascii_lowercase();
    let state = match state_text.as_str() {
        "success" => AssistantStatus::Success,
        "fail" => AssistantStatus::FailedAndEndTheLoop,
        "proceeding" => AssistantStatus::FailedButNeedFurtherSteps,
        _ => return None,
    };

    Some(ParsedAssistantPayload {
        state,
        output: extract_string_field(value.get("output")),
        description: extract_string_field(value.get("description")),
        arguments: value.get("arguments").cloned(),
        thinking: value
            .get("thinking")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

fn parse_json_like_object(text: &str) -> Option<serde_json::Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if value.is_object() {
            return Some(value);
        }
    }

    if let (Some(first), Some(last)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if last > first {
            let candidate = &trimmed[first..=last];
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(candidate) {
                if value.is_object() {
                    return Some(value);
                }
            }
        }
    }

    None
}

fn extract_string_field(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) if !other.is_null() => other.to_string(),
        _ => String::new(),
    }
}

fn build_followup_request(payload: &ParsedAssistantPayload) -> FollowupRequest {
    let mut prompt = if !payload.description.trim().is_empty() {
        payload.description.clone()
    } else {
        "Additional input is required to continue.".to_string()
    };
    let mut options = Vec::new();

    if let Some(arguments) = payload.arguments.as_ref() {
        match arguments {
            serde_json::Value::String(s) => {
                if !s.trim().is_empty() {
                    prompt = s.clone();
                }
            }
            serde_json::Value::Object(map) => {
                for key in ["prompt", "question", "message"] {
                    if let Some(text) = map.get(key).and_then(|v| v.as_str()) {
                        if !text.trim().is_empty() {
                            prompt = text.to_string();
                            break;
                        }
                    }
                }
                for key in ["options", "choices"] {
                    if let Some(items) = map.get(key).and_then(|v| v.as_array()) {
                        options = items
                            .iter()
                            .filter_map(|item| item.as_str())
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>();
                        if !options.is_empty() {
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    FollowupRequest {
        prompt,
        options,
        description: payload.description.clone(),
    }
}

fn collect_followup_input(
    request: &FollowupRequest,
    silent_enabled: bool,
) -> anyhow::Result<String> {
    if silent_enabled || !is_interactive() {
        anyhow::bail!("Assistant requires additional input, but follow-up prompting is disabled.");
    }

    if !request.options.is_empty() {
        let selection = Select::new()
            .with_prompt(&request.prompt)
            .items(&request.options)
            .default(0)
            .interact_opt()?;
        let Some(index) = selection else {
            anyhow::bail!("Cancelled follow-up input.");
        };
        return Ok(request.options[index].clone());
    }

    let value: String = Input::new().with_prompt(&request.prompt).interact_text()?;
    Ok(value)
}

fn response_has_failure_language(response: &str) -> bool {
    let (_, cleaned) = extract_thinking_blocks(response);
    let lowercase = cleaned.to_ascii_lowercase();
    [
        "i couldn't retrieve",
        "i could not retrieve",
        "i was unable to retrieve",
        "unable to retrieve",
        "i couldn't access",
        "i could not access",
        "unable to access",
        "failed to retrieve",
        "failed to access",
        "unable to complete this request",
    ]
    .iter()
    .any(|needle| lowercase.contains(needle))
}

fn derive_user_final_state(response: &str) -> UserFinalState {
    match parse_assistant_status(response) {
        Some(AssistantStatus::Success)
        | Some(AssistantStatus::SuccessWithWarnings)
        | Some(AssistantStatus::SuccessButCanGoDeeper) => UserFinalState::Success,
        Some(AssistantStatus::FailedAndEndTheLoop)
        | Some(AssistantStatus::FailedButNeedFurtherSteps) => UserFinalState::Fail,
        None => {
            if response_has_failure_language(response) {
                UserFinalState::Fail
            } else {
                UserFinalState::Success
            }
        }
    }
}

fn print_and_validate_response(
    response: &str,
    think_enabled: bool,
    silent_enabled: bool,
) -> anyhow::Result<ResponseDirective> {
    let (thoughts, _cleaned) = extract_thinking_blocks(response);
    if think_enabled {
        if thoughts.is_empty() {
            if let Some(parsed) = parse_assistant_payload(response) {
                if let Some(thinking) = parsed.thinking {
                    if !thinking.trim().is_empty() {
                        print_thinking(&thinking);
                    } else {
                        print_thinking("No thinking chain returned by provider.");
                    }
                } else {
                    print_thinking("No thinking chain returned by provider.");
                }
            } else {
                print_thinking("No thinking chain returned by provider.");
            }
        } else {
            for thought in thoughts {
                print_thinking(&thought);
            }
        }
    }

    if let Some(parsed) = parse_assistant_payload(response) {
        match parsed.state {
            AssistantStatus::Success
            | AssistantStatus::SuccessWithWarnings
            | AssistantStatus::SuccessButCanGoDeeper => {
                let visible = if !parsed.output.trim().is_empty() {
                    parsed.output
                } else {
                    parsed.description
                };
                if !visible.trim().is_empty() {
                    print_result(visible.trim());
                }
                return Ok(ResponseDirective::Done);
            }
            AssistantStatus::FailedAndEndTheLoop => {
                let message = if !parsed.description.trim().is_empty() {
                    parsed.description
                } else if !parsed.output.trim().is_empty() {
                    parsed.output
                } else {
                    "Request failed.".to_string()
                };
                print_result(message.trim());
                anyhow::bail!(
                    "Assistant response indicates failure. Returning non-zero exit code."
                );
            }
            AssistantStatus::FailedButNeedFurtherSteps => {
                let followup = build_followup_request(&parsed);
                if silent_enabled || !is_interactive() {
                    let message = if !followup.description.trim().is_empty() {
                        followup.description
                    } else {
                        "Additional input is required but prompting is disabled.".to_string()
                    };
                    print_result(message.trim());
                    anyhow::bail!(
                        "Assistant requires additional input, but follow-up prompting is disabled."
                    );
                }
                if !followup.description.trim().is_empty() {
                    print_info(followup.description.trim());
                }
                return Ok(ResponseDirective::NeedsInput(followup));
            }
        }
    }

    print_processed_response(response, think_enabled);
    match derive_user_final_state(response) {
        UserFinalState::Success => Ok(ResponseDirective::Done),
        UserFinalState::Fail => {
            anyhow::bail!("Assistant response indicates failure. Returning non-zero exit code.")
        }
    }
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

    // Verify we can immediately read the just-saved key to catch keyring issues early.
    #[cfg(not(test))]
    {
        if get_api_key_helper(&scoped_provider).is_err() {
            anyhow::bail!(
                "API key was not readable from keyring after save. Check OS keyring access permissions and try again."
            );
        }
    }

    Ok(())
}

/// Returns the API key for a provider from env vars only (no keyring).
fn get_key_for_provider_from_env(provider: &str) -> Option<String> {
    let provider = provider_catalog::normalize_provider_name(provider)
        .unwrap_or_else(|| provider.trim().to_ascii_lowercase());

    for name in provider_catalog::provider_env_vars(&provider) {
        if let Ok(v) = std::env::var(name) {
            if !v.trim().is_empty() {
                return Some(v);
            }
        }
    }

    let generic_name = provider_catalog::generic_provider_env_var(&provider)?;
    std::env::var(generic_name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Key source for provider selector labels.
#[cfg_attr(test, allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProviderKeySource {
    Env,
    Keyring,
}

#[cfg(not(test))]
fn profile_provider_key_source(profile: &str, provider: &str) -> Option<ProviderKeySource> {
    if get_key_for_provider_from_env(provider).is_some() {
        return Some(ProviderKeySource::Env);
    }
    let scoped_provider = format!("{}:{}", profile, provider);
    if let Ok(key) = get_api_key_helper(&scoped_provider) {
        if !key.trim().is_empty() {
            return Some(ProviderKeySource::Keyring);
        }
    }
    if let Ok(key) = get_api_key_helper(provider) {
        if !key.trim().is_empty() {
            return Some(ProviderKeySource::Keyring);
        }
    }
    None
}

#[cfg(test)]
fn profile_provider_key_source(_profile: &str, _provider: &str) -> Option<ProviderKeySource> {
    None
}

/// Resolves API key for (profile, provider) from env then keyring. Used so rai run can use a provider with existing credentials.
fn get_key_for_provider(profile: &str, provider: &str) -> Option<String> {
    #[cfg(test)]
    let _ = profile;

    if let Some(key) = get_key_for_provider_from_env(provider) {
        return Some(key);
    }
    #[cfg(not(test))]
    {
        let scoped = format!("{}:{}", profile, provider);
        if let Ok(key) = get_api_key_helper(&scoped) {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
        if let Ok(key) = get_api_key_helper(provider) {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
    }
    None
}

#[cfg(not(test))]
fn profile_provider_has_saved_key(profile: &str, provider: &str) -> bool {
    profile_provider_key_source(profile, provider).is_some()
}

#[cfg(test)]
fn profile_provider_has_saved_key(_profile: &str, _provider: &str) -> bool {
    false
}

fn available_providers() -> Vec<&'static str> {
    provider_catalog::available_providers()
}

fn provider_requires_explicit_base_url(provider: &str) -> bool {
    provider_catalog::provider_supports_base_url(provider)
        && provider_catalog::provider_default_base_url(provider).is_none()
}

fn configure_provider_base_url(config: &mut Config, provider: &str) -> anyhow::Result<()> {
    if !provider_catalog::provider_supports_base_url(provider) {
        config.provider_base_url.clear();
        return Ok(());
    }

    let default_base_url = provider_catalog::provider_default_base_url(provider).unwrap_or("");
    let prompt = if provider_requires_explicit_base_url(provider) {
        "Base URL (required, e.g. https://api.openai.com/v1)"
    } else {
        "Base URL (press Enter to accept default)"
    };

    let initial_value = if !config.provider_base_url.trim().is_empty() {
        config.provider_base_url.clone()
    } else {
        default_base_url.to_string()
    };

    loop {
        let input: String = Input::new()
            .with_prompt(prompt)
            .default(initial_value.clone())
            .allow_empty(true)
            .interact_text()?;
        let trimmed = input.trim().to_string();

        if trimmed.is_empty() && provider_requires_explicit_base_url(provider) {
            println!(
                "Provider '{}' requires an explicit base URL. Enter one or press Ctrl+C to cancel.",
                provider
            );
            continue;
        }

        config.provider_base_url = trimmed;
        if config.provider_base_url.is_empty() && !default_base_url.is_empty() {
            println!("Using provider default base URL: {}", default_base_url);
        }
        break;
    }

    Ok(())
}

fn configure_provider_and_key(config: &mut Config, require_key: bool) -> anyhow::Result<()> {
    print_section("Provider");
    let providers = available_providers();
    let active_provider = provider_catalog::normalize_provider_name(&config.provider)
        .unwrap_or_else(|| config.provider.clone());
    let default_idx = providers
        .iter()
        .position(|provider| *provider == active_provider)
        .unwrap_or(0);
    let provider_labels: Vec<String> = providers
        .iter()
        .map(
            |provider| match profile_provider_key_source(&config.profile, provider) {
                Some(ProviderKeySource::Env) => format!("{} [read from env]", provider),
                Some(ProviderKeySource::Keyring) => format!("{} [configured]", provider),
                None => format!("{} [not configured]", provider),
            },
        )
        .collect();
    println!(
        "Hint: [read from env] = key from OPENAI_API_KEY/POE_API_KEY/etc.; [configured] = key in OS keyring."
    );
    let selection = Select::new()
        .with_prompt("Select provider")
        .items(&provider_labels)
        .default(default_idx)
        .interact_opt()?;
    let provider = match selection {
        Some(index) => providers[index].to_string(),
        None => {
            println!("No changes made.");
            return Ok(());
        }
    };
    let provider = provider_catalog::normalize_provider_name(&provider).unwrap_or(provider);

    if provider != active_provider {
        config.provider_base_url.clear();
    }
    config.provider = provider.clone();
    config.providers = vec![provider.clone()];
    config.default_provider = Some(provider.clone());
    configure_provider_base_url(config, &provider)?;

    let requires_key = provider_catalog::provider_requires_api_key(&provider);
    if !requires_key {
        println!(
            "Provider '{}' can run without an API key. If your endpoint needs auth, enter a key below.",
            provider
        );
    }

    let has_saved_key = profile_provider_has_saved_key(&config.profile, &provider);
    println!(
        "Security: API keys are stored in your OS keyring (secure encrypted credential store), not in plain-text config files."
    );
    loop {
        let key_prompt = if has_saved_key || !require_key || !requires_key {
            "API key (leave empty to keep current keyring value)"
        } else {
            "API key (required for setup)"
        };
        let api_key: String = Input::new()
            .with_prompt(key_prompt)
            .allow_empty(true)
            .interact_text()?;
        if !api_key.trim().is_empty() {
            set_profile_api_key(&config.profile, &provider, &api_key)?;
            println!(
                "Saved API key for provider '{}' in OS keyring (profile '{}').",
                provider, config.profile
            );
            break;
        }
        if has_saved_key || !require_key || !requires_key {
            break;
        }
        println!(
            "An API key is required to finish `rai start`. Enter one now or press Ctrl+C to cancel setup."
        );
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
    let autosave = |config: &Config| -> anyhow::Result<()> {
        config.save()?;
        Config::set_active_profile(&config.profile)?;
        Ok(())
    };
    loop {
        print_section("Configuration");
        let options = vec![
            "Provider & API key",
            "Model defaults",
            "Tools",
            "Profiles",
            "Exit config",
        ];
        let selection = Select::new()
            .with_prompt(format!("Editing profile: {}", config.profile))
            .items(&options)
            .default(0)
            .interact_opt()?;

        match selection {
            Some(0) => {
                configure_provider_and_key(&mut config, false)?;
                autosave(&config)?;
            }
            Some(1) => {
                configure_model_defaults(&mut config)?;
                autosave(&config)?;
            }
            Some(2) => {
                configure_tools(&mut config)?;
                autosave(&config)?;
            }
            Some(3) => {
                configure_profiles_menu(&mut config)?;
                autosave(&config)?;
            }
            Some(4) => {
                println!("Exited config.");
                return Ok(());
            }
            None => {
                println!("Exited config.");
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

    configure_provider_and_key(&mut config, true)?;
    configure_model_defaults(&mut config)?;
    config.save()?;
    Config::set_active_profile(&config.profile)?;

    println!("\nSaved profile '{}'.", config.profile);
    println!("Try: rai run \"Hello world\"");

    let choices = vec!["Finish", "More settings"];
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
            println!(
                "Provider base URL: {}",
                if config.provider_base_url.trim().is_empty() {
                    "(provider default)"
                } else {
                    config.provider_base_url.as_str()
                }
            );
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
    let resolved = resolve_task_arguments(
        &parsed.global_frontmatter.args,
        &section.frontmatter.args,
        &section.content,
        prefilled_args,
        true,
    )?;
    let mapped = resolved.variables;

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

    let mut final_args: Vec<String> = Vec::new();
    for arg_spec in &resolved.arg_specs {
        if let Some(value) = mapped.get(&arg_spec.name) {
            if arg_spec.required || !value.is_empty() {
                final_args.push(format!("--{}", arg_spec.name));
                final_args.push(value.clone());
            }
        }
    }

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
    // Load .env from current directory so POE_API_KEY etc. are available
    let _ = dotenvy::dotenv();

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
        apply_status_contract_prompt, apply_think_mode_prompt, compose_adhoc_prompt,
        derive_user_final_state, ensure_non_empty_piped_stdin, extract_thinking_blocks,
        parse_assistant_payload, parse_assistant_status, parse_shorthand_args, parse_task_cli_args,
        print_and_validate_response, resolve_provider, resolve_task_arguments,
        response_has_failure_language, strip_internal_status_lines, AssistantStatus, Cli, Config,
        PipedStdin, ResponseDirective, UserFinalState,
    };
    use clap::Parser;

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

    #[test]
    fn test_apply_think_mode_prompt_adds_instruction_when_enabled() {
        let prompt = apply_think_mode_prompt("Explain Rust ownership".to_string(), true);
        assert!(prompt.contains("[Think mode]"));
        assert!(prompt.contains("<think>...</think>"));
    }

    #[test]
    fn test_apply_status_contract_prompt_adds_status_requirements() {
        let prompt = apply_status_contract_prompt("Solve this".to_string(), false);
        assert!(prompt.contains("[Output contract]"));
        assert!(prompt.contains("\"state\": \"success\" | \"fail\" | \"proceeding\""));
        assert!(prompt.contains("additional input"));
    }

    #[test]
    fn test_extract_thinking_blocks_splits_reasoning_and_final_answer() {
        let response = "<think>step 1\nstep 2</think>\nFinal answer";
        let (thoughts, cleaned) = extract_thinking_blocks(response);
        assert_eq!(thoughts, vec!["step 1\nstep 2".to_string()]);
        assert_eq!(cleaned.trim(), "Final answer");
    }

    #[test]
    fn test_parse_assistant_status_from_response() {
        let response = "STATUS: success_with_warnings\nDone.";
        assert_eq!(
            parse_assistant_status(response),
            Some(AssistantStatus::SuccessWithWarnings)
        );
    }

    #[test]
    fn test_parse_assistant_status_ignores_think_block() {
        let response = "<think>draft</think>\nSTATUS: failed_but_need_further_steps\nRetry";
        assert_eq!(
            parse_assistant_status(response),
            Some(AssistantStatus::FailedButNeedFurtherSteps)
        );
    }

    #[test]
    fn test_strip_internal_status_lines_removes_status_for_user_output() {
        let text = "STATUS: success_with_warnings\nResult line";
        assert_eq!(strip_internal_status_lines(text), "Result line");
    }

    #[test]
    fn test_failure_language_detection_for_inability_reply() {
        let response = "I couldn't retrieve the current weather for Shanghai.";
        assert!(response_has_failure_language(response));
    }

    #[test]
    fn test_print_and_validate_response_fails_on_failed_status() {
        let response =
            r#"{"state":"fail","output":"","description":"I could not complete this task."}"#;
        let result = print_and_validate_response(response, false, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_print_and_validate_response_fails_on_failure_language_without_status() {
        let response = "I couldn't retrieve the current weather for Shanghai.";
        let result = print_and_validate_response(response, false, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_print_and_validate_response_accepts_success_status() {
        let response = r#"{"state":"success","output":"Hello","description":"Translated"}"#;
        let result = print_and_validate_response(response, false, false).expect("success");
        assert_eq!(result, ResponseDirective::Done);
    }

    #[test]
    fn test_derive_user_final_state_maps_internal_status_to_fail() {
        let response = r#"{"state":"fail","output":"","description":"Cannot complete"}"#;
        assert_eq!(derive_user_final_state(response), UserFinalState::Fail);
    }

    #[test]
    fn test_parse_assistant_payload_from_json_contract() {
        let response = r#"{"state":"proceeding","output":"","description":"Need target text","arguments":{"prompt":"Enter text","options":["A","B"]}}"#;
        let parsed = parse_assistant_payload(response).expect("payload should parse");
        assert_eq!(parsed.state, AssistantStatus::FailedButNeedFurtherSteps);
        assert_eq!(parsed.description, "Need target text");
    }

    #[test]
    fn test_cli_parses_detail_flag() {
        let cli = Cli::parse_from(["rai", "--detail", "run", "hello"]);
        assert!(cli.detail);
    }

    #[test]
    fn test_cli_parses_legacy_log_alias_as_detail() {
        let cli = Cli::parse_from(["rai", "--log", "run", "hello"]);
        assert!(cli.detail);
    }

    #[test]
    fn test_cli_parses_think_flag() {
        let cli = Cli::parse_from(["rai", "--think", "run", "hello"]);
        assert!(cli.think);
    }

    #[test]
    fn test_cli_parses_silent_flag() {
        let cli = Cli::parse_from(["rai", "-s", "run", "hello"]);
        assert!(cli.silent);
    }

    #[test]
    fn test_parse_task_cli_args_supports_named_and_positional() {
        let raw = vec![
            "--input".to_string(),
            "source.md".to_string(),
            "extra-positional".to_string(),
            "--output=target/output.rtf".to_string(),
        ];
        let parsed = parse_task_cli_args(&raw).expect("task args should parse");
        assert_eq!(parsed.positional, vec!["extra-positional".to_string()]);
        assert_eq!(parsed.named.get("input"), Some(&"source.md".to_string()));
        assert_eq!(
            parsed.named.get("output"),
            Some(&"target/output.rtf".to_string())
        );
    }

    #[test]
    fn test_parse_task_cli_args_missing_value_errors() {
        let raw = vec!["--input".to_string()];
        let error = parse_task_cli_args(&raw).unwrap_err();
        assert!(error.to_string().contains("Missing value"));
    }

    #[test]
    fn test_resolve_task_arguments_named_flags_with_optional_args() {
        let global = vec![
            "input".to_string(),
            "output".to_string(),
            "input_format?".to_string(),
            "output_format?".to_string(),
        ];
        let section = vec![];
        let content =
            "Convert {{ input }} to {{ output }} ({{ input_format }} -> {{ output_format }})";
        let raw = vec![
            "--input".to_string(),
            "demo/source.md".to_string(),
            "--output".to_string(),
            "target/out.rtf".to_string(),
        ];

        let resolved = resolve_task_arguments(&global, &section, content, &raw, false)
            .expect("named flags with optional args should resolve");
        assert_eq!(
            resolved.variables.get("input"),
            Some(&"demo/source.md".to_string())
        );
        assert_eq!(
            resolved.variables.get("output"),
            Some(&"target/out.rtf".to_string())
        );
        assert_eq!(
            resolved.variables.get("input_format"),
            Some(&"".to_string())
        );
        assert_eq!(
            resolved.variables.get("output_format"),
            Some(&"".to_string())
        );
    }

    #[test]
    fn test_resolve_task_arguments_missing_required_errors() {
        let global = vec!["input".to_string(), "output".to_string()];
        let section = vec![];
        let content = "Convert {{ input }} to {{ output }}";
        let raw = vec!["--input".to_string(), "demo/source.md".to_string()];

        let error = resolve_task_arguments(&global, &section, content, &raw, false).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("Missing arguments"));
        assert!(message.contains("output"));
    }

    #[test]
    fn test_resolve_task_arguments_unknown_named_argument_errors() {
        let global = vec!["input".to_string(), "output".to_string()];
        let section = vec![];
        let content = "Convert {{ input }} to {{ output }}";
        let raw = vec![
            "--input".to_string(),
            "demo/source.md".to_string(),
            "--target".to_string(),
            "target/out.rtf".to_string(),
        ];

        let error = resolve_task_arguments(&global, &section, content, &raw, false).unwrap_err();
        assert!(error.to_string().contains("Unknown argument"));
    }

    fn provider_test_config(provider: &str, api_key: &str, base_url: &str) -> Config {
        Config {
            profile: "default".to_string(),
            provider: provider.to_string(),
            providers: vec![provider.to_string()],
            default_provider: Some(provider.to_string()),
            default_model: "gpt-4o".to_string(),
            provider_base_url: base_url.to_string(),
            tool_mode: "ask".to_string(),
            no_tools: false,
            auto_approve: false,
            api_key: api_key.to_string(),
        }
    }

    #[test]
    fn test_resolve_provider_accepts_ollama_without_api_key() {
        let config = provider_test_config("ollama", "", "");
        assert!(resolve_provider(&config).is_ok());
    }

    #[test]
    fn test_resolve_provider_requires_base_url_for_openai_compatible() {
        let config = provider_test_config("openai-compatible", "dummy-key", "");
        let result = resolve_provider(&config);
        assert!(result.is_err(), "base URL should be required");
        let message = result
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(message.contains("requires a base URL"));
    }

    #[test]
    fn test_resolve_provider_supports_anthropic_when_api_key_present() {
        let config = provider_test_config("anthropic", "dummy-key", "");
        assert!(resolve_provider(&config).is_ok());
    }

    #[test]
    fn test_resolve_provider_supports_openai_when_api_key_present() {
        let config = provider_test_config("openai", "dummy-key", "");
        assert!(resolve_provider(&config).is_ok());
    }
}
