---
id: REQ-002
priority: P1
status: draft
type: functional
stakeholder: growth
sprint: 0.8.0
---

# Multi-Provider AI: OpenAI-Compatible & Anthropic-Compatible APIs

As a user, I want to use any OpenAI-compatible or Anthropic-compatible API endpoint with my own API key, so that I can run summaries and chat with local models (Ollama, LM Studio), alternative cloud providers (Groq, Together, OpenRouter, Azure), and Anthropic Claude — not just OpenAI and Gemini.

---

## User Stories

### US-1: Custom OpenAI-Compatible Provider

**As a** user with access to an OpenAI-compatible API (Ollama, LM Studio, Groq, Together AI, Azure OpenAI, OpenRouter),
**I want to** configure a custom base URL, model name, and API key for the "OpenAI-compatible" provider slot,
**so that** I can use my preferred model for summarization, translation, and transcript chat.

### US-2: Anthropic-Compatible Provider

**As a** user with an Anthropic API key or access to an Anthropic-compatible endpoint (Claude via OpenRouter, Amazon Bedrock),
**I want to** select "Anthropic" as my AI provider and configure the base URL, model name, and API key,
**so that** I can use Claude models for summarization, translation, and transcript chat.

### US-3: Per-Provider Model Selection

**As a** user switching between providers,
**I want to** configure the model name independently for each provider (including the built-in OpenAI and Gemini defaults),
**so that** I can choose the right model for my cost/latency/quality needs without editing config files.

### US-4: API Key Optional for Local Endpoints

**As a** user running a local model (Ollama, LM Studio) without authentication,
**I want to** leave the API key field empty and still use the provider,
**so that** I'm not forced to enter a meaningless key for localhost endpoints.

### US-5: Preserve Existing Experience

**As an** existing user with OpenAI or Gemini configured,
**I want** my current setup to work identically after the upgrade,
**so that** nothing breaks when I update.

---

## Acceptance Criteria

### AC-1: Provider Model

- [ ] `Provider` enum gains `OpenAiCompatible` and `Anthropic` variants
- [ ] `Provider::from_setting()` maps `"openai-compatible"` and `"anthropic"` (case-insensitive)
- [ ] Backwards-compatible: `"openai"` and `"gemini"` continue to work unchanged
- [ ] Unknown strings default to `OpenAi` (existing behavior preserved)

### AC-2: Custom Endpoint & Model — Settings Persistence

- [ ] New `app_settings` keys:
  - `openai_compatible_base_url` (empty → defaults to `https://api.openai.com/v1`)
  - `openai_compatible_model` (empty → defaults to `gpt-4o-mini`)
  - `anthropic_base_url` (empty → defaults to `https://api.anthropic.com`)
  - `anthropic_model` (empty → defaults to `claude-sonnet-4-20250514`)
- [ ] `ai_provider` setting gains two new valid values: `"openai-compatible"` and `"anthropic"`
- [ ] `set_ai_provider` command accepts all four values
- [ ] Model name for OpenAI and Gemini is configurable via same mechanism: `openai_model`, `gemini_model` keys

### AC-3: OpenAI-Compatible Transport

- [ ] Reuses existing `openai.rs` transport code
- [ ] When provider is `OpenAiCompatible`, reads `openai_compatible_base_url` and `openai_compatible_model` from settings
- [ ] Endpoint constructed as `{base_url}/chat/completions`
- [ ] Auth header: `Authorization: Bearer {api_key}` when key is set; no header when key is empty
- [ ] Response format identical to OpenAI (`choices[0].message.content`)
- [ ] Error response format attempts OpenAI envelope, falls back to truncated raw body (existing behavior)

### AC-4: Anthropic Transport

