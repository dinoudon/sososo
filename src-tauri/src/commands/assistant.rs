//! AI commands (Milestone E + live translate + transcript chat) over the active
//! provider (OpenAI or Gemini). For every command the DB mutex is held only for
//! the synchronous read/write steps — never across the network `await` — so the
//! command futures stay `Send`.

use tauri::State;

use crate::ai::Provider;
use crate::db::{ChatMessage, Db};
use crate::error::{AppError, AppResult};
use crate::{ai, keys};

/// `app_settings` key for the persisted AI-summary output-language preference.
const SUMMARY_LANGUAGE_KEY: &str = "summary_language";

/// `app_settings` key for the active AI provider ("openai" | "openai-compatible" | "gemini" | "anthropic").
const AI_PROVIDER_KEY: &str = "ai_provider";

/// Defaults for AI settings keys. Not stored — the absence of a setting implies the default.
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
const DEFAULT_OPENAI_COMPATIBLE_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OPENAI_COMPATIBLE_MODEL: &str = "gpt-4o-mini";
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-20250514";

/// How many of the most recent chat turns to send to the model per request. The
/// full transcript is always sent as context, so older turns are dropped first to
/// keep the prompt bounded.
const CHAT_HISTORY_LIMIT: usize = 20;

fn read_setting(db: &Db, key: &str, default: &str) -> String {
    db.get_setting(key)
        .ok()
        .flatten()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn load_ai_settings_inner(db: &Db, provider: Provider, api_key: Option<String>) -> ai::AiSettings {
    let s = |key, default| read_setting(db, key, default);

    let (base_url, model) = match provider {
        Provider::OpenAi => (
            s("openai_base_url", DEFAULT_OPENAI_BASE_URL),
            s("openai_model", DEFAULT_OPENAI_MODEL),
        ),
        Provider::OpenAiCompatible => (
            s(
                "openai_compatible_base_url",
                DEFAULT_OPENAI_COMPATIBLE_BASE_URL,
            ),
            s("openai_compatible_model", DEFAULT_OPENAI_COMPATIBLE_MODEL),
        ),
        Provider::Gemini => (String::new(), s("gemini_model", DEFAULT_GEMINI_MODEL)),
        Provider::Anthropic => (
            s("anthropic_base_url", DEFAULT_ANTHROPIC_BASE_URL),
            s("anthropic_model", DEFAULT_ANTHROPIC_MODEL),
        ),
    };

    ai::AiSettings {
        provider,
        api_key,
        base_url,
        model,
    }
}

/// Read the persisted AI provider ("openai" | "gemini"). Defaults to "openai"
/// when never set. Used by Settings to populate the provider dropdown.
#[tauri::command]
pub fn get_ai_provider(db: State<'_, Db>) -> AppResult<String> {
    Ok(db
        .get_setting(AI_PROVIDER_KEY)?
        .unwrap_or_else(|| "openai".to_string()))
}

/// Persist the active AI provider. "openai", "openai-compatible", "gemini",
/// and "anthropic" are accepted.
#[tauri::command]
pub fn set_ai_provider(db: State<'_, Db>, provider: String) -> AppResult<()> {
    let normalized = match provider.trim().to_ascii_lowercase().as_str() {
        "gemini" => "gemini",
        "openai" => "openai",
        "openai-compatible" => "openai-compatible",
        "anthropic" => "anthropic",
        other => return Err(AppError::Config(format!("unknown AI provider: {other}"))),
    };
    db.set_setting(AI_PROVIDER_KEY, normalized)
}

/// Resolve the active AI provider, its API key (from the keychain), and all
/// configurable settings (model name, base URL). Reads the persisted settings
/// synchronously — the returned values are owned, so callers never hold the DB
/// lock across a network `await`.
///
/// For built-in providers (OpenAI, Gemini) the API key is required. For custom
/// endpoints (OpenAI Compatible, Anthropic) it is optional — local models may not
/// need authentication.
fn resolve_ai_provider(db: &Db) -> AppResult<ai::AiSettings> {
    let setting = db
        .get_setting(AI_PROVIDER_KEY)?
        .unwrap_or_else(|| "openai".to_string());
    let provider = Provider::from_setting(&setting);
    let api_key = keys::get_api_key(provider.key_service())?;
    if provider.key_required() && api_key.is_none() {
        return Err(AppError::Config(format!(
            "{} API key is not set (open Settings)",
            provider.label()
        )));
    }
    Ok(load_ai_settings_inner(db, provider, api_key))
}

/// Read the persisted AI-summary output language (a Deepgram language code or the
/// literal `"auto"`). Defaults to `"auto"` when never set. Used by Settings to
/// populate the dropdown.
#[tauri::command]
pub fn get_summary_language(db: State<'_, Db>) -> AppResult<String> {
    Ok(db
        .get_setting(SUMMARY_LANGUAGE_KEY)?
        .unwrap_or_else(|| "auto".to_string()))
}

/// Persist the AI-summary output language (a language code or `"auto"`).
#[tauri::command]
pub fn set_summary_language(db: State<'_, Db>, language: String) -> AppResult<()> {
    db.set_setting(SUMMARY_LANGUAGE_KEY, &language)
}

/// Generate (and persist) an AI summary for a recorded session via the active AI
/// provider (OpenAI or Gemini), then return the Markdown summary. Requires that
/// provider's API key to be set in Settings.
///
/// `summary_language` is the desired output language: the literal `"auto"` (match
/// the transcript) or a human-readable language name (e.g. `"Indonesian"`). The
/// frontend resolves the persisted language code to this value.
#[tauri::command]
pub async fn summarize_session(
    db: State<'_, Db>,
    id: i64,
    summary_language: String,
) -> AppResult<String> {
    let detail = db
        .get_session(id)?
        .ok_or_else(|| AppError::Session("session not found".into()))?;
    if detail.segments.is_empty() {
        return Err(AppError::Session("no transcript to summarize yet".into()));
    }

    let settings = resolve_ai_provider(&db)?;

    let (summary, model) = ai::summarize(
        &settings,
        &detail.session.title,
        &detail.session.language,
        &summary_language,
        &detail.segments,
    )
    .await?;

    let at = chrono::Utc::now().to_rfc3339();
    db.save_summary(id, &summary, &model, &at)?;
    Ok(summary)
}

/// Translate one finalized transcript line via the active AI provider (OpenAI or
/// Gemini) into `target_lang` (a human-readable language name like "English") and
/// persist it on the segment row, returning the translated text.
///
/// Idempotent: if the row already has a translation for the same language it is
/// returned without calling the provider, so a line is never translated twice. The
/// frontend additionally caches per segment, so this is the defensive backstop.
/// Requires the active provider's key (Settings).
#[tauri::command]
pub async fn translate_segment(
    db: State<'_, Db>,
    session_id: i64,
    segment_id: String,
    text: String,
    target_lang: String,
) -> AppResult<String> {
    if let Some((existing, lang)) = db.get_translation(session_id, &segment_id)? {
        if lang == target_lang {
            return Ok(existing);
        }
    }

    let settings = resolve_ai_provider(&db)?;

    let translated = ai::translate(&settings, &text, &target_lang).await?;
    db.save_translation(session_id, &segment_id, &translated, &target_lang)?;
    Ok(translated)
}

/// All stored chat turns for a session (oldest first), for rendering the panel.
#[tauri::command]
pub fn get_chat_messages(db: State<'_, Db>, session_id: i64) -> AppResult<Vec<ChatMessage>> {
    db.get_chat_messages(session_id)
}

/// Delete a session's entire chat history.
#[tauri::command]
pub fn clear_chat(db: State<'_, Db>, session_id: i64) -> AppResult<()> {
    db.clear_chat_messages(session_id)
}

/// Ask a question about a session's transcript via the active AI provider (OpenAI
/// or Gemini) and persist the exchange. Returns the two newly stored turns
/// `[user, assistant]`. Requires the active provider's API key (Settings) and a
/// non-empty transcript. Rows are written only after the AI call succeeds, so a
/// failed turn leaves no orphan question in the history.
#[tauri::command]
pub async fn chat_session(
    db: State<'_, Db>,
    id: i64,
    message: String,
) -> AppResult<Vec<ChatMessage>> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(AppError::Ai("message is empty".into()));
    }

    let detail = db
        .get_session(id)?
        .ok_or_else(|| AppError::Session("session not found".into()))?;
    if detail.segments.is_empty() {
        return Err(AppError::Session("no transcript to chat about yet".into()));
    }

    let settings = resolve_ai_provider(&db)?;

    // Send only the most recent turns (older ones dropped first) to bound tokens.
    let stored = db.get_chat_messages(id)?;
    let start = stored.len().saturating_sub(CHAT_HISTORY_LIMIT);
    let history: Vec<ai::ChatTurn> = stored[start..]
        .iter()
        .map(|m| ai::ChatTurn {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();

    let (reply, model) = ai::chat_about_transcript(
        &settings,
        &detail.session.title,
        &detail.segments,
        &history,
        trimmed,
    )
    .await?;

    let at = chrono::Utc::now().to_rfc3339();
    let user_msg = db.add_chat_message(id, "user", trimmed, None, &at)?;
    let assistant_msg = db.add_chat_message(id, "assistant", &reply, Some(&model), &at)?;
    Ok(vec![user_msg, assistant_msg])
}

// ── AI model / endpoint settings commands ──────────────────────────────

fn validate_base_url(url: &str, label: &str) -> AppResult<()> {
    if url.is_empty() {
        return Ok(());
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    if lower.starts_with("http://localhost") || lower.starts_with("http://127.0.0.1") {
        return Ok(());
    }
    Err(AppError::Config(format!(
        "{label} base URL must use https:// or be a local http://localhost address"
    )))
}

#[tauri::command]
pub fn set_openai_model(db: State<'_, Db>, model: String) -> AppResult<()> {
    let model = model.trim();
    if model.is_empty() {
        return Err(AppError::Config("model name is required".into()));
    }
    db.set_setting("openai_model", model)
}

#[tauri::command]
pub fn set_gemini_model(db: State<'_, Db>, model: String) -> AppResult<()> {
    let model = model.trim();
    if model.is_empty() {
        return Err(AppError::Config("model name is required".into()));
    }
    db.set_setting("gemini_model", model)
}

#[tauri::command]
pub fn set_openai_compatible_base_url(db: State<'_, Db>, url: String) -> AppResult<()> {
    let url = url.trim();
    validate_base_url(url, "OpenAI Compatible")?;
    db.set_setting(
        "openai_compatible_base_url",
        if url.is_empty() {
            DEFAULT_OPENAI_COMPATIBLE_BASE_URL
        } else {
            url
        },
    )
}

#[tauri::command]
pub fn set_openai_compatible_model(db: State<'_, Db>, model: String) -> AppResult<()> {
    let model = model.trim();
    if model.is_empty() {
        return Err(AppError::Config("model name is required".into()));
    }
    db.set_setting("openai_compatible_model", model)
}

#[tauri::command]
pub fn set_anthropic_base_url(db: State<'_, Db>, url: String) -> AppResult<()> {
    let url = url.trim();
    validate_base_url(url, "Anthropic")?;
    db.set_setting(
        "anthropic_base_url",
        if url.is_empty() {
            DEFAULT_ANTHROPIC_BASE_URL
        } else {
            url
        },
    )
}

#[tauri::command]
pub fn set_anthropic_model(db: State<'_, Db>, model: String) -> AppResult<()> {
    let model = model.trim();
    if model.is_empty() {
        return Err(AppError::Config("model name is required".into()));
    }
    db.set_setting("anthropic_model", model)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_base_url ───────────────────────────────────────────

    #[test]
    fn validate_base_url_accepts_https() {
        assert!(validate_base_url("https://api.openai.com/v1", "Test").is_ok());
    }

    #[test]
    fn validate_base_url_accepts_localhost_http() {
        assert!(validate_base_url("http://localhost:11434/v1", "Test").is_ok());
        assert!(validate_base_url("http://127.0.0.1:11434", "Test").is_ok());
    }

    #[test]
    fn validate_base_url_accepts_empty() {
        assert!(validate_base_url("", "Test").is_ok());
    }

    #[test]
    fn validate_base_url_rejects_non_local_http() {
        assert!(validate_base_url("http://api.example.com", "Test").is_err());
        assert!(validate_base_url("ftp://example.com", "Test").is_err());
    }

    #[test]
    fn validate_base_url_error_includes_label() {
        let err = validate_base_url("http://evil.com", "OpenAI Compatible").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("OpenAI Compatible"));
        assert!(msg.contains("https://"));
    }

    // ── load_ai_settings_inner defaults ─────────────────────────────

    fn mem_db() -> Db {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::db::SCHEMA).unwrap();
        crate::db::migrate(&conn).unwrap();
        Db(std::sync::Mutex::new(conn))
    }

    #[test]
    fn load_ai_settings_defaults_for_openai() {
        let db = mem_db();
        let settings = load_ai_settings_inner(&db, Provider::OpenAi, None);
        assert_eq!(settings.provider, Provider::OpenAi);
        assert_eq!(settings.model, "gpt-4o-mini");
        assert_eq!(settings.base_url, "https://api.openai.com/v1");
        assert!(settings.api_key.is_none());
    }

    #[test]
    fn load_ai_settings_defaults_for_openai_compatible() {
        let db = mem_db();
        let settings =
            load_ai_settings_inner(&db, Provider::OpenAiCompatible, Some("sk-test".into()));
        assert_eq!(settings.provider, Provider::OpenAiCompatible);
        assert_eq!(settings.model, "gpt-4o-mini");
        assert_eq!(settings.base_url, "https://api.openai.com/v1");
        assert_eq!(settings.api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn load_ai_settings_defaults_for_anthropic() {
        let db = mem_db();
        let settings = load_ai_settings_inner(&db, Provider::Anthropic, Some("sk-ant-test".into()));
        assert_eq!(settings.provider, Provider::Anthropic);
        assert_eq!(settings.model, "claude-sonnet-4-20250514");
        assert_eq!(settings.base_url, "https://api.anthropic.com");
        assert_eq!(settings.api_key.as_deref(), Some("sk-ant-test"));
    }

    #[test]
    fn load_ai_settings_defaults_for_gemini() {
        let db = mem_db();
        let settings = load_ai_settings_inner(&db, Provider::Gemini, Some("gemini-key".into()));
        assert_eq!(settings.provider, Provider::Gemini);
        assert_eq!(settings.model, "gemini-2.5-flash");
        assert_eq!(settings.base_url, "");
        assert_eq!(settings.api_key.as_deref(), Some("gemini-key"));
    }

    #[test]
    fn load_ai_settings_custom_model_is_used() {
        let db = mem_db();
        db.set_setting("anthropic_model", "claude-opus-4").unwrap();
        let settings = load_ai_settings_inner(&db, Provider::Anthropic, None);
        assert_eq!(settings.model, "claude-opus-4");
    }

    #[test]
    fn load_ai_settings_custom_base_url_is_used() {
        let db = mem_db();
        db.set_setting("anthropic_base_url", "https://api.anthropic.custom.com")
            .unwrap();
        let settings = load_ai_settings_inner(&db, Provider::Anthropic, None);
        assert_eq!(settings.base_url, "https://api.anthropic.custom.com");
    }

    #[test]
    fn load_ai_settings_ignores_empty_model_setting() {
        let db = mem_db();
        db.set_setting("anthropic_model", "").unwrap();
        let settings = load_ai_settings_inner(&db, Provider::Anthropic, None);
        assert_eq!(settings.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn load_ai_settings_openai_compatible_custom_url() {
        let db = mem_db();
        db.set_setting("openai_compatible_base_url", "http://localhost:11434/v1")
            .unwrap();
        let settings = load_ai_settings_inner(&db, Provider::OpenAiCompatible, None);
        assert_eq!(settings.base_url, "http://localhost:11434/v1");
    }
}

/// Bulk-read all AI model/endpoint settings for the frontend settings UI.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AiModelSettings {
    openai_model: String,
    gemini_model: String,
    openai_compatible_base_url: String,
    openai_compatible_model: String,
    anthropic_base_url: String,
    anthropic_model: String,
}

#[tauri::command]
pub fn get_ai_model_settings(db: State<'_, Db>) -> AppResult<AiModelSettings> {
    let s = |key, default| read_setting(&db, key, default);
    Ok(AiModelSettings {
        openai_model: s("openai_model", DEFAULT_OPENAI_MODEL),
        gemini_model: s("gemini_model", DEFAULT_GEMINI_MODEL),
        openai_compatible_base_url: s(
            "openai_compatible_base_url",
            DEFAULT_OPENAI_COMPATIBLE_BASE_URL,
        ),
        openai_compatible_model: s("openai_compatible_model", DEFAULT_OPENAI_COMPATIBLE_MODEL),
        anthropic_base_url: s("anthropic_base_url", DEFAULT_ANTHROPIC_BASE_URL),
        anthropic_model: s("anthropic_model", DEFAULT_ANTHROPIC_MODEL),
    })
}

/// Fetch the list of model IDs available for the currently-configured AI provider.
/// Uses the provider's API key and base URL from persisted settings. Returns an
/// error (not a panic) if the key is missing or the API call fails — the frontend
/// falls back to a free-text input in that case.
#[tauri::command]
pub async fn list_ai_models(db: State<'_, Db>) -> AppResult<Vec<String>> {
    let settings = resolve_ai_provider(&db)?;
    ai::list_models(&settings).await
}
