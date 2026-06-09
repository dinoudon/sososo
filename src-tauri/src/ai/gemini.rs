//! Google Gemini `generateContent` transport.

use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, AppResult};

/// Default model for the built-in Gemini provider (overridable via settings).
pub(crate) const GEMINI_MODEL: &str = "gemini-2.5-flash";
const GEMINI_ENDPOINT_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    // Absent when a candidate was blocked (e.g. safety) instead of producing text.
    content: Option<GeminiContent>,
}

#[derive(Deserialize)]
struct GeminiContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Deserialize)]
struct GeminiPart {
    #[serde(default)]
    text: String,
}

/// Gemini error response envelope (`{ "error": { "message": ... } }`).
#[derive(Deserialize)]
struct GeminiErrorEnvelope {
    error: GeminiErrorBody,
}

#[derive(Deserialize)]
struct GeminiErrorBody {
    message: String,
}

/// Gemini `generateContent` transport (system + single user turn) — the legacy
/// signature. New callers should use [`gemini_generate_with_config`] directly.
#[allow(dead_code)]
pub(crate) async fn gemini_chat(
    api_key: &str,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
) -> AppResult<String> {
    let contents = serde_json::json!([ { "role": "user", "parts": [ { "text": user } ] } ]);
    gemini_generate_with_config(api_key, system, contents, temperature, timeout, GEMINI_MODEL)
        .await
}

/// Gemini `generateContent` transport given a fully-built `contents` array — the
/// legacy signature. New callers should use [`gemini_generate_with_config`] directly.
#[allow(dead_code)]
pub(crate) async fn gemini_chat_messages(
    api_key: &str,
    system: &str,
    contents: serde_json::Value,
    temperature: f32,
    timeout: Duration,
) -> AppResult<String> {
    gemini_generate_with_config(api_key, system, contents, temperature, timeout, GEMINI_MODEL)
        .await
}

/// Gemini `generateContent` transport with runtime-configurable model name. Note
/// Gemini uses the role `"model"` (not `"assistant"`) for prior model turns —
/// callers must map accordingly when building `contents`. Auth is the
/// `x-goog-api-key` header (the key is never placed in the URL/query string).
pub(crate) async fn gemini_generate_with_config(
    api_key: &str,
    system: &str,
    contents: serde_json::Value,
    temperature: f32,
    timeout: Duration,
    model: &str,
) -> AppResult<String> {
    let url = format!("{GEMINI_ENDPOINT_BASE}/{model}:generateContent");
    let body = serde_json::json!({
        "systemInstruction": { "parts": [ { "text": system } ] },
        "contents": contents,
        "generationConfig": { "temperature": temperature },
    });

    let client = reqwest::Client::builder().timeout(timeout).build()?;
    let resp = client
        .post(&url)
        .header("x-goog-api-key", api_key)
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    let raw = resp.text().await?;

    if !status.is_success() {
        let detail = serde_json::from_str::<GeminiErrorEnvelope>(&raw)
            .map(|e| e.error.message)
            .unwrap_or_else(|_| {
                let truncated: String = raw.chars().take(200).collect();
                format!("(unparseable response) {truncated}")
            });
        let hint = match status.as_u16() {
            400 | 401 | 403 => " (check the Gemini API key in Settings)",
            _ => "",
        };
        return Err(AppError::Ai(format!("Gemini {status}: {detail}{hint}")));
    }

    let parsed: GeminiResponse = serde_json::from_str(&raw)
        .map_err(|e| AppError::Ai(format!("could not parse Gemini response: {e}")))?;
    parsed
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content)
        .and_then(|c| c.parts.into_iter().next())
        .map(|p| p.text.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Ai("Gemini returned no content".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_response_with_text() {
        let json = r#"{
            "candidates": [{
                "content": {
                    "parts": [{ "text": "Hello from Gemini" }]
                }
            }]
        }"#;
        let parsed: GeminiResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content)
            .and_then(|c| c.parts.into_iter().next())
            .map(|p| p.text)
            .unwrap();
        assert_eq!(text, "Hello from Gemini");
    }

    #[test]
    fn parse_response_with_multiple_parts() {
        let json = r#"{
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "first part" },
                        { "text": "second part" }
                    ]
                }
            }]
        }"#;
        let parsed: GeminiResponse = serde_json::from_str(json).unwrap();
        let candidate = parsed.candidates.into_iter().next().unwrap();
        let content = candidate.content.unwrap();
        assert_eq!(content.parts.len(), 2);
        assert_eq!(content.parts[0].text, "first part");
    }

    #[test]
    fn parse_response_with_multiple_candidates_takes_first() {
        let json = r#"{
            "candidates": [
                { "content": { "parts": [{ "text": "first" }] } },
                { "content": { "parts": [{ "text": "second" }] } }
            ]
        }"#;
        let parsed: GeminiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.candidates.len(), 2);
        let first = parsed
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content)
            .and_then(|c| c.parts.into_iter().next())
            .map(|p| p.text)
            .unwrap();
        assert_eq!(first, "first");
    }

    #[test]
    fn parse_response_empty_candidates() {
        let json = r#"{"candidates": []}"#;
        let parsed: GeminiResponse = serde_json::from_str(json).unwrap();
        assert!(parsed.candidates.is_empty());
    }

    #[test]
    fn parse_response_candidate_without_content() {
        let json = r#"{"candidates": [{}]}"#;
        let parsed: GeminiResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content)
            .and_then(|c| c.parts.into_iter().next())
            .map(|p| p.text.trim().to_string())
            .filter(|s| !s.is_empty());
        assert!(text.is_none());
    }

    #[test]
    fn parse_response_empty_parts() {
        let json = r#"{"candidates": [{"content": {"parts": []}}]}"#;
        let parsed: GeminiResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content)
            .and_then(|c| c.parts.into_iter().next())
            .map(|p| p.text.trim().to_string())
            .filter(|s| !s.is_empty());
        assert!(text.is_none());
    }

    #[test]
    fn parse_error_envelope() {
        let json = r#"{"error": {"message": "API key not valid"}}"#;
        let parsed: GeminiErrorEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.error.message, "API key not valid");
    }

    #[test]
    fn parse_response_text_is_trimmed() {
        let json = r#"{
            "candidates": [{
                "content": { "parts": [{ "text": "  padded text  " }] }
            }]
        }"#;
        let parsed: GeminiResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content)
            .and_then(|c| c.parts.into_iter().next())
            .map(|p| p.text.trim().to_string())
            .unwrap();
        assert_eq!(text, "padded text");
    }

    #[test]
    fn default_missing_text_is_empty_string() {
        let json = r#"{
            "candidates": [{
                "content": { "parts": [{}] }
            }]
        }"#;
        let parsed: GeminiResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content)
            .and_then(|c| c.parts.into_iter().next())
            .map(|p| p.text.trim().to_string())
            .filter(|s| !s.is_empty());
        assert!(text.is_none());
    }
}
