// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 Yusup Supriyadi
//! Anthropic Messages API transport.

use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, AppResult};

const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicErrorEnvelope {
    error: AnthropicErrorBody,
}

#[derive(Deserialize)]
struct AnthropicErrorBody {
    message: String,
}

/// Single-turn chat (system + one user message). Thin wrapper over
/// [`anthropic_messages`].
pub(crate) async fn anthropic_chat(
    api_key: Option<&str>,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
    base_url: &str,
    model: &str,
) -> AppResult<String> {
    let messages = serde_json::json!([{ "role": "user", "content": user }]);
    anthropic_messages(
        api_key,
        system,
        messages,
        temperature,
        timeout,
        base_url,
        model,
    )
    .await
}

/// Multi-turn Messages API transport given a fully-built `messages` array (for
/// transcript chat with prior turns). The `system` prompt is passed as a top-level
/// field (Anthropic's preferred shape), not embedded in the messages array. Auth
/// is the `x-api-key` header; when `api_key` is `None` the header is omitted (for
/// local/compatible endpoints without authentication).
pub(crate) async fn anthropic_messages(
    api_key: Option<&str>,
    system: &str,
    messages: serde_json::Value,
    temperature: f32,
    timeout: Duration,
    base_url: &str,
    model: &str,
) -> AppResult<String> {
    let endpoint = format!("{}/v1/messages", base_url.trim_end_matches('/'));

    // Anthropic requires temperature > 0; clamp if caller passes 0.
    let temp = if temperature < 0.01 {
        0.01
    } else {
        temperature
    };

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 1024,
        "temperature": temp,
        "stream": false,
        "system": system,
        "messages": messages,
    });

    let client = reqwest::Client::builder().timeout(timeout).build()?;
    let mut req = client
        .post(&endpoint)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&body);
    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }
    let resp = req.send().await?;

    let status = resp.status();
    let raw = resp.text().await?;

    if !status.is_success() {
        let detail = serde_json::from_str::<AnthropicErrorEnvelope>(&raw)
            .map(|e| e.error.message)
            .unwrap_or_else(|_| {
                let truncated: String = raw.chars().take(200).collect();
                format!("(unparseable response) {truncated}")
            });
        let hint = match status.as_u16() {
            401 | 403 => " (check the Anthropic API key in Settings)",
            _ => "",
        };
        return Err(AppError::Ai(format!("Anthropic {status}: {detail}{hint}")));
    }

    let parsed: AnthropicResponse = serde_json::from_str(&raw)
        .map_err(|e| AppError::Ai(format!("could not parse Anthropic response: {e}")))?;
    parsed
        .content
        .into_iter()
        .find(|b| b.block_type == "text")
        .and_then(|b| b.text)
        .map(|t| t.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Ai("Anthropic returned no content".into()))
}

/// List available Anthropic models from the `/v1/models` endpoint, sorted
/// alphabetically. When `api_key` is `None`, the header is omitted.
pub(crate) async fn list_models(
    api_key: Option<&str>,
    base_url: &str,
    timeout: Duration,
) -> AppResult<Vec<String>> {
    #[derive(Deserialize)]
    struct ModelsResponse {
        data: Vec<ModelObject>,
    }
    #[derive(Deserialize)]
    struct ModelObject {
        id: String,
    }

    let endpoint = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder().timeout(timeout).build()?;
    let mut req = client
        .get(&endpoint)
        .header("anthropic-version", ANTHROPIC_VERSION);
    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }
    let resp = req.send().await?;
    let status = resp.status();
    let raw = resp.text().await?;
    if !status.is_success() {
        let detail = serde_json::from_str::<AnthropicErrorEnvelope>(&raw)
            .map(|e| e.error.message)
            .unwrap_or_else(|_| raw.chars().take(200).collect());
        return Err(AppError::Ai(format!(
            "models list failed ({status}): {detail}"
        )));
    }
    let parsed: ModelsResponse = serde_json::from_str(&raw)
        .map_err(|e| AppError::Ai(format!("could not parse models list: {e}")))?;
    let mut ids: Vec<String> = parsed.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_response_with_text_block() {
        let json = r#"{"content":[{"type":"text","text":"Hello world"}]}"#;
        let parsed: AnthropicResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .content
            .into_iter()
            .find(|b| b.block_type == "text")
            .and_then(|b| b.text)
            .unwrap();
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn parse_response_skips_tool_use_block() {
        let json =
            r#"{"content":[{"type":"tool_use","name":"foo"},{"type":"text","text":"the answer"}]}"#;
        let parsed: AnthropicResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .content
            .into_iter()
            .find(|b| b.block_type == "text")
            .and_then(|b| b.text)
            .unwrap();
        assert_eq!(text, "the answer");
    }

    #[test]
    fn parse_error_envelope() {
        let json = r#"{"error":{"type":"invalid_request_error","message":"Invalid API key"}}"#;
        let parsed: AnthropicErrorEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.error.message, "Invalid API key");
    }

    #[test]
    fn empty_content_returns_none() {
        let json = r#"{"content":[]}"#;
        let parsed: AnthropicResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .content
            .into_iter()
            .find(|b| b.block_type == "text")
            .and_then(|b| b.text);
        assert!(text.is_none());
    }

    #[test]
    fn parse_response_with_multiple_text_blocks_uses_first() {
        let json = r#"{"content":[
            {"type":"text","text":"first text"},
            {"type":"text","text":"second text"}
        ]}"#;
        let parsed: AnthropicResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .content
            .into_iter()
            .find(|b| b.block_type == "text")
            .and_then(|b| b.text)
            .unwrap();
        assert_eq!(text, "first text");
    }

    #[test]
    fn parse_response_with_unknown_block_type_is_skipped() {
        let json = r#"{"content":[
            {"type":"thinking","thinking":"hmm"},
            {"type":"text","text":"real answer"}
        ]}"#;
        let parsed: AnthropicResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .content
            .into_iter()
            .find(|b| b.block_type == "text")
            .and_then(|b| b.text)
            .unwrap();
        assert_eq!(text, "real answer");
    }

    #[test]
    fn parse_response_with_only_unknown_blocks_returns_none() {
        let json = r#"{"content":[
            {"type":"thinking","thinking":"hmm"}
        ]}"#;
        let parsed: AnthropicResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .content
            .into_iter()
            .find(|b| b.block_type == "text")
            .and_then(|b| b.text);
        assert!(text.is_none());
    }

    #[test]
    fn parse_response_text_is_trimmed() {
        let json = r#"{"content":[{"type":"text","text":"  padded  "}]}"#;
        let parsed: AnthropicResponse = serde_json::from_str(json).unwrap();
        let text = parsed
            .content
            .into_iter()
            .find(|b| b.block_type == "text")
            .and_then(|b| b.text)
            .map(|t| t.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap();
        assert_eq!(text, "padded");
    }

    #[test]
    fn parse_error_envelope_with_type_field() {
        let json = r#"{"error":{"type":"rate_limit_error","message":"Rate limit exceeded"}}"#;
        let parsed: AnthropicErrorEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.error.message, "Rate limit exceeded");
    }
}
