use anyhow::Context;
use clap::{Parser, Subcommand};
use config::Config;
use dialoguer::{Confirm, Input, Select};
use std::io::Read;
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

use providers::Provider;

fn set_api_key_helper(provider: &str, api_key: &str) -> anyhow::Result<()> {
    key_store::set_api_key(provider, api_key)
}

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
    /// Configure AI model provider and other settings
    Config,
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

fn resolve_provider(config: &Config) -> anyhow::Result<Box<dyn Provider>> {
    let provider = config.provider.trim().to_lowercase();
    if provider.is_empty() {
        anyhow::bail!(
            "No provider configured. Set `providers` in ~/.config/rai/config.toml or run `rai config`."
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

async fn handle_run(
    task: &str,
    subtask: Option<&str>,
    args: &[String],
    model_override: Option<&str>,
    use_agent: bool,
    auto_approve: bool,
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

    let mut config = Config::load()?;
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

        let effective_model = model_override
            .map(|s| s.to_string())
            .or_else(|| parsed.effective_model(subtask))
            .unwrap_or(config.default_model.clone());

        info!("Task: {} (section: {})", task, section.name);
        (rendered, effective_model)
    } else {
        let model = model_override
            .map(|s| s.to_string())
            .unwrap_or(config.default_model.clone());
        let piped_content = match &piped_stdin {
            PipedStdin::Content(content) => Some(content.as_str()),
            PipedStdin::NotPiped | PipedStdin::Empty => None,
        };
        (compose_adhoc_prompt(task, piped_content), model)
    };

    let provider_impl = resolve_provider(&config)?;
    info!("Using provider: {}, model: {}", config.provider, model);

    if use_agent {
        let builtin = tools::builtin_tools();
        let agent_config = agent::AgentConfig {
            auto_approve,
            ..Default::default()
        };
        let mut agent_loop = agent::Agent::new(provider_impl, model, builtin, agent_config);

        let response = agent_loop.run(&prompt).await?;
        println!("{}", response);
    } else {
        println!("Sending request to {}...", config.provider);
        let response = provider_impl.chat(&model, &prompt).await?;
        println!("\nResponse:\n{}", response);
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

async fn handle_plan(
    task_file: &str,
    subtask: Option<&str>,
    prefilled_args: &[String],
    model_override: Option<&str>,
    use_agent: bool,
    auto_approve: bool,
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

    handle_run(
        task_file,
        subtask_opt,
        &final_args,
        model_override,
        use_agent,
        auto_approve,
    )
    .await
}

async fn smart_execute(
    task: &str,
    subtask: Option<&str>,
    args: &[String],
    model_override: Option<&str>,
    use_agent: bool,
    auto_approve: bool,
) -> anyhow::Result<()> {
    let task_path = Path::new(task);
    let is_file = task_path.exists() && task_path.is_file();

    if !is_file {
        return handle_run(task, subtask, args, model_override, use_agent, auto_approve).await;
    }

    let parsed = task_parser::parse_task_file(task_path)?;

    let section = match parsed.get_section(subtask) {
        Ok(s) => s,
        Err(_) if subtask.is_none() && !parsed.subtasks.is_empty() => {
            return handle_plan(task, subtask, args, model_override, use_agent, auto_approve).await;
        }
        Err(e) => return Err(e),
    };

    let vars = template::find_variables(&section.content);

    if vars.is_empty() || args.len() >= vars.len() {
        handle_run(task, subtask, args, model_override, use_agent, auto_approve).await
    } else if is_interactive() {
        handle_plan(task, subtask, args, model_override, use_agent, auto_approve).await
    } else {
        handle_run(task, subtask, args, model_override, use_agent, auto_approve).await
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

    let use_agent = !cli.no_tools;
    let auto_approve = cli.yes;

    match &cli.command {
        Some(Commands::Config) => {
            if !is_interactive() {
                anyhow::bail!(
                    "Cannot run `rai config` in non-interactive mode. \
                     Set RAI_API_KEY and configure via environment variables or config file."
                );
            }

            info!("Config command selected");
            let mut config = Config::load()?;

            let providers = vec!["poe", "openai", "anthropic", "google"];
            let default_provider_index = providers
                .iter()
                .position(|&p| p == config.provider)
                .unwrap_or(0);

            let selection = Select::new()
                .with_prompt("Select AI Provider")
                .items(&providers)
                .default(default_provider_index)
                .interact_opt()?;

            let provider = match selection {
                Some(index) => providers[index].to_string(),
                None => {
                    println!("Operation cancelled.");
                    return Ok(());
                }
            };
            config.provider = provider.clone();
            config.providers = vec![provider.clone()];
            config.default_provider = Some(provider.clone());

            let api_key: String = Input::new()
                .with_prompt("API Key (saved to system keyring)")
                .default(String::new())
                .interact_text()?;

            set_api_key_helper(&provider, &api_key).context("Failed to save API key to keyring")?;

            let default_model: String = Input::new()
                .with_prompt("Default Model")
                .default(if config.default_model.is_empty() {
                    "gpt-4o".to_string()
                } else {
                    config.default_model.clone()
                })
                .interact_text()?;
            config.default_model = default_model;

            config.save()?;
            println!("Configuration saved successfully!");
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
                cli.model.as_deref(),
                use_agent,
                auto_approve,
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
                cli.model.as_deref(),
                use_agent,
                auto_approve,
            )
            .await?;
        }
        None => {
            if let Some(task) = &cli.task {
                let (subtask, clean_args) = extract_subtask_from_args(None, &cli.args);
                smart_execute(
                    task,
                    subtask.as_deref(),
                    &clean_args,
                    cli.model.as_deref(),
                    use_agent,
                    auto_approve,
                )
                .await?;
            } else {
                use clap::CommandFactory;
                Cli::command().print_help()?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        compose_adhoc_prompt, ensure_non_empty_piped_stdin, parse_shorthand_args, PipedStdin,
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
