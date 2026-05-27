# `std.llm` — v0.26 Track A engineering notes

## Scope

Land a typed LLM provider abstraction so Mighty stops being "the
language with no LLM integration." After this slice:

- `mty_stdlib::llm::LlmProvider` is the single trait every backend
  implements.
- `AnthropicClient` is the **full** reference implementation —
  HTTP/1.1 over the workspace's existing `hyper` + `tokio-rustls`
  stack, SSE streaming, `tool_use` content blocks, typed budget
  short-circuiting.
- `OpenAiClient` / `GeminiClient` / `BedrockClient` ship as
  auth-only **skeletons**: each one shapes the request body
  correctly, routes to the right URL, and returns a stub
  `Message::assistant_text(format!("[<vendor> stub v0.26 — model=...]"))`
  from `complete()`. v0.27 wires the response-parsing + streaming
  bodies.

## Design decisions

### Why a single typed `Message`/`ContentBlock` shape

Every provider models conversation messages as a list of typed
content blocks, but the *names* drift:

- Anthropic: `text` / `tool_use` / `tool_result` / `image`.
- OpenAI Responses: `input_text` / `input_image` /
  `function_call_output` ...
- Gemini: `text` / `inlineData` / `functionCall` /
  `functionResponse`.
- Bedrock Converse: `text` / `image` / `toolUse` / `toolResult`.

Picking Anthropic's vocabulary as the typed surface keeps us
forward-compatible with the API that ships the strictest type
guarantees, and lets Track C's memory backends index messages
without per-provider conditionals. Translation to per-vendor wire
shape happens at the `LlmProvider` boundary (in each provider's
`build_body` / response-parser pair), not in user code.

### Why budgets are typed (not just env-variables)

Two recurring failure modes drove the typed `TokenBudget` +
`DollarBudget`:

1. **Streaming loops** that don't notice the tokens-per-second draw
   until the user's invoice is 5x what was estimated.
2. **Tool-use chains** that fan out N concurrent provider calls
   against a single conceptual user request — each one within
   "budget," together over it.

Both fixes need a *ref-counted handle* the caller can hand down
through the agent graph so child calls deduct from the parent's
pool. Hence `Arc<Mutex<TokenInner>>` — sharing a budget is
`budget.clone()`, not "remember to pass the same `u64` around".

The streaming path consults the budget between every text-delta
(and deducts a `len()/4` token estimate per-delta). When the budget
trips, the stream short-circuits with `LlmError::BudgetExhausted`
and drops the rest of the upstream response — agent loops watching
the budget tile can cancel a runaway stream deterministically
without waiting for the upstream to time out.

### Why we hand-rolled the HTTPS connector instead of pulling `hyper-rustls`

The workspace already links `rustls` + `tokio-rustls` (via
`std.tls`). Wiring a small HTTPS connector inline (≈ 30 lines)
avoided a new top-level dep, kept the build-time small, and reuses
the `ensure_crypto_provider()` idempotent installer. Trade-off: we
don't get HTTP/2 multiplexing today — every call opens a fresh
HTTP/1.1 connection. That's fine for v0.26 because:

- Anthropic Messages requests are bursty (one chat turn at a time),
  not pooled streams.
- The connection-per-call overhead is dominated by the TLS
  handshake, which we'd amortise with a connection pool — but pools
  are a v0.27 concern once the four backends are all live.

If/when we need HTTP/2, swap to `hyper-rustls` + `hyper-util`'s
`Client<HttpsConnector, ...>` in one place (`send_request` in
`anthropic.rs`).

### Why SSE parsing is a pure function

`parse_anthropic_sse(&str) -> (Vec<MessageDelta>, String)` is a
pure free function. The async stream adapter in
`AnthropicClient::complete_stream` just feeds chunks of the response
body through it and yields the deltas. This shape made the
streaming tests trivial: feed a captured fixture, assert the
delta sequence, no async runtime, no wiremock. The streaming
fixtures (`tests/fixtures/llm_sse/*.sse`) are the source of truth
for what the parser accepts; new Anthropic event types get added
here first.

### Why skeleton providers return a stub `Message`

The alternative was `LlmError::NotImplemented` from
`OpenAiClient::complete()`. We rejected that because:

- Track B's `@tool` macro needs to register tools against *all*
  providers in v0.26 (the macro emits `schema_for_tool` calls into
  each registered client). Returning `NotImplemented` would force
  every macro-derived agent to gate on provider == anthropic.
