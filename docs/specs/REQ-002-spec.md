# Multi-Provider AI — Spec

**Feature:** REQ-002 — OpenAI-Compatible & Anthropic-Compatible API providers
**Stage:** Spec (Stage 2 of 8)
**Date:** 2026-06-08

---

## 1. Anthropic Messages API Contract

### 1.1 Request (single-turn with system)

```
POST {base_url}/v1/messages
x-api-key: {api_key}
anthropic-version: 2023-06-01
Content-Type: application/json

{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 1024,
  "system": "{system_prompt}",
  "messages": [
    { "role": "user", "content": "{user_content}" }
  ]
}
```

### 1.2 Request (multi-turn chat with history)

```
{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 1024,
  "system": "{system_prompt including transcript}",
  "messages": [
    { "role": "user", "content": "question 1" },
    { "role": "assistant", "content": "answer 1" },
    { "role": "user", "content": "question 2" }
  ]
}
```

Anthropic natively uses `"user"` / `"assistant"` roles — no role remapping needed (unlike Gemini where assistant → `"model"`).

### 1.3 Response (success)

```json
{
  "id": "msg_01XzAb...",
  "model": "claude-sonnet-4-20250514",
  "content": [
    {
      "type": "text",
      "text": "The summary..."
    }
  ],
  "stop_reason": "end_turn"
}
```

**Parsing path:** `content[0].text` from first content block where `type == "text"`.

### 1.4 Response (error)

```json
{
  "error": {
    "type": "invalid_request_error",
    "message": "Invalid API key provided"
  }
}
```

**Deserialize into:**

```rust
struct AnthropicErrorEnvelope {
    error: AnthropicErrorBody,
}
struct AnthropicErrorBody {
    message: String,
}
```

Fallback to truncated raw body (200 chars, same as OpenAI/Gemini pattern).

### 1.5 Timeout behavior

| Caller                  | Timeout |
| ----------------------- | ------- |
| `summarize`             | 90s     |
| `translate`             | 30s     |
| `chat_about_transcript` | 60s     |

Passed as `reqwest::Client::builder().timeout(timeout)` — same as OpenAI/Gemini.

---

## 2. Transport Parameterization

### 2.1 Current: Hardcoded constants

```
openai.rs:  OPENAI_MODEL = "gpt-4o-mini", OPENAI_ENDPOINT = "https://api.openai.com/v1/chat/completions"
gemini.rs:  GEMINI_MODEL = "gemini-2.5-flash", GEMINI_ENDPOINT_BASE = "https://generativelanguage.googleapis.com/v1beta/models"
```

### 2.2 After: Runtime-configurable via SettingsSnapshot

The `chat()` and `chat_about_transcript()` dispatch functions in `ai/mod.rs` receive a `SettingsSnapshot` carrying:

```rust
struct SettingsSnapshot {
    // OpenAI / OpenAI-Compatible
    openai_base_url: String,    // "https://api.openai.com/v1" or custom
    openai_model: String,       // "gpt-4o-mini" or custom

    // Gemini
    gemini_model: String,       // "gemini-2.5-flash" or custom

    // Anthropic
    anthropic_base_url: String, // "https://api.anthropic.com" or custom
    anthropic_model: String,    // "claude-sonnet-4-20250514" or custom
}
```

**Reading from DB (once per command invocation):**

```rust
fn load_settings(db: &Db) -> AppResult<SettingsSnapshot> {
    Ok(SettingsSnapshot {
        openai_base_url: db.get_setting("openai_base_url")?
            .unwrap_or_else(|| "https://api.openai.com/v1".into()),
        openai_model: db.get_setting("openai_model")?
            .unwrap_or_else(|| "gpt-4o-mini".into()),
        gemini_model: db.get_setting("gemini_model")?
            .unwrap_or_else(|| "gemini-2.5-flash".into()),
        anthropic_base_url: db.get_setting("anthropic_base_url")?
            .unwrap_or_else(|| "https://api.anthropic.com".into()),
        anthropic_model: db.get_setting("anthropic_model")?
            .unwrap_or_else(|| "claude-sonnet-4-20250514".into()),
    })
}
```

### 2.3 openai.rs changes

Replace `OPENAI_ENDPOINT` constant with parameter:

```rust
// Before:
const OPENAI_ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";

pub(crate) async fn openai_chat_messages(api_key: &str, messages: ..., timeout: ...) {
    // uses OPENAI_ENDPOINT, OPENAI_MODEL

// After:
pub(crate) async fn openai_chat_with_config(
    api_key: Option<&str>,
    messages: serde_json::Value,
    temperature: f32,
    timeout: Duration,
    base_url: &str,
    model: &str,
) -> AppResult<String> {
    let endpoint = format!("{base_url}/chat/completions");
    // ... uses endpoint and model params
}
```

