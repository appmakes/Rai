pub const OPENAI_COMPATIBLE_PROVIDER: &str = "openai-compatible";

pub fn normalize_provider_name(provider: &str) -> Option<String> {
    let normalized = provider.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    let canonical = match normalized.as_str() {
        "claude" => "anthropic",
        "gemini" | "google-gemini" => "google",
        "x.ai" => "xai",
        "aws-bedrock" | "aws_bedrock" | "amazon-bedrock" | "amazon_bedrock" | "aws bedrock" => {
            "bedrock"
        }
        "z.ai" | "z-ai" => "zai",
        "openai_compatible" | "openai-compat" | "openai_compat" | "openai compat" => {
            OPENAI_COMPATIBLE_PROVIDER
        }
        _ => normalized.as_str(),
    };
    Some(canonical.to_string())
}

pub fn available_providers() -> Vec<&'static str> {
    vec![
        "poe",
        "openai",
        "anthropic",
        "google",
        "xai",
        "openrouter",
        "ollama",
        "deepseek",
        "minimax",
        "kimi",
        "zai",
        "bedrock",
        OPENAI_COMPATIBLE_PROVIDER,
    ]
}

pub fn provider_env_vars(provider: &str) -> Vec<&'static str> {
    let normalized = normalize_provider_name(provider).unwrap_or_else(|| provider.to_string());
    match normalized.as_str() {
        "poe" => vec!["POE_API_KEY"],
        "openai" => vec!["OPENAI_API_KEY"],
        "anthropic" => vec!["ANTHROPIC_API_KEY", "CLAUDE_API_KEY"],
        "google" => vec!["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        "xai" => vec!["XAI_API_KEY"],
        "openrouter" => vec!["OPENROUTER_API_KEY"],
        "ollama" => vec!["OLLAMA_API_KEY"],
        "deepseek" => vec!["DEEPSEEK_API_KEY"],
        "minimax" => vec!["MINIMAX_API_KEY"],
        "kimi" => vec!["KIMI_API_KEY", "MOONSHOT_API_KEY"],
        "zai" => vec!["ZAI_API_KEY", "Z_AI_API_KEY", "Z_DOT_AI_API_KEY"],
        "bedrock" => vec!["BEDROCK_API_KEY", "AWS_BEDROCK_API_KEY"],
        OPENAI_COMPATIBLE_PROVIDER => vec!["RAI_OPENAI_COMPAT_API_KEY", "OPENAI_COMPAT_API_KEY"],
        _ => vec![],
    }
}

pub fn generic_provider_env_var(provider: &str) -> Option<String> {
    let normalized = normalize_provider_name(provider)?;
    let upper = normalized
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    Some(format!("{}_API_KEY", upper))
}

pub fn provider_default_base_url(provider: &str) -> Option<&'static str> {
    let normalized = normalize_provider_name(provider).unwrap_or_else(|| provider.to_string());
    match normalized.as_str() {
        "openai" => Some("https://api.openai.com/v1"),
        "xai" => Some("https://api.x.ai/v1"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "ollama" => Some("http://localhost:11434/v1"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "minimax" => Some("https://api.minimax.chat/v1"),
        "kimi" => Some("https://api.moonshot.cn/v1"),
        "zai" => Some("https://api.z.ai/v1"),
        _ => None,
    }
}

pub fn provider_uses_openai_compatible_api(provider: &str) -> bool {
    let normalized = normalize_provider_name(provider).unwrap_or_else(|| provider.to_string());
    matches!(
        normalized.as_str(),
        "openai"
            | "xai"
            | "openrouter"
            | "ollama"
            | "deepseek"
            | "minimax"
            | "kimi"
            | "zai"
            | "bedrock"
            | OPENAI_COMPATIBLE_PROVIDER
    )
}

pub fn provider_supports_base_url(provider: &str) -> bool {
    provider_uses_openai_compatible_api(provider)
}

pub fn provider_requires_api_key(provider: &str) -> bool {
    let normalized = normalize_provider_name(provider).unwrap_or_else(|| provider.to_string());
    !matches!(normalized.as_str(), "ollama")
}

#[cfg(test)]
mod tests {
    use super::{
        generic_provider_env_var, normalize_provider_name, provider_default_base_url,
        provider_requires_api_key, provider_uses_openai_compatible_api, OPENAI_COMPATIBLE_PROVIDER,
    };

    #[test]
    fn normalizes_provider_aliases() {
        assert_eq!(normalize_provider_name("z.ai").as_deref(), Some("zai"));
        assert_eq!(normalize_provider_name("gemini").as_deref(), Some("google"));
        assert_eq!(
            normalize_provider_name("openai_compatible").as_deref(),
            Some(OPENAI_COMPATIBLE_PROVIDER)
        );
    }

    #[test]
    fn computes_generic_env_var_with_safe_characters() {
        assert_eq!(
            generic_provider_env_var(OPENAI_COMPATIBLE_PROVIDER).as_deref(),
            Some("OPENAI_COMPATIBLE_API_KEY")
        );
    }

    #[test]
    fn reports_default_base_urls_for_openai_like_providers() {
        assert_eq!(
            provider_default_base_url("openai").as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(
            provider_default_base_url("ollama").as_deref(),
            Some("http://localhost:11434/v1")
        );
    }

    #[test]
    fn marks_openai_compatible_set_correctly() {
        assert!(provider_uses_openai_compatible_api("openrouter"));
        assert!(provider_uses_openai_compatible_api(
            OPENAI_COMPATIBLE_PROVIDER
        ));
        assert!(!provider_uses_openai_compatible_api("anthropic"));
    }

    #[test]
    fn marks_ollama_as_key_optional() {
        assert!(provider_requires_api_key("openai"));
        assert!(!provider_requires_api_key("ollama"));
    }
}
