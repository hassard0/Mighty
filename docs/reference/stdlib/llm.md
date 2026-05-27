# `std.llm`

Typed LLM provider abstraction. Shipped in v0.26 (Track A) — the
single biggest gap between Mighty and "the standard language for
agents". One trait, four backends.

| Provider | Status (v0.26) | Rust API |
|---|---|---|
| Anthropic Messages | **full** — HTTP + streaming + tool-use + budgets | [`AnthropicClient`](https://docs.rs/mty-stdlib/latest/mty_stdlib/llm/anthropic/) |
| OpenAI Responses | skeleton — auth + request shape | [`OpenAiClient`](https://docs.rs/mty-stdlib/latest/mty_stdlib/llm/openai/) |
| Google Gemini `generateContent` | skeleton | [`GeminiClient`](https://docs.rs/mty-stdlib/latest/mty_stdlib/llm/gemini/) |
| AWS Bedrock Converse | skeleton (bearer-token; SigV4 v0.27) | [`BedrockClient`](https://docs.rs/mty-stdlib/latest/mty_stdlib/llm/bedrock/) |

The three skeleton clients ship **auth, endpoint routing, and
request-body shaping**. Their `complete()` returns a stub
[`Message`](#message) so the trait surface compiles and downstream
tools (Track B's `@tool` macro, Track C's memory backends) can be
written against the typed shape today. The actual response-parsing
+ streaming-SSE-conversion bodies are tagged `TODO v0.27`.

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
| Bedrock | `AWS_BEDROCK_API_TOKEN` (region: `AWS_REGION`, default `us-east-1`) | v0.27 will add SigV4 with `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`. |

## Pricing table

`DollarBudget` consults a small built-in table for canonical models
(opus / sonnet / haiku, gpt-5 / gpt-4o / gpt-4o-mini, gemini-2.5-pro
/ gemini-2.5-flash). Unknown models fall back to a conservative
frontier-class rate so the budget over- rather than under-estimates.
Override with `DollarBudget::with_pricing(input_cents_per_million, output_cents_per_million)`.

See `dev/history/notes/STD_LLM_V0_26_NOTES.md` for design rationale,
forward-compat strategy, and the v0.27 follow-up backlog.