Keep `openai_chat` and `openai_chat_messages` as wrappers that call through with defaults for backwards compatibility (used internally by the existing `Provider::OpenAi` path).

### 2.4 gemini.rs changes

Replace `GEMINI_MODEL` constant with parameter:

```rust
pub(crate) async fn gemini_chat_with_config(
    api_key: &str,
    system: &str,
    contents: serde_json::Value,
    temperature: f32,
    timeout: Duration,
    model: &str,  // NEW
) -> AppResult<String> {
    let url = format!("{GEMINI_ENDPOINT_BASE}/{model}:generateContent");
    // ...
}
```

Keep `gemini_chat` and `gemini_chat_messages` as wrappers calling with `GEMINI_MODEL` default.

---

## 3. Dispatch Changes

### 3.1 `ai/mod.rs` — `chat()` function

```rust
async fn chat(
    provider: Provider,
    api_key: Option<&str>,  // Changed from &str
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
    settings: &SettingsSnapshot,  // NEW
) -> AppResult<(String, String)> {
    match provider {
        Provider::OpenAi => {
            let key = api_key.ok_or_else(|| AppError::Config("OpenAI API key not set".into()))?;
            let text = openai::openai_chat_with_config(
                Some(key),
                /* messages array */,
                temperature, timeout,
                &settings.openai_base_url,
                &settings.openai_model,
            ).await?;
            Ok((text, settings.openai_model.clone()))
        }
        Provider::OpenAiCompatible => {
            let key_opt = api_key; // optional for local endpoints
            let text = openai::openai_chat_with_config(
                key_opt,
                /* messages array */,
                temperature, timeout,
                &settings.openai_base_url, // reads `openai_compatible_base_url` actually
                &settings.openai_model,     // reads `openai_compatible_model` actually
            ).await?;
            Ok((text, settings.openai_model.clone()))
        }
        Provider::Gemini => {
            let key = api_key.ok_or_else(|| AppError::Config("Gemini API key not set".into()))?;
            let text = gemini::gemini_chat_with_config(
                key,
                system, user,
                temperature, timeout,
                &settings.gemini_model,
            ).await?;
            Ok((text, settings.gemini_model.clone()))
        }
        Provider::Anthropic => {
            let text = anthropic::anthropic_chat(
                api_key,
                system, user,
                temperature, timeout,
                &settings.anthropic_base_url,
                &settings.anthropic_model,
            ).await?;
            Ok((text, settings.anthropic_model.clone()))
        }
    }
}
```

Wait — the `SettingsSnapshot` needs to carry the right field per provider. The `openai_compatible_base_url` and `openai_compatible_model` are separate settings from `openai_base_url` and `openai_model`. So:

```rust
struct SettingsSnapshot {
    // Provider-specific (maps to the active provider's config)
    base_url: String,   // depends on provider
    model: String,      // depends on provider
    // Built-in provider overrides (always available)
    openai_model: String,
    gemini_model: String,
}
```

Actually, simpler approach: load all settings, then resolve at the call site. The snapshot just holds everything:

```rust
struct SettingsSnapshot {
    openai_base_url: String,
    openai_model: String,
    openai_compatible_base_url: String,
    openai_compatible_model: String,
    gemini_model: String,
    anthropic_base_url: String,
    anthropic_model: String,
}
```

Then the dispatch picks the right fields:

```rust
match provider {
    Provider::OpenAi => (settings.openai_base_url, settings.openai_model),
    Provider::OpenAiCompatible => (settings.openai_compatible_base_url, settings.openai_compatible_model),
    Provider::Gemini => (settings.gemini_model, ...),
    Provider::Anthropic => (settings.anthropic_base_url, settings.anthropic_model),
}
```

### 3.2 `ai/mod.rs` — `chat_about_transcript()` function

Same dispatch pattern, but handles multi-turn `messages` array. For Anthropic, this means building:

```rust
Provider::Anthropic => {
    let mut messages: Vec<serde_json::Value> = Vec::with_capacity(history.len() + 1);
    for turn in history {
        let role = &turn.role; // already "user" or "assistant" — no remapping
        messages.push(serde_json::json!({ "role": role, "content": turn.content }));
    }
    messages.push(serde_json::json!({ "role": "user", "content": question }));
    let text = anthropic::anthropic_messages(
        api_key,
        system, // passed as system param, not in messages array
        messages, // user/assistant turns only
        temperature, timeout,
        &settings.anthropic_base_url,
        &settings.anthropic_model,
    ).await?;
    Ok((text, settings.anthropic_model.clone()))
}
```

---

## 4. Command Changes

### 4.1 `resolve_ai_provider` signature change

