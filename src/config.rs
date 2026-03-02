use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_PROFILE_NAME: &str = "default";
const DEFAULT_MODEL_NAME: &str = "gpt-4o";
const DEFAULT_TOOL_MODE: &str = "ask";

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

fn validate_profile_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("Profile name cannot be empty.");
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        bail!("Profile name cannot contain path separators.");
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        bail!(
            "Invalid profile name '{}'. Use only letters, numbers, '-', '_' or '.'.",
            trimmed
        );
    }
    Ok(trimmed.to_string())
}

fn profile_name_from_file(file_name: &str) -> Option<String> {
    let prefix = "config.";
    let suffix = ".toml";
    if file_name == "config.toml" {
        return None;
    }
    if file_name.starts_with(prefix) && file_name.ends_with(suffix) {
        let inner = &file_name[prefix.len()..file_name.len() - suffix.len()];
        if !inner.is_empty() {
            return Some(inner.to_string());
        }
    }
    None
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GlobalConfigFile {
    #[serde(default = "default_profile_name")]
    default_profile: String,
    #[serde(default)]
    active_profile: Option<String>,
}

impl Default for GlobalConfigFile {
    fn default() -> Self {
        Self {
            default_profile: default_profile_name(),
            active_profile: Some(default_profile_name()),
        }
    }
}

fn default_profile_name() -> String {
    DEFAULT_PROFILE_NAME.to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct ProfileConfigFile {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    providers: Vec<String>,
    #[serde(default)]
    default_provider: Option<String>,
    #[serde(default)]
    default_model: String,
    #[serde(default)]
    tool_mode: String,
    #[serde(default)]
    no_tools: bool,
    #[serde(default)]
    auto_approve: bool,
}

impl ProfileConfigFile {
    fn has_meaningful_fields(&self) -> bool {
        !self.provider.trim().is_empty()
            || !self.providers.is_empty()
            || self.default_provider.is_some()
            || !self.default_model.trim().is_empty()
            || !self.tool_mode.trim().is_empty()
            || self.no_tools
            || self.auto_approve
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct CombinedConfigFile {
    #[serde(default = "default_profile_name")]
    default_profile: String,
    #[serde(default)]
    active_profile: Option<String>,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    providers: Vec<String>,
    #[serde(default)]
    default_provider: Option<String>,
    #[serde(default)]
    default_model: String,
    #[serde(default)]
    tool_mode: String,
    #[serde(default)]
    no_tools: bool,
    #[serde(default)]
    auto_approve: bool,
}

impl CombinedConfigFile {
    fn from_parts(global: &GlobalConfigFile, profile: &ProfileConfigFile) -> Self {
        Self {
            default_profile: global.default_profile.clone(),
            active_profile: global.active_profile.clone(),
            provider: profile.provider.clone(),
            providers: profile.providers.clone(),
            default_provider: profile.default_provider.clone(),
            default_model: profile.default_model.clone(),
            tool_mode: profile.tool_mode.clone(),
            no_tools: profile.no_tools,
            auto_approve: profile.auto_approve,
        }
    }

    fn to_global(&self) -> GlobalConfigFile {
        GlobalConfigFile {
            default_profile: self.default_profile.clone(),
            active_profile: self.active_profile.clone(),
        }
    }

    fn to_profile(&self) -> ProfileConfigFile {
        ProfileConfigFile {
            provider: self.provider.clone(),
            providers: self.providers.clone(),
            default_provider: self.default_provider.clone(),
            default_model: self.default_model.clone(),
            tool_mode: self.tool_mode.clone(),
            no_tools: self.no_tools,
            auto_approve: self.auto_approve,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct LegacyConfigFile {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    providers: Vec<String>,
    #[serde(default)]
    default_provider: Option<String>,
    #[serde(default)]
    default_model: String,
    #[serde(default)]
    tool_mode: String,
    #[serde(default)]
    no_tools: bool,
    #[serde(default)]
    auto_approve: bool,
}

impl LegacyConfigFile {
    fn has_meaningful_fields(&self) -> bool {
        !self.provider.trim().is_empty()
            || !self.providers.is_empty()
            || self.default_provider.is_some()
            || !self.default_model.trim().is_empty()
            || !self.tool_mode.trim().is_empty()
            || self.no_tools
            || self.auto_approve
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub profile: String,
    pub provider: String,
    pub providers: Vec<String>,
    pub default_provider: Option<String>,
    pub default_model: String,
    pub tool_mode: String,
    pub no_tools: bool,
    pub auto_approve: bool,
    pub api_key: String, // Not serialized, populated at runtime
}

impl Config {
    pub fn load(profile_override: Option<&str>) -> Result<Self> {
        let config_dir = Self::get_config_dir()?;
        fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

        let mut global = Self::load_global_config(&config_dir)?;
        Self::migrate_legacy_if_needed(&config_dir, &mut global)?;

        let (mut requested_profile, explicit_profile_selection) = if let Some(profile) = profile_override
        {
            (validate_profile_name(profile)?, true)
        } else if let Ok(profile) = env::var("RAI_PROFILE") {
            if profile.trim().is_empty() {
                (Self::resolve_profile_from_global(&global)?, false)
            } else {
                (validate_profile_name(&profile)?, true)
            }
        } else {
            (Self::resolve_profile_from_global(&global)?, false)
        };

        if !explicit_profile_selection && !Self::profile_exists(&requested_profile)? {
            requested_profile = DEFAULT_PROFILE_NAME.to_string();
            Self::ensure_default_profile_exists(&config_dir, &mut global)?;
            if global.active_profile.as_deref() != Some(DEFAULT_PROFILE_NAME) {
                global.active_profile = Some(DEFAULT_PROFILE_NAME.to_string());
                Self::save_global_config(&config_dir, &global)?;
            }
        }

        let profile_file = Self::load_profile_config(&config_dir, &requested_profile)?;
        Ok(Self::from_profile_file(requested_profile, profile_file))
    }

    pub fn save(&self) -> Result<()> {
        let config_dir = Self::get_config_dir()?;
        fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

        let mut global = Self::load_global_config(&config_dir)?;
        if global.default_profile.trim().is_empty() {
            global.default_profile = DEFAULT_PROFILE_NAME.to_string();
        }
        if global.active_profile.is_none() {
            global.active_profile = Some(self.profile.clone());
        }
        Self::save_global_config(&config_dir, &global)?;
        Self::save_profile_config(&config_dir, &self.profile, &self.to_profile_file())?;
        Ok(())
    }

    pub fn list_profiles() -> Result<Vec<String>> {
        let config_dir = Self::get_config_dir()?;
        if !config_dir.exists() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        for entry in fs::read_dir(&config_dir).context("Failed to read config directory")? {
            let entry = entry?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if let Some(name) = profile_name_from_file(&file_name) {
                names.push(name);
            }
        }
        if Self::profile_exists(DEFAULT_PROFILE_NAME)?
            && !names.iter().any(|name| name == DEFAULT_PROFILE_NAME)
        {
            names.push(DEFAULT_PROFILE_NAME.to_string());
        }
        names.sort();
        Ok(names)
    }

    pub fn profile_exists(profile: &str) -> Result<bool> {
        let profile = validate_profile_name(profile)?;
        let config_dir = Self::get_config_dir()?;
        let path = Self::profile_config_path(&config_dir, &profile);
        if !path.exists() {
            return Ok(false);
        }
        if profile == DEFAULT_PROFILE_NAME {
            let content = fs::read_to_string(path).context("Failed to read profile config file")?;
            let combined: CombinedConfigFile =
                toml::from_str(&content).context("Failed to parse profile config file")?;
            return Ok(combined.to_profile().has_meaningful_fields());
        }
        Ok(true)
    }

    pub fn create_profile(name: &str, copy_from: Option<&str>) -> Result<()> {
        let profile_name = validate_profile_name(name)?;
        let config_dir = Self::get_config_dir()?;
        fs::create_dir_all(&config_dir).context("Failed to create config directory")?;
        if Self::profile_exists(&profile_name)? {
            bail!("Profile '{}' already exists.", profile_name);
        }

        let source_profile = if let Some(source) = copy_from {
            let source_name = validate_profile_name(source)?;
            Self::load_profile_config(&config_dir, &source_name)?
        } else {
            Self::default_profile_config()
        };
        Self::save_profile_config(&config_dir, &profile_name, &source_profile)?;

        let mut global = Self::load_global_config(&config_dir)?;
        if global.default_profile.trim().is_empty() {
            global.default_profile = profile_name.clone();
        }
        if global.active_profile.is_none() {
            global.active_profile = Some(profile_name);
        }
        Self::save_global_config(&config_dir, &global)?;
        Ok(())
    }

    pub fn delete_profile(name: &str) -> Result<()> {
        let profile_name = validate_profile_name(name)?;
        let config_dir = Self::get_config_dir()?;
        let path = Self::profile_config_path(&config_dir, &profile_name);
        if !path.exists() {
            bail!("Profile '{}' not found.", profile_name);
        }

        let global = Self::load_global_config(&config_dir)?;
        if global.default_profile == profile_name {
            bail!(
                "Cannot delete default profile '{}'. Set another default profile first.",
                profile_name
            );
        }
        if global.active_profile.as_deref() == Some(profile_name.as_str()) {
            bail!(
                "Cannot delete active profile '{}'. Switch active profile first.",
                profile_name
            );
        }

        fs::remove_file(path).context("Failed to delete profile file")?;
        Ok(())
    }

    pub fn rename_profile(old_name: &str, new_name: &str) -> Result<()> {
        let old_name = validate_profile_name(old_name)?;
        let new_name = validate_profile_name(new_name)?;
        let config_dir = Self::get_config_dir()?;

        let old_path = Self::profile_config_path(&config_dir, &old_name);
        let new_path = Self::profile_config_path(&config_dir, &new_name);
        if !old_path.exists() {
            bail!("Profile '{}' not found.", old_name);
        }
        if new_path.exists() {
            bail!("Profile '{}' already exists.", new_name);
        }

        fs::rename(old_path, new_path).context("Failed to rename profile file")?;

        let mut global = Self::load_global_config(&config_dir)?;
        if global.default_profile == old_name {
            global.default_profile = new_name.clone();
        }
        if global.active_profile.as_deref() == Some(old_name.as_str()) {
            global.active_profile = Some(new_name.clone());
        }
        Self::save_global_config(&config_dir, &global)?;
        Ok(())
    }

    pub fn set_active_profile(name: &str) -> Result<()> {
        let profile_name = validate_profile_name(name)?;
        let config_dir = Self::get_config_dir()?;
        if !Self::profile_config_path(&config_dir, &profile_name).exists() {
            bail!("Profile '{}' not found.", profile_name);
        }
        let mut global = Self::load_global_config(&config_dir)?;
        global.active_profile = Some(profile_name);
        Self::save_global_config(&config_dir, &global)
    }

    pub fn set_default_profile(name: &str) -> Result<()> {
        let profile_name = validate_profile_name(name)?;
        let config_dir = Self::get_config_dir()?;
        if !Self::profile_config_path(&config_dir, &profile_name).exists() {
            bail!("Profile '{}' not found.", profile_name);
        }
        let mut global = Self::load_global_config(&config_dir)?;
        global.default_profile = profile_name;
        Self::save_global_config(&config_dir, &global)
    }

    pub fn read_global_profile_settings() -> Result<(String, Option<String>)> {
        let config_dir = Self::get_config_dir()?;
        let global = Self::load_global_config(&config_dir)?;
        Ok((global.default_profile, global.active_profile))
    }

    fn from_profile_file(profile: String, mut file: ProfileConfigFile) -> Self {
        file.providers = normalize_provider_list(&file.providers);
        file.default_provider = file
            .default_provider
            .as_deref()
            .and_then(normalize_provider_name)
            .filter(|default_provider| file.providers.contains(default_provider));

        let provider = resolve_active_provider(
            &file.provider,
            &file.providers,
            file.default_provider.as_deref(),
        )
        .unwrap_or_default();

        if file.providers.is_empty() && !provider.is_empty() {
            file.providers.push(provider.clone());
        }
        if file.default_provider.is_none() {
            file.default_provider = file.providers.first().cloned();
        }
        if file.default_model.trim().is_empty() {
            file.default_model = DEFAULT_MODEL_NAME.to_string();
        }
        if file.tool_mode.trim().is_empty() {
            file.tool_mode = DEFAULT_TOOL_MODE.to_string();
        }

        Self {
            profile,
            provider,
            providers: file.providers,
            default_provider: file.default_provider,
            default_model: file.default_model,
            tool_mode: file.tool_mode,
            no_tools: file.no_tools,
            auto_approve: file.auto_approve,
            api_key: String::new(),
        }
    }

    fn to_profile_file(&self) -> ProfileConfigFile {
        ProfileConfigFile {
            provider: self.provider.clone(),
            providers: self.providers.clone(),
            default_provider: self.default_provider.clone(),
            default_model: self.default_model.clone(),
            tool_mode: self.tool_mode.clone(),
            no_tools: self.no_tools,
            auto_approve: self.auto_approve,
        }
    }

    fn resolve_profile_from_global(global: &GlobalConfigFile) -> Result<String> {
        if let Some(active) = global.active_profile.as_deref() {
            return validate_profile_name(active);
        }
        validate_profile_name(&global.default_profile)
    }

    fn default_profile_config() -> ProfileConfigFile {
        ProfileConfigFile {
            providers: vec!["poe".to_string()],
            default_provider: Some("poe".to_string()),
            default_model: DEFAULT_MODEL_NAME.to_string(),
            tool_mode: DEFAULT_TOOL_MODE.to_string(),
            no_tools: false,
            auto_approve: false,
            provider: "poe".to_string(),
        }
    }

    fn ensure_default_profile_exists(config_dir: &Path, global: &mut GlobalConfigFile) -> Result<()> {
        let default_profile_path = Self::profile_config_path(config_dir, DEFAULT_PROFILE_NAME);
        if !default_profile_path.exists() || !Self::profile_exists(DEFAULT_PROFILE_NAME)? {
            let data = Self::default_profile_config();
            Self::save_profile_config(config_dir, DEFAULT_PROFILE_NAME, &data)?;
        }

        let mut changed = false;
        if global.default_profile.trim().is_empty() {
            global.default_profile = DEFAULT_PROFILE_NAME.to_string();
            changed = true;
        }
        if global.active_profile.is_none() {
            global.active_profile = Some(DEFAULT_PROFILE_NAME.to_string());
            changed = true;
        }
        if changed {
            Self::save_global_config(config_dir, global)?;
        }

        Ok(())
    }

    fn migrate_legacy_if_needed(config_dir: &Path, global: &mut GlobalConfigFile) -> Result<()> {
        let default_profile_path = Self::profile_config_path(config_dir, DEFAULT_PROFILE_NAME);
        if default_profile_path.exists() {
            if global.default_profile.trim().is_empty() {
                global.default_profile = DEFAULT_PROFILE_NAME.to_string();
                Self::save_global_config(config_dir, global)?;
            }
            return Ok(());
        }

        let global_path = Self::global_config_path(config_dir);
        if !global_path.exists() {
            return Ok(());
        }

        let content =
            fs::read_to_string(&global_path).context("Failed to read potential legacy config")?;
        let legacy: LegacyConfigFile = toml::from_str(&content).unwrap_or_default();
        if !legacy.has_meaningful_fields() {
            return Ok(());
        }

        let profile_file = ProfileConfigFile {
            provider: legacy.provider,
            providers: legacy.providers,
            default_provider: legacy.default_provider,
            default_model: legacy.default_model,
            tool_mode: legacy.tool_mode,
            no_tools: legacy.no_tools,
            auto_approve: legacy.auto_approve,
        };

        Self::save_profile_config(config_dir, DEFAULT_PROFILE_NAME, &profile_file)?;
        global.default_profile = DEFAULT_PROFILE_NAME.to_string();
        if global.active_profile.is_none() {
            global.active_profile = Some(DEFAULT_PROFILE_NAME.to_string());
        }
        Self::save_global_config(config_dir, global)?;
        Ok(())
    }

    fn load_global_config(config_dir: &Path) -> Result<GlobalConfigFile> {
        let path = Self::global_config_path(config_dir);
        if !path.exists() {
            return Ok(GlobalConfigFile::default());
        }
        let content = fs::read_to_string(path).context("Failed to read global config file")?;
        let combined: CombinedConfigFile =
            toml::from_str(&content).context("Failed to parse global config file")?;
        Ok(combined.to_global())
    }

    fn save_global_config(config_dir: &Path, global: &GlobalConfigFile) -> Result<()> {
        let path = Self::global_config_path(config_dir);
        let existing_profile = if path.exists() {
            let content = fs::read_to_string(&path).context("Failed to read global config file")?;
            toml::from_str::<CombinedConfigFile>(&content)
                .context("Failed to parse global config file")?
                .to_profile()
        } else {
            ProfileConfigFile::default()
        };
        let combined = CombinedConfigFile::from_parts(global, &existing_profile);
        let content =
            toml::to_string_pretty(&combined).context("Failed to serialize global config file")?;
        fs::write(path, content).context("Failed to write global config file")
    }

    fn load_profile_config(config_dir: &Path, profile: &str) -> Result<ProfileConfigFile> {
        let profile = validate_profile_name(profile)?;
        let path = Self::profile_config_path(config_dir, &profile);
        if !path.exists() {
            bail!(
                "Profile '{}' not found. Run `rai start` or `rai profile create {}`.",
                profile,
                profile
            );
        }
        let content = fs::read_to_string(path).context("Failed to read profile config file")?;
        if profile == DEFAULT_PROFILE_NAME {
            let combined: CombinedConfigFile =
                toml::from_str(&content).context("Failed to parse profile config file")?;
            let profile_file = combined.to_profile();
            if profile_file.has_meaningful_fields() {
                return Ok(profile_file);
            }
            let defaults = Self::default_profile_config();
            Self::save_profile_config(config_dir, DEFAULT_PROFILE_NAME, &defaults)?;
            return Ok(defaults);
        }
        let profile_file: ProfileConfigFile =
            toml::from_str(&content).context("Failed to parse profile config file")?;
        Ok(profile_file)
    }

    fn save_profile_config(
        config_dir: &Path,
        profile: &str,
        data: &ProfileConfigFile,
    ) -> Result<()> {
        let profile = validate_profile_name(profile)?;
        let path = Self::profile_config_path(config_dir, &profile);
        if profile == DEFAULT_PROFILE_NAME {
            let global = if path.exists() {
                let content =
                    fs::read_to_string(&path).context("Failed to read global config file")?;
                toml::from_str::<CombinedConfigFile>(&content)
                    .context("Failed to parse global config file")?
                    .to_global()
            } else {
                GlobalConfigFile::default()
            };
            let combined = CombinedConfigFile::from_parts(&global, data);
            let content = toml::to_string_pretty(&combined)
                .context("Failed to serialize profile config file")?;
            return fs::write(path, content).context("Failed to write profile config file");
        }
        let content =
            toml::to_string_pretty(data).context("Failed to serialize profile config file")?;
        fs::write(path, content).context("Failed to write profile config file")
    }

    fn global_config_path(config_dir: &Path) -> PathBuf {
        config_dir.join("config.toml")
    }

    fn profile_config_path(config_dir: &Path, profile: &str) -> PathBuf {
        if profile == DEFAULT_PROFILE_NAME {
            return Self::global_config_path(config_dir);
        }
        config_dir.join(format!("config.{}.toml", profile))
    }

    fn get_config_dir() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "rai", "rai")
            .context("Failed to determine config directory")?;
        Ok(proj_dirs.config_dir().to_path_buf())
    }

    // Helper to resolve API key from Keyring -> Provider Env
    pub fn resolve_api_key(&mut self) -> Result<()> {
        if self.provider.trim().is_empty() {
            return Ok(());
        }

        // 1. Check Keyring first (profile-scoped then legacy provider scope).
        #[cfg(not(test))]
        {
            let profile_scoped = format!("{}:{}", self.profile, self.provider);
            if let Ok(key) = crate::get_api_key_helper(&profile_scoped) {
                self.api_key = key;
                return Ok(());
            }

            if let Ok(key) = crate::get_api_key_helper(&self.provider) {
                self.api_key = key;
                return Ok(());
            }
        }

        // 2. Check provider-specific env vars (automation fallback).
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{profile_name_from_file, resolve_active_provider, validate_profile_name};

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

    #[test]
    fn validates_profile_names() {
        assert!(validate_profile_name("default").is_ok());
        assert!(validate_profile_name("hard-task_1").is_ok());
        assert!(validate_profile_name("bad/name").is_err());
        assert!(validate_profile_name("with space").is_err());
    }

    #[test]
    fn parses_profile_name_from_file() {
        assert_eq!(
            profile_name_from_file("config.default.toml").as_deref(),
            Some("default")
        );
        assert_eq!(profile_name_from_file("config.toml"), None);
        assert_eq!(profile_name_from_file("other.toml"), None);
    }
}
