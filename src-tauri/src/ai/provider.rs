//! AI backend selection.

/// Which AI backend powers summaries + live translation. Persisted as the
/// `ai_provider` app setting; resolved from that string via [`Provider::from_setting`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    OpenAi,
    OpenAiCompatible,
    Gemini,
    Anthropic,
}

impl Provider {
    /// Parse the persisted setting string; anything unknown falls back to OpenAI.
    pub fn from_setting(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "gemini" => Provider::Gemini,
            "openai-compatible" => Provider::OpenAiCompatible,
            "anthropic" => Provider::Anthropic,
            _ => Provider::OpenAi,
        }
    }

    /// Keychain service name holding this provider's API key.
    pub fn key_service(self) -> &'static str {
        match self {
            Provider::OpenAi => "openai",
            Provider::OpenAiCompatible => "openai-compatible",
            Provider::Gemini => "gemini",
            Provider::Anthropic => "anthropic",
        }
    }

    /// Human-readable name for error/status messages.
    pub fn label(self) -> &'static str {
        match self {
            Provider::OpenAi => "OpenAI",
            Provider::OpenAiCompatible => "OpenAI Compatible",
            Provider::Gemini => "Gemini",
            Provider::Anthropic => "Anthropic",
        }
    }

    /// Whether the API key is required (built-in providers require one; custom
    /// endpoints may run local models without auth).
    pub fn key_required(self) -> bool {
        matches!(self, Provider::OpenAi | Provider::Gemini)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_from_setting_is_case_insensitive_and_defaults_to_openai() {
        assert_eq!(Provider::from_setting("gemini"), Provider::Gemini);
        assert_eq!(Provider::from_setting("  GEMINI "), Provider::Gemini);
        assert_eq!(Provider::from_setting("openai"), Provider::OpenAi);
        assert_eq!(Provider::from_setting("openai-compatible"), Provider::OpenAiCompatible);
        assert_eq!(Provider::from_setting("  OpenAI-Compatible "), Provider::OpenAiCompatible);
        assert_eq!(Provider::from_setting("anthropic"), Provider::Anthropic);
        assert_eq!(Provider::from_setting("AnThRoPiC"), Provider::Anthropic);
        assert_eq!(Provider::from_setting(""), Provider::OpenAi);
        assert_eq!(Provider::from_setting("something-else"), Provider::OpenAi);
    }

    #[test]
    fn provider_exposes_keychain_service_names() {
        assert_eq!(Provider::OpenAi.key_service(), "openai");
        assert_eq!(Provider::OpenAiCompatible.key_service(), "openai-compatible");
        assert_eq!(Provider::Gemini.key_service(), "gemini");
        assert_eq!(Provider::Anthropic.key_service(), "anthropic");
    }

    #[test]
    fn provider_key_required_is_correct() {
        assert!(Provider::OpenAi.key_required());
        assert!(Provider::Gemini.key_required());
        assert!(!Provider::OpenAiCompatible.key_required());
        assert!(!Provider::Anthropic.key_required());
    }
}
