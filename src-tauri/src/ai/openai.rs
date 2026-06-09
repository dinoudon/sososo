//! OpenAI Chat Completions transport.

use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, AppResult};

/// Default model for the built-in OpenAI provider (overridable via settings).
pub(crate) const OPENAI_MODEL: &str = "gpt-4o-mini";
/// Default base URL for the built-in OpenAI provider.
pub(crate) const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: String,
}

/// OpenAI error response envelope (`{ "error": { "message": ... } }`).
#[derive(Deserialize)]
struct OpenAiErrorEnvelope {
    error: OpenAiErrorBody,
}

#[derive(Deserialize)]
struct OpenAiErrorBody {
    message: String,
}

/// OpenAI Chat Completions transport (system + single user turn) — the legacy
/// signature, kept for backwards compatibility. New callers should use
/// [`openai_chat_with_config`] directly.
#[allow(dead_code)]
pub(crate) async fn openai_chat(
    api_key: &str,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
) -> AppResult<String> {
    let messages = serde_json::json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user },
    ]);
    openai_chat_with_config(
        Some(api_key),
        messages,
        temperature,
        timeout,
        OPENAI_BASE_URL,
        OPENAI_MODEL,
    )
    .await
}

/// OpenAI Chat Completions transport given a fully-built `messages` array — the
/// legacy signature. New callers should use [`openai_chat_with_config`] directly.
#[allow(dead_code)]
pub(crate) async fn openai_chat_messages(
    api_key: &str,
    messages: serde_json::Value,
    temperature: f32,
    timeout: Duration,
) -> AppResult<String> {
    openai_chat_with_config(
        Some(api_key),
        messages,
        temperature,
        timeout,
        OPENAI_BASE_URL,
        OPENAI_MODEL,
    )
    .await
}

/// OpenAI Chat Completions transport with runtime-configurable endpoint, model,
/// and optional API key (for local/compatible endpoints without auth). When
/// `api_key` is `None`, no `Authorization` header is sent.
pub(crate) async fn openai_chat_with_config(
    api_key: Option<&str>,
    messages: serde_json::Value,
    temperature: f32,
    timeout: Duration,
    base_url: &str,
    model: &str,
) -> AppResult<String> {
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "temperature": temperature,
        "messages": messages,
    });

    let client = reqwest::Client::builder().timeout(timeout).build()?;
    let mut req = client.post(&endpoint).json(&body);
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }
    let resp = req.send().await?;

    let status = resp.status();
    let raw = resp.text().await?;

    if !status.is_success() {
        let detail = serde_json::from_str::<OpenAiErrorEnvelope>(&raw)
            .map(|e| e.error.message)
            .unwrap_or_else(|_| {
                let truncated: String = raw.chars().take(200).collect();
                format!("(unparseable response) {truncated}")
            });
        let hint = if status.as_u16() == 401 {
            " (check the OpenAI API key in Settings)"
        } else {
            ""
        };
        return Err(AppError::Ai(format!("OpenAI {status}: {detail}{hint}")));
    }

    let parsed: OpenAiChatResponse = serde_json::from_str(&raw)
        .map_err(|e| AppError::Ai(format!("could not parse OpenAI response: {e}")))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Ai("OpenAI returned no content".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_response_with_choices() {
        let json = r#"{
            "choices": [{
                "message": { "content": "Hello from OpenAI" }
            }]
        }"#;
        let parsed: OpenAiChatResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap();
        assert_eq!(text, "Hello from OpenAI");
    }

    #[test]
    fn parse_response_with_multiple_choices_takes_first() {
        let json = r#"{
            "choices": [
                { "message": { "content": "first reply" } },
                { "message": { "content": "second reply" } }
            ]
        }"#;
        let parsed: OpenAiChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.choices.len(), 2);
        let first = parsed.choices.into_iter().next().unwrap();
        assert_eq!(first.message.content, "first reply");
    }

    #[test]
    fn parse_error_envelope() {
        let json = r#"{
            "error": { "message": "Incorrect API key provided" }
        }"#;
        let parsed: OpenAiErrorEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.error.message, "Incorrect API key provided");
    }

    #[test]
    fn parse_error_envelope_with_type() {
        let json = r#"{
            "error": {
                "type": "invalid_request_error",
                "message": "The model does not exist"
            }
        }"#;
        let parsed: OpenAiErrorEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.error.message, "The model does not exist");
    }

    #[test]
    fn empty_choices_returns_none() {
        let json = r#"{"choices": []}"#;
        let parsed: OpenAiChatResponse = serde_json::from_str(json).unwrap();
        assert!(parsed.choices.is_empty());
    }

    #[test]
    fn empty_content_trimmed_is_filtered_out() {
        let json = r#"{
            "choices": [{ "message": { "content": "   " } }]
        }"#;
        let parsed: OpenAiChatResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.trim().to_string())
            .filter(|s| !s.is_empty());
        assert!(text.is_none());
    }

    #[test]
    fn content_is_trimmed_of_whitespace() {
        let json = r#"{
            "choices": [{ "message": { "content": "  trimmed text  " } }]
        }"#;
        let parsed: OpenAiChatResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.trim().to_string())
            .unwrap();
        assert_eq!(text, "trimmed text");
    }
}
