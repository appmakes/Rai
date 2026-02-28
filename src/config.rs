use serde::{Deserialize, Serialize};
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result};
use std::env;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub provider: String,
    pub default_model: String,
    #[serde(skip)]
    pub api_key: String, // Not serialized, populated at runtime
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;
        let mut config = if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .context("Failed to read config file")?;
            toml::from_str(&content)
                .context("Failed to parse config file")?
        } else {
            Config::default()
        };
        
        // Ensure defaults if empty
        if config.provider.is_empty() {
             config.provider = "poe".to_string();
        }
        if config.default_model.is_empty() {
             config.default_model = "gpt-4o".to_string();
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::get_config_path()?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create config directory")?;
        }
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        fs::write(&config_path, content)
            .context("Failed to write config file")?;
        Ok(())
    }

    fn get_config_path() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "rai", "rai")
            .context("Failed to determine config directory")?;
        Ok(proj_dirs.config_dir().join("config.toml"))
    }
    
    // Helper to resolve API key from Env -> Keyring
    pub fn resolve_api_key(&mut self) -> Result<()> {
        // 1. Check RAI_API_KEY (Global override)
        if let Ok(key) = env::var("RAI_API_KEY") {
            self.api_key = key;
            return Ok(());
        }
        
        // 2. Check Provider Specific Standard Env Vars
        // This allows rai to piggyback on existing setups (e.g. claude cli, gemini cli)
        let provider_key = self.provider.to_lowercase();
        let env_candidates = match provider_key.as_str() {
            "openai" => vec!["OPENAI_API_KEY"],
            "anthropic" | "claude" => vec!["ANTHROPIC_API_KEY"],
            "gemini" | "google" => vec!["GEMINI_API_KEY", "GOOGLE_API_KEY"],
            "poe" => vec!["POE_API_KEY"],
            _ => vec![],
        };

        for env_var in env_candidates {
            if let Ok(key) = env::var(env_var) {
                if !key.is_empty() {
                    self.api_key = key;
                    return Ok(());
                }
            }
        }
        
        // 3. Check Generic Provider Env Var (fallback logic)
        let provider_env = format!("{}_API_KEY", self.provider.to_uppercase());
        if let Ok(key) = env::var(&provider_env) {
             if !key.is_empty() {
                self.api_key = key;
                return Ok(());
             }
        }

        // 4. Check Keyring (if not in env)
        #[cfg(not(test))] // Avoid keyring in tests/sandbox if needed, but for now we try
        match crate::get_api_key_helper(&self.provider) {
            Ok(key) => {
                self.api_key = key;
                return Ok(());
            },
            Err(_) => {
                // Ignore keyring error (key not found) and leave empty
            }
        }
        
        Ok(())
    }
}