- Track E's demos can show `client.complete()` succeeding with a
  visible "this is a v0.26 stub" message, which makes the v0.27
  migration story self-documenting (the stub text disappears when
  the real body lands).

The structural tests (build_body, endpoint routing, tool
serialisation) cover the skeleton providers; integration tests
against the real upstreams are deferred to v0.27 behind `#[ignore]`.

## Tests

- `crates/mty-stdlib/tests/llm_anthropic.rs` — 9 tests, full
  wiremock round-trip coverage:
  - `anthropic_complete_round_trips_with_wiremock`
  - `anthropic_tool_use_emits_tool_block`
  - `anthropic_rate_limit_returns_typed_error` (asserts
    `Retry-After` parse)
  - `anthropic_auth_error_surfaces_as_typed_auth_variant`
  - `anthropic_budget_exhausted_short_circuits_before_request`
  - `anthropic_complete_records_usage_into_token_budget`
  - `anthropic_provider_5xx_surfaces_as_provider_error`
  - `anthropic_streaming_parses_sse_chunks`
  - `anthropic_budget_exhausted_drops_stream`
- `crates/mty-stdlib/tests/llm_streaming.rs` — 5 tests against four
  captured SSE fixtures:
  - `text_only_fixture_concatenates_to_full_message`
  - `tool_use_fixture_stitches_input_json_into_one_payload`
  - `multi_paragraph_fixture_preserves_newlines_inside_deltas`
  - `unknown_event_types_are_dropped_without_breaking_parse`
  - `parser_handles_chunked_feed_by_carrying_tail_across_calls`
- 35 in-crate unit tests covering builders, error variants, budget
  arithmetic, default pricing fallback, tool/message serialisation,
  per-provider request shaping, and stub skeleton paths.

Total: **49 new tests**.

## Forward-compat backlog (v0.27)

- Real OpenAI Responses parse + SSE.
- Real Gemini `generateContent` parse + `streamGenerateContent` SSE.
- Bedrock SigV4 signing path behind `aws-sigv4`.
- HTTP/2 + connection pooling via `hyper-rustls`.
- Caching headers (Anthropic prompt-caching `cache_control`).
- Image-input content blocks (skeleton today; provider serialisers
  drop image blocks silently).
- `tool_choice: required` enforcement on streaming responses (the
  spec says the model *must* call a tool — surface a typed error if
  it doesn't).

## Track interplay

- **Track B (`@tool` macro)** consumes our `Tool` /
  `ToolUse` / `ToolResult` shapes verbatim. The macro emits
  `Tool::new(name, description, json_schema)` and registers it in
  the user's agent against any `LlmProvider`.
- **Track C (memory)** indexes `Message` + `ContentBlock` directly
  — vector backends embed the `.text()` of each message, episodic
  buffers store the raw block list.
- **Track D (codegen-wasm)** owns the wasm-side dispatch — no
  changes to our crate.
- **Track E (demos)** can build an end-to-end "ask Claude a
  question + cite sources from a memory backend" demo on top of
  what landed here.

## Files

NEW:

- `crates/mty-stdlib/src/llm/mod.rs`
- `crates/mty-stdlib/src/llm/message.rs`
- `crates/mty-stdlib/src/llm/tools.rs`
- `crates/mty-stdlib/src/llm/streaming.rs`
- `crates/mty-stdlib/src/llm/anthropic.rs`
- `crates/mty-stdlib/src/llm/openai.rs`
- `crates/mty-stdlib/src/llm/gemini.rs`
- `crates/mty-stdlib/src/llm/bedrock.rs`
- `crates/mty-stdlib/src/llm/error.rs`
- `crates/mty-stdlib/src/llm/budget.rs`
- `crates/mty-stdlib/src/llm/provider.rs`
- `crates/mty-stdlib/tests/llm_anthropic.rs`
- `crates/mty-stdlib/tests/llm_streaming.rs`
- `crates/mty-stdlib/tests/fixtures/llm_sse/*.sse` (4 fixtures)
- `docs/reference/stdlib/llm.md`
- `dev/history/notes/STD_LLM_V0_26_NOTES.md` (this file)

EXTENDED:

- `crates/mty-stdlib/src/lib.rs` (`pub mod llm;`)
- `crates/mty-stdlib/Cargo.toml` (`async-trait`, `async-stream`,
  `bytes`, `futures-core`, `futures-util`, `webpki-roots`,
  dev-dep `wiremock`)
- `crates/mty-types/src/prelude.rs` (registers `std.llm` as opaque
  module + 8 LLM permissive method names)
