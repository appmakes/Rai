use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

fn normalize_provider_name(provider: &str) -> Option<String> {
    let normalized = provider.trim().to_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_provider_list(providers: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for provider in providers {
        if let Some(name) = normalize_provider_name(provider) {
            if !normalized.contains(&name) {
                normalized.push(name);
            }
        }
    }
    normalized
}

fn resolve_active_provider(
    legacy_provider: &str,
    configured_providers: &[String],
    default_provider: Option<&str>,
) -> Option<String> {
    let legacy_provider = normalize_provider_name(legacy_provider);
    let providers = normalize_provider_list(configured_providers);

    if providers.is_empty() {
        return legacy_provider;
    }

    if providers.len() == 1 {
        return Some(providers[0].clone());
    }

    if let Some(default_provider) = default_provider.and_then(normalize_provider_name) {
        if providers
            .iter()
            .any(|provider| provider == &default_provider)
        {
            return Some(default_provider);
        }
    }

    if let Some(legacy_provider) = legacy_provider {
        if providers
            .iter()
            .any(|provider| provider == &legacy_provider)
        {
            return Some(legacy_provider);
        }
    }

    providers.first().cloned()
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub default_model: String,
    #[serde(skip)]
    pub api_key: String, // Not serialized, populated at runtime
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;
        let mut config = if config_path.exists() {
            let content = fs::read_to_string(&config_path).context("Failed to read config file")?;
            toml::from_str(&content).context("Failed to parse config file")?
        } else {
            Config::default()
        };

        config.providers = normalize_provider_list(&config.providers);
        config.default_provider = config
            .default_provider
            .as_deref()
            .and_then(normalize_provider_name)
            .filter(|default_provider| config.providers.contains(default_provider));

        // Ensure defaults if empty
        config.provider = resolve_active_provider(
            &config.provider,
            &config.providers,
            config.default_provider.as_deref(),
        )
        .unwrap_or_default();
        if config.providers.is_empty() && !config.provider.is_empty() {
            config.providers.push(config.provider.clone());
        }
        if config.default_provider.is_none() {
            config.default_provider = config.providers.first().cloned();
        }
        if config.default_model.is_empty() {
            config.default_model = "gpt-4o".to_string();
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::get_config_path()?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&config_path, content).context("Failed to write config file")?;
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

        if self.provider.trim().is_empty() {
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
            }
            Err(_) => {
                // Ignore keyring error (key not found) and leave empty
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_active_provider;

    #[test]
    fn resolves_single_configured_provider() {
        let active = resolve_active_provider("", &["poe".to_string()], None);
        assert_eq!(active.as_deref(), Some("poe"));
    }

    #[test]
    fn resolves_default_provider_when_multiple_exist() {
        let active =
            resolve_active_provider("", &["openai".to_string(), "poe".to_string()], Some("poe"));
        assert_eq!(active.as_deref(), Some("poe"));
    }

    #[test]
    fn falls_back_to_first_provider_when_default_is_missing() {
        let active = resolve_active_provider(
            "",
            &["poe".to_string(), "openai".to_string()],
            Some("anthropic"),
        );
        assert_eq!(active.as_deref(), Some("poe"));
    }

    #[test]
    fn uses_legacy_provider_when_no_provider_list_exists() {
        let active = resolve_active_provider("openai", &[], None);
        assert_eq!(active.as_deref(), Some("openai"));
    }
}