- [ ] New `src-tauri/src/ai/anthropic.rs` module implementing the [Anthropic Messages API](https://docs.anthropic.com/en/api/messages)
- [ ] Endpoint: `POST {base_url}/v1/messages`
- [ ] Auth header: `x-api-key: {api_key}` (Anthropic's required format)
- [ ] Request shape:
  ```json
  {
    "model": "{model}",
    "max_tokens": 1024,
    "system": "{system_prompt}",
    "messages": [{ "role": "user", "content": "{user_content}" }]
  }
  ```
- [ ] Multi-turn chat uses `messages` array with alternating user/assistant roles (Anthropic supports `assistant` natively, unlike Gemini which requires `"model"`)
- [ ] Response parsing: `content[0].text` from the first `ContentBlock`
- [ ] Error response format: `{ "error": { "message": "..." } }` (Anthropic error envelope)
- [ ] Timeout from existing callers (30s for translate, 60s for chat, 90s for summarize) respected

### AC-5: API Key Handling

- [ ] `keys.rs` supports `"openai-compatible"` and `"anthropic"` service names (keychain entries)
- [ ] `set_api_key` command accepts these service names
- [ ] `has_api_key` works for all four services
- [ ] `resolve_ai_provider` in `commands/assistant.rs`: for `OpenAiCompatible` and `Anthropic`, key is optional (returns `None` without error when unset)
- [ ] Provider dispatch in `ai/mod.rs`: passes `Option<&str>` for API key instead of `&str`

### AC-6: Settings UI

- [ ] Settings → AI section shows provider dropdown: "OpenAI", "OpenAI Compatible", "Gemini", "Anthropic"
- [ ] Selecting "OpenAI Compatible" reveals fields: Base URL, Model, API Key
- [ ] Selecting "Anthropic" reveals fields: Base URL, Model, API Key
- [ ] Base URL and Model pre-filled with defaults, editable
- [ ] API Key field is `type="password"`, with "Show" toggle
- [ ] Key presence indicator (✅ set / ⚠️ not set) shown for each provider
- [ ] "OpenAI" and "Gemini" providers also show Model field (editable, overriding the hardcoded constant)

### AC-7: Backwards Compatibility

- [ ] Existing `openai` and `gemini` keychain entries continue to work (no data migration needed)
- [ ] Default model names unchanged: OpenAI = `gpt-4o-mini`, Gemini = `gemini-2.5-flash`
- [ ] Users who never touch the new settings see zero behavioral change
- [ ] `ai_provider` setting defaults to `"openai"` (existing behavior)
- [ ] `ai_provider` migration: any value other than `"openai"`, `"gemini"`, `"openai-compatible"`, `"anthropic"` falls back to `"openai"`

---

## Technical Design

### Architecture Changes

```
src-tauri/src/ai/
├── mod.rs              # Dispatch: match Provider, resolve key, route to transport
├── provider.rs         # Provider enum (+OpenAiCompatible, +Anthropic)
├── openai.rs           # Unchanged (OpenAI transport, endpoint/configurable)
├── anthropic.rs        # NEW — Anthropic Messages API transport
├── gemini.rs           # Unchanged
└── transcript.rs       # Unchanged

src-tauri/src/commands/
├── assistant.rs        # resolve_ai_provider → Option<key> for custom providers
└── apikeys.rs          # Unchanged (service names: "openai-compatible", "anthropic")

src-tauri/src/
└── keys.rs             # Unchanged (generic key-value store per service)

src-tauri/src/db/
└── settings.rs         # Unchanged (generic key-value store)
```

### Key Routing Change

**Current:**

```rust
fn chat(provider: Provider, api_key: &str, system: &str, user: &str, ...) {
    match provider {
        Provider::OpenAi => openai::openai_chat(api_key, system, user, ...),
        Provider::Gemini => gemini::gemini_chat(api_key, system, user, ...),
    }
}
```

**After:**

```rust
fn chat(provider: Provider, api_key: Option<&str>, system: &str, user: &str, ...,
        settings: &SettingsSnapshot) {  // carries base_url + model overrides
    match provider {
        Provider::OpenAi | Provider::OpenAiCompatible => {
            let url = settings.openai_base_url();
            let model = settings.openai_model();
            openai::openai_chat_with_config(api_key, system, user, url, model, ...)
        }
        Provider::Gemini => {
            let model = settings.gemini_model();
            gemini::gemini_chat(api_key, system, user, model, ...)
        }
        Provider::Anthropic => {
            let url = settings.anthropic_base_url();
            let model = settings.anthropic_model();
            anthropic::anthropic_chat(api_key, system, user, url, model, ...)
        }
    }
}
```

### SettingsSnapshot (read once per command, avoids repeated DB lookups)

```rust
struct SettingsSnapshot {
    openai_base_url: String,
    openai_model: String,
    gemini_model: String,
    anthropic_base_url: String,
    anthropic_model: String,
}
```

Filled from `app_settings` table with defaults for unset keys.

### Frontend Changes

**`src/types/domain.ts`:**

```typescript
export type AiProvider = 'openai' | 'openai-compatible' | 'gemini' | 'anthropic';

export interface AiSettings {
  provider: AiProvider;
  openaiModel: string;
  openaiCompatibleBaseUrl: string;
  openaiCompatibleModel: string;
  anthropicBaseUrl: string;
  anthropicModel: string;
}
```

**`src/state/configStore.ts`:**
New Tauri commands: `get_ai_settings` / `set_ai_settings` (or individual commands per field — match existing pattern of `get_ai_provider` / `set_ai_provider`).

### Database Migrations

No schema change needed — all new settings are `app_settings` key-value pairs. Migration code in `db/mod.rs` not required.

---

## Testing Seams

| Layer               | What to test                                                             | How                                     |
| ------------------- | ------------------------------------------------------------------------ | --------------------------------------- |
| `provider.rs`       | `from_setting` parses all 4 variants, case-insensitive, unknown → OpenAI | Unit test (existing pattern)            |
| `openai.rs`         | Custom base URL used instead of `api.openai.com`                         | Unit test with `httptest` or `wiremock` |
| `anthropic.rs`      | Messages API request/response shape matches Anthropic spec               | Unit test with `httptest` or `wiremock` |
| `anthropic.rs`      | Multi-turn chat maps `assistant` → `assistant` (not `model`)             | Unit test                               |
| `anthropic.rs`      | Error envelope parsing + truncated fallback                              | Unit test                               |
| `ai/mod.rs`         | Dispatch routes to correct transport with correct config                 | Integration test in `db/tests.rs`       |
| `keys.rs`           | `"openai-compatible"` and `"anthropic"` services work in keychain        | Integration test                        |
| `SettingsRoute.tsx` | Provider dropdown shows/hides correct fields                             | Frontend test                           |

---

## Non-Functional Requirements

- **Security:** Custom base URLs must be validated to not allow `file://` or `localhost` paths to internal services when key is set (defense in depth). Validate scheme is `https://` (unless it's `http://localhost` or `http://127.0.0.1` for local models).
- **Performance:** SettingsSnapshot read once per AI command invocation (not per HTTP request) — negligible overhead.
- **Privacy:** Custom base URL stored in SQLite `app_settings` (plaintext) — same privacy model as other settings. Document that custom endpoint URLs are stored locally.
- **UX:** Empty API key for local providers should not show "Set API key" nag in Settings when provider is selected.

---

## Prioritization

**RICE Score:**

| Capability                       | Reach | Impact       | Confidence | Effort  | Score |
| -------------------------------- | ----- | ------------ | ---------- | ------- | ----- |
| Anthropic transport (new module) | 40%   | High (0.8)   | 90%        | L (5pt) | 5.8   |
| OpenAI-compatible custom URL     | 30%   | High (0.8)   | 90%        | M (3pt) | 7.2   |
| Per-provider model selection     | 80%   | Medium (0.5) | 95%        | S (2pt) | 19.0  |
| Settings UI (dropdown + fields)  | 100%  | High (0.8)   | 90%        | M (3pt) | 24.0  |
| API key optional for local       | 15%   | Medium (0.5) | 85%        | S (1pt) | 6.4   |

**MoSCoW:**

- **Must have:** OpenAI-compatible custom URL + model, Anthropic transport, Settings UI
- **Should have:** Per-provider model selection for built-in OpenAI and Gemini
- **Could have:** Pre-filled provider presets (Groq, Together, OpenRouter) as quick-select dropdown
- **Won't have (this release):** Streaming responses, multiple concurrent providers, function calling

---

## Stakeholders

| Role                    | Interest                                  |
| ----------------------- | ----------------------------------------- |
| Solo developer (Yusup)  | Implementation, maintenance burden        |
| Privacy-conscious users | Local models = no data leaves machine     |
| Cost-conscious users    | OpenRouter / Groq cheaper than direct API |
| Power users             | Bring their own fine-tuned models         |
| Enterprise users        | Azure OpenAI / Bedrock compliance         |