```rust
// Before:
fn resolve_ai_provider(db: &Db) -> AppResult<(ai::Provider, String)> {
    // always returns api_key

// After:
fn resolve_ai_provider(db: &Db) -> AppResult<(ai::Provider, Option<String>, SettingsSnapshot)> {
```

For `OpenAiCompatible` and `Anthropic`, the key is optional:

```rust
Provider::OpenAiCompatible | Provider::Anthropic => {
    let key = keys::get_api_key(provider.key_service())?.ok(); // Ok(None) not Err
    Ok((provider, key, settings))
}
```

Wait no — `get_api_key` already returns `Result<Option<String>>` with `NoEntry → Ok(None)`. So:

```rust
let key = keys::get_api_key(provider.key_service())?;
// For built-in providers, key is required:
match provider {
    Provider::OpenAi | Provider::Gemini => {
        if key.is_none() {
            return Err(AppError::Config(format!("{} API key is not set", provider.label())));
        }
    }
    Provider::OpenAiCompatible | Provider::Anthropic => {
        // optional — fine to be None
    }
}
```

### 4.2 New commands for model/URL settings

```
get_ai_model_settings  → returns AiModelSettings { openai_model, gemini_model, ... }
set_openai_model(model: String)
set_gemini_model(model: String)
set_openai_compatible_base_url(url: String)
set_openai_compatible_model(model: String)
set_anthropic_base_url(url: String)
set_anthropic_model(model: String)
```

Or, simpler: one command per setting (matching existing pattern of individual commands like `get_ai_provider` / `set_ai_provider`).

---

## 5. Frontend Settings UI

### 5.1 Wireframe

```
┌─ Settings ──────────────────────────────────────────────┐
│                                                          │
│  🔑 API Keys                                             │
│  ┌──────────────────────────────────────────┐            │
│  │ Deepgram is required...                   │            │
│  └──────────────────────────────────────────┘            │
│                                                          │
│  Speech-to-text engine                     [Required]    │
│  ┌ Deepgram API Key ───────────────────────┐            │
│  │ [••••••••••••]              [Save]      │            │
│  └──────────────────────────────────────────┘            │
│                                                          │
│  AI summaries & translation               [Optional]     │
│                                                          │
│  Active AI provider:                                     │
│  ┌─────────────────▼──┐                                  │
│  │ OpenAI             │                                  │
│  │ OpenAI Compatible  │  ← new                          │
│  │ Gemini             │                                  │
│  │ Anthropic          │  ← new                          │
│  └────────────────────┘                                  │
│                                                          │
│  ── When "OpenAI" selected: ──                           │
│  ┌ Model ───────────────────────────────────┐           │
│  │ [gpt-4o-mini             ] (editable)    │  ← new    │
│  └──────────────────────────────────────────┘           │
│  ┌ OpenAI API Key ── [✅ saved] ──────────┐             │
│  │ [sk-...]                    [Save]     │             │
│  └─────────────────────────────────────────┘            │
│  Key stored in Windows Credential Manager               │
│                                                          │
│  ── When "OpenAI Compatible" selected: ──                │
│  ┌ Base URL ──────────────────────────────────┐        │
│  │ [https://api.openai.com/v1 ]               │        │
│  └─────────────────────────────────────────────┘        │
│  ┌ Model ────────────────────────────────────┐         │
│  │ [gpt-4o-mini              ]               │         │
│  └─────────────────────────────────────────────┘        │
│  ┌ API Key (optional for local endpoints) ───┐         │
│  │ [                         ]    [Save]     │         │
│  └─────────────────────────────────────────────┘       │
│                                                          │
│  ── When "Anthropic" selected: ──                        │
│  ┌ Base URL ──────────────────────────────────┐        │
│  │ [https://api.anthropic.com ]                │        │
│  └─────────────────────────────────────────────┘        │
│  ┌ Model ────────────────────────────────────┐         │
│  │ [claude-sonnet-4-20250514  ]                │        │
│  └─────────────────────────────────────────────┘        │
│  ┌ Anthropic API Key ────────────────────────┐         │
│  │ [sk-ant-...]                [Save]         │         │
│  └─────────────────────────────────────────────┘        │
│                                                          │
│  ☑ Auto-summarize when a recording finishes              │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

### 5.2 Component state

```typescript
// In SettingsRoute.tsx — new state variables
const [oaModel, setOaModel] = useState('gpt-4o-mini');
const [oaCompatibleBaseUrl, setOaCompatibleBaseUrl] = useState('https://api.openai.com/v1');
const [oaCompatibleModel, setOaCompatibleModel] = useState('gpt-4o-mini');
const [oaCompatibleKey, setOaCompatibleKey] = useState('');
const [oaCompatibleSaved, setOaCompatibleSaved] = useState(false);
const [anBaseUrl, setAnBaseUrl] = useState('https://api.anthropic.com');
const [anModel, setAnModel] = useState('claude-sonnet-4-20250514');
const [anKey, setAnKey] = useState('');
const [anSaved, setAnSaved] = useState(false);
```

### 5.3 Provider-specific fields (conditional rendering)

```tsx
{aiProvider === 'openai' && (
  <label className={FIELD}>
    <span className={FIELD_LABEL}>Model</span>
    <input className={FIELD_CTRL} value={oaModel} ... />
  </label>
)}
{aiProvider === 'openai-compatible' && (
  <>
    <label className={FIELD}>
      <span className={FIELD_LABEL}>Base URL</span>
      <input className={FIELD_CTRL} value={oaCompatibleBaseUrl} ... />
    </label>
    <label className={FIELD}>
      <span className={FIELD_LABEL}>Model</span>
      <input className={FIELD_CTRL} value={oaCompatibleModel} ... />
    </label>
    <label className={FIELD}>
      <span className={FIELD_LABEL}>API Key (optional for local endpoints)</span>
      <input className={FIELD_CTRL} type="password" value={oaCompatibleKey} ... />
    </label>
  </>
)}
{aiProvider === 'anthropic' && (
  <>
    <label className={FIELD}>
      <span className={FIELD_LABEL}>Base URL</span>
      <input className={FIELD_CTRL} value={anBaseUrl} ... />
    </label>
    <label className={FIELD}>
      <span className={FIELD_LABEL}>Model</span>
      <input className={FIELD_CTRL} value={anModel} ... />
    </label>
    <label className={FIELD}>
      <span className={FIELD_LABEL}>Anthropic API Key</span>
      <input className={FIELD_CTRL} type="password" value={anKey} ... />
    </label>
  </>
)}
```

---

## 6. Domain Types

### 6.1 `src/types/domain.ts` changes

```typescript
export type ApiService = 'deepgram' | 'openai' | 'openai-compatible' | 'gemini' | 'anthropic';
export type AiProvider = 'openai' | 'openai-compatible' | 'gemini' | 'anthropic';
```

### 6.2 `src/lib/ipc.ts` changes

New wrapper functions for model/base-url settings (matching existing individual-command pattern):

```typescript
export const getAiModelSettings = (): Promise<AiModelSettings> => invoke('get_ai_model_settings');

