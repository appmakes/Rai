use clap::{Parser, Subcommand};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use dialoguer::{Input, Select};
use config::Config;
use anyhow::Context;

mod config;
mod key_store;
mod providers;

use providers::Provider;

fn set_api_key_helper(provider: &str, api_key: &str) -> anyhow::Result<()> {
    key_store::set_api_key(provider, api_key)
}

fn get_api_key_helper(provider: &str) -> anyhow::Result<String> {
    key_store::get_api_key(provider)
}

#[derive(Parser)]#[command(name = "rai")]
#[command(version)]
#[command(about = "A CLI tool to run AI tasks in terminal or CI/CD", long_about = None)]
struct Cli {
    /// Turn debugging information on
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Override the AI model to use (e.g., gpt-4o, kimi-k2)
    #[arg(short, long)]
    model: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Configure AI model provider and other settings
    Config,
    /// Run a task
    Run {
        /// The task description or file path
        #[arg(index = 1)]
        task: String,

        /// Optional sub-task name (e.g., #summary)
        #[arg(short, long)]
        subtask: Option<String>,

        /// Arguments for the task
        #[arg(last = true)]
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
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = match cli.verbose {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    if let Some(model) = cli.model.as_ref() {
        info!("Overriding model to: {}", model);
    }

    match &cli.command {
        Some(Commands::Config) => {
            info!("Config command selected");
            let mut config = Config::load()?;

            let providers = vec!["poe", "openai", "anthropic", "google"];
            let default_provider_index = providers.iter()
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

            let api_key: String = Input::new()
                .with_prompt("API Key (saved to system keyring)")
                .default(String::new())
                .interact_text()?;
            
            // Save API key to keyring
            set_api_key_helper(&provider, &api_key)
                .context("Failed to save API key to keyring")?;

            let default_model: String = Input::new()
                .with_prompt("Default Model")
                .default(if config.default_model.is_empty() { "gpt-4o".to_string() } else { config.default_model.clone() })
                .interact_text()?;
            config.default_model = default_model;

            config.save()?;
            println!("Configuration saved successfully!");
        }
        Some(Commands::Run { task, subtask: _, args: _ }) => {
            info!("Running task: {}", task);
            
            let mut config = Config::load()?;
            config.resolve_api_key()?;
            
            if config.api_key.is_empty() {
                anyhow::bail!("No API key found. Please run `rai config` or set RAI_API_KEY environment variable.");
            }

            let provider_impl: Box<dyn Provider> = match config.provider.to_lowercase().as_str() {
                "poe" => Box::new(providers::poe::PoeProvider::new(&config.api_key)),
                _ => anyhow::bail!("Provider '{}' is not yet supported. Only 'poe' is supported in this phase.", config.provider),
            };

            let model = cli.model.clone().unwrap_or(config.default_model);
            info!("Using provider: {}, model: {}", config.provider, model);
            
            println!("Sending request to {}...", config.provider);
            let response = provider_impl.chat(&model, &task).await?;
            println!("\nResponse:\n{}", response);
        },
        Some(Commands::Create { filename }) => {
            info!("Create command selected");
            info!("Filename: {}", filename);
            // TODO: Implement create logic
        }
        Some(Commands::Plan { task_file }) => {
            info!("Plan command selected");
            info!("Task file: {}", task_file);
            // TODO: Implement plan logic
        }
        None => {
            // If no subcommand is provided, maybe we should default to help or something else?
            // For now, let's just print help
            use clap::CommandFactory;
            Cli::command().print_help()?;
        }
    }

    Ok(())
}
