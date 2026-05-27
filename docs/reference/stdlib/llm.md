# `std.llm`

Typed LLM provider abstraction. Shipped in v0.26 (Track A) — the
single biggest gap between Mighty and "the standard language for
agents". One trait, four backends.

| Provider | Status (v0.27) | Rust API |
|---|---|---|
| Anthropic Messages | **full** — HTTP + streaming + tool-use + budgets (v0.26) | [`AnthropicClient`](https://docs.rs/mty-stdlib/latest/mty_stdlib/llm/anthropic/) |
| OpenAI Responses | **full** — HTTP + SSE streaming + tools + structured outputs (v0.27) | [`OpenAiClient`](https://docs.rs/mty-stdlib/latest/mty_stdlib/llm/openai/) |
| Google Gemini `generateContent` | **full** — HTTP + `alt=sse` streaming + tools + safety settings (v0.27) | [`GeminiClient`](https://docs.rs/mty-stdlib/latest/mty_stdlib/llm/gemini/) |
| AWS Bedrock Converse | **full** — SigV4 + ConverseStream binary event-stream + tools (v0.27) | [`BedrockClient`](https://docs.rs/mty-stdlib/latest/mty_stdlib/llm/bedrock/) |

v0.27 Track C promoted the three skeletons (OpenAI / Gemini / Bedrock)
to full implementations. All four backends now ship the same shape:
HTTP/1.1 round-trip + streaming + tool-use + budget short-circuit +
typed error variants.

## Provider-specific quirks

The trait surface is uniform, but each upstream has wire-format
peculiarities the provider modules paper over:

| Concern | Anthropic | OpenAI | Gemini | Bedrock |
|---|---|---|---|---|
| Endpoint | `/v1/messages` | `/v1/responses` | `/v1beta/models/<M>:generateContent` | `/model/<M>/converse` |
| Auth header | `x-api-key` | `Authorization: Bearer` | `?key=` URL param | SigV4 OR `Authorization: Bearer` |
| System prompt | top-level `system` field | first `developer`-role item | `systemInstruction` field | top-level `system` array |
| Assistant role | `assistant` | `assistant` | **`model`** | `assistant` |
| Tool def shape | `{name, description, input_schema}` | `{type:"function", name, parameters}` | `{functionDeclarations: [{name, parameters}]}` | `{toolSpec: {name, inputSchema: {json}}}` |
| Tool call carry | `tool_use` content block w/ `id` | `function_call` output item w/ `call_id` | `functionCall` part (no id; we synthesise `gem_<name>`) | `toolUse` content block w/ `toolUseId` |
| Streaming envelope | SSE w/ named events | SSE w/ `response.*` event types | SSE (`alt=sse` opt-in); JSON-array otherwise | **Binary event-stream** (AWS proprietary framing) |
| Terminal event | `message_stop` / `message_delta.stop_reason` | `response.completed` | `finishReason` on candidate | `messageStop.stopReason` |
| `ToolChoice::Any` | `{type:"any"}` | `"required"` | `functionCallingConfig.mode = "ANY"` | `{any: {}}` |
| `ToolChoice::Tool{name}` | `{type:"tool", name}` | `{type:"function", name}` | `mode="ANY" + allowedFunctionNames=[name]` | `{tool: {name}}` |
| Rate-limit signal | 429 + `retry-after` | 429 + `retry-after` | 429 + `retry-after` | 429 + `retry-after` |

### Bedrock SigV4 + event-stream notes

Bedrock requests are signed with AWS Signature Version 4 against the
`bedrock` service. The signing key derives `secret → date → region →
service → "aws4_request"` via HMAC-SHA256, and the canonical request
hashes method + URI + query + sorted headers + body. We implement
this inline on top of `sha2` rather than pulling `aws-sigv4` + the
`aws-smithy-*` tree in.

ConverseStream uses AWS's binary event-stream framing (not SSE).
Each frame is:

```text
┌────────────┬────────────┬────────────┬──────────┬─────────┬────────────┐
│ total_len  │ headers_len│ prelude_crc│ headers  │ payload │ message_crc│
│  (4 bytes) │  (4 bytes) │  (4 bytes) │   (var)  │  (var)  │  (4 bytes) │
└────────────┴────────────┴────────────┴──────────┴─────────┴────────────┘
```

We parse the `:event-type` header (header value-type `7` = string) to
discriminate between `messageStart` / `contentBlockDelta` /
`messageStop` and project each into the typed [`MessageDelta`]
stream. The message CRC isn't validated — TLS already guarantees
end-to-end integrity.

## Mighty surface

```mty
use std.llm

let reply = anthropic.messages(
  model: "claude-opus-4-7",
  system: "You are a careful code reviewer.",
  messages: history,
  tools: [search_tool, write_tool],
) effect {net, model}
```

The `model` effect is registered alongside `net`, `dom`, `spawn` in
`mty_types::prelude::build_prelude`; the `std.llm` module is opaque
to the typechecker and dispatches through the permissive method
table (same shape as `std.http`).

## Rust surface

### One-shot completion

```rust
use mty_stdlib::llm::{
    anthropic::AnthropicClient,
    provider::{CompletionRequest, LlmProvider},
    message::Message,
};

let client = AnthropicClient::from_env()?;
let req = CompletionRequest::new(
    "claude-opus-4-7",
    vec![Message::user_text("Why is the sky blue?")],
)
.with_system("Be brief.")
.with_max_tokens(512);
let reply = client.complete(req).await?;
println!("{}", reply.text());
```

### Streaming

```rust
use futures_util::StreamExt;

let mut stream = client.complete_stream(req).await?;
while let Some(delta) = stream.next().await {
    match delta? {
        MessageDelta::TextDelta { text } => print!("{text}"),
        MessageDelta::ToolUseDelta { id, name, input_partial } => {
            // Tool input arrives as fragmented JSON; stitch and parse
            // when the run ends.
        }
        MessageDelta::Done { stop_reason } => break,
    }
}
```

### Tools

```rust
use mty_stdlib::llm::tools::Tool;
use serde_json::json;

let search = Tool::new(
    "search",
    "Look up documents in the knowledge base.",
    json!({
        "type": "object",
        "properties": { "q": { "type": "string" } },
        "required": ["q"],
    }),
);

let req = CompletionRequest::new("claude-opus-4-7", history)
    .with_tools(vec![search]);
let reply = client.complete(req).await?;
for tool_use in reply.tool_uses() {
    // dispatch to the @tool registry (Track B) ...
}
```

### Budgets

`TokenBudget` and `DollarBudget` are typed, ref-counted handles that
cap a chain of completions. Cloning either *shares* the underlying
counter, so a parent agent can hand a single budget down through its
children:

```rust
use mty_stdlib::llm::budget::{TokenBudget, DollarBudget};

let tokens = TokenBudget::new(10_000);
let dollars = DollarBudget::new(50); // $0.50 cap

let req = CompletionRequest::new("claude-opus-4-7", history)
    .with_token_budget(tokens.clone())
    .with_dollar_budget(dollars.clone());

match client.complete(req).await {
    Ok(reply) => { /* ... */ }
    Err(LlmError::BudgetExhausted(b)) => {
        log::warn!("budget {} exceeded: {} > {}", b.kind, b.consumed, b.limit);
    }
    Err(e) => return Err(e),
}
```

On the streaming path, the budget is consulted between every
text-delta. Once it's exhausted, the stream short-circuits with
`LlmError::BudgetExhausted` and no further deltas are emitted —
agent loops watching the budget tile can drop a runaway stream
without waiting for the upstream to time out.

## Error model

All four providers surface errors through one enum:

| Variant | Trigger | Notes |
|---|---|---|
| `LlmError::Auth(String)` | 401/403 | The API key is missing or invalid. Never echoes the key in the message. |
| `LlmError::RateLimit(RateLimitError)` | 429 | Carries `retry_after_secs` when the upstream sent a `Retry-After` header. |
| `LlmError::BudgetExhausted(BudgetExhausted)` | The caller's `TokenBudget` or `DollarBudget` tripped. | `kind` is `"tokens"` or `"dollars"`. |
| `LlmError::Provider { status, body }` | Any other non-2xx from the upstream. | Includes the upstream's error body (truncated). |
| `LlmError::Transport(String)` | TCP / TLS / IO failure. | Surface is intentionally untyped. |
| `LlmError::Decode(String)` | 2xx response we couldn't parse. | Usually means the provider rolled out a new schema. |
| `LlmError::UnknownModel(String)` | The model name isn't in the endpoint map. | Distinct variant so callers can `?` it without catching real transport failures. |
| `LlmError::NotImplemented(&'static str)` | A v0.26 skeleton surface (e.g. OpenAI streaming) was hit. | Resolves in v0.27. |

## Auth (env-var roster)

| Provider | Env var | Notes |
|---|---|---|
| Anthropic | `ANTHROPIC_API_KEY` | Sent as `x-api-key` header. |
| OpenAI | `OPENAI_API_KEY` | Sent as `Authorization: Bearer …`. |
| Gemini | `GEMINI_API_KEY` (fallback `GOOGLE_API_KEY`) | Sent as `?key=…` URL parameter. |
| Bedrock | `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` (+ optional `AWS_SESSION_TOKEN`), OR `AWS_BEDROCK_API_TOKEN`. `AWS_REGION` defaults to `us-east-1`. | SigV4 path is preferred; bearer-token is the fallback for short-lived API tokens. |

## Pricing table

`DollarBudget` consults a small built-in table for canonical models
(opus / sonnet / haiku, gpt-5 / gpt-4o / gpt-4o-mini, gemini-2.5-pro
/ gemini-2.5-flash). Unknown models fall back to a conservative
frontier-class rate so the budget over- rather than under-estimates.
Override with `DollarBudget::with_pricing(input_cents_per_million, output_cents_per_million)`.

See `dev/history/notes/STD_LLM_V0_26_NOTES.md` for design rationale,
forward-compat strategy, and the v0.27 follow-up backlog.