export const setOpenaiModel = (model: string): Promise<void> =>
  invoke('set_openai_model', { model });
export const setOpenaiCompatibleBaseUrl = (url: string): Promise<void> =>
  invoke('set_openai_compatible_base_url', { url });
export const setOpenaiCompatibleModel = (model: string): Promise<void> =>
  invoke('set_openai_compatible_model', { model });
export const setAnthropicBaseUrl = (url: string): Promise<void> =>
  invoke('set_anthropic_base_url', { url });
export const setAnthropicModel = (model: string): Promise<void> =>
  invoke('set_anthropic_model', { model });
```

---

## 7. URL Validation (Security)

```rust
fn validate_base_url(url: &str, provider_label: &str) -> AppResult<()> {
    if url.is_empty() { return Ok(()); }  // will use default
    let lower = url.to_ascii_lowercase();
    // Allow https:// (always) and http://localhost/127.0.0.1 (local models)
    if lower.starts_with("https://") { return Ok(()); }
    if lower.starts_with("http://localhost") || lower.starts_with("http://127.0.0.1") { return Ok(()); }
    Err(AppError::Config(format!(
        "{provider_label} base URL must use https:// or be a local http://localhost address"
    )))
}
```

Applied in `set_openai_compatible_base_url` and `set_anthropic_base_url` commands.

---

## 8. Test Plan

| File                | Test                                                                                                       | Type              |
| ------------------- | ---------------------------------------------------------------------------------------------------------- | ----------------- |
| `provider.rs`       | `from_setting` parses 4 variants + case-insensitive + default                                              | Unit (existing)   |
| `anthropic.rs`      | `#[cfg(test)]` — single-turn request shape, multi-turn, error envelope, empty response, truncated fallback | Unit (inline)     |
| `openai.rs`         | Custom base URL builds correct endpoint, missing key → no auth header                                      | Unit (inline)     |
| `ai/mod.rs`         | `SettingsSnapshot::load()` — defaults filled, custom values read                                           | Unit (inline)     |
| `db/tests.rs`       | New model/URL settings read/write roundtrip                                                                | Integration       |
| `assistant.rs`      | `resolve_ai_provider` returns optional key for custom providers                                            | Integration       |
| `SettingsRoute.tsx` | Provider dropdown shows/hides correct fields                                                               | Frontend (Vitest) |
