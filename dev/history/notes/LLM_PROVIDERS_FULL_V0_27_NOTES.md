# `std.llm` v0.27 — OpenAI / Gemini / Bedrock full implementations

v0.26 Track A shipped Anthropic full + three skeletons (OpenAI,
Gemini, Bedrock) that did auth + endpoint routing + request-body
shaping but returned a stub `Message` from `complete()`. v0.27 Track
C promotes all three to full implementations with the same surface
as Anthropic: HTTP/1.1 + streaming + tool-use + structured outputs +
budget short-circuit.

## What shipped

### OpenAI Responses API (`crates/mty-stdlib/src/llm/openai.rs`)

- `POST /v1/responses` with `Authorization: Bearer $OPENAI_API_KEY`.
- Request body: `{model, input, tools, tool_choice, temperature,
  max_output_tokens, stream}`. Messages serialise to "input items"
  with `role` + typed `content` parts (`input_text` for user,
  `output_text` for assistant). System prompt becomes a
  `developer`-role item.
- Tool-use: function-call output items carry `call_id`, `name`,
  `arguments` (a JSON-encoded string). We parse the arguments back
  into `serde_json::Value` and surface as `ContentBlock::ToolUse`.
- Streaming: SSE events `response.output_text.delta`,
  `response.output_item.added` (for function-call lifecycle),
  `response.function_call_arguments.delta`, and `response.completed`.
- Older `response.tool_call.delta` shape also handled as a tolerance
  knob so captured fixtures from earlier SDK snapshots still parse.

### Google Gemini (`crates/mty-stdlib/src/llm/gemini.rs`)

- `POST /v1beta/models/<model>:generateContent?key=$GEMINI_API_KEY`.
- Streaming endpoint: `:streamGenerateContent?alt=sse`. The default
  streaming endpoint emits a JSON array that flushes per element
  (awkward to parse incrementally); `?alt=sse` gives proper SSE with
  one `GenerateContentResponse` JSON per `data:` line.
- Role mapping: `assistant` → `model`. `system` is hoisted out of
  the messages list into a top-level `systemInstruction` field.
- Tools: nested under `tools[0].functionDeclarations[]`. `ToolChoice`
  maps to `toolConfig.functionCallingConfig.mode = AUTO|ANY|NONE`;
  `ToolChoice::Tool{name}` uses `mode=ANY` + `allowedFunctionNames`.
- Safety settings: per-`GeminiClient` override via
  `with_safety_settings(json!([...]))`. Default is upstream's preset.
- Gemini doesn't issue IDs for function calls; we synthesise stable
  ids of the form `gem_<name>_<index>` so `ToolResult.tool_use_id`
  pairing still works.

### AWS Bedrock (`crates/mty-stdlib/src/llm/bedrock.rs`)

- `POST /model/<model_id>/converse` (one-shot) or `/converse-stream`
  (streaming). Region from `AWS_REGION` (default `us-east-1`).
- Authentication: SigV4 signing (preferred) OR `Authorization: Bearer`
  (Bedrock's newer short-lived token shape). The two modes are picked
  via the typed `BedrockAuth` enum; callers go through
  `with_credentials(AwsCredentials{...})` or `with_api_token(...)`.
- SigV4 implementation is **inline**, not from `aws-sigv4`. The
  algorithm is small: hash canonical request → string-to-sign →
  derive signing key via HMAC chain (date → region → service →
  "aws4_request") → final HMAC = signature. We pulled in `sha2` and
  `hex` (both already workspace deps) and hand-rolled HMAC-SHA256
  (RFC 2104) inline. This avoids the `aws-sigv4` + `aws-smithy-*`
  dep tree (which is large and pulls multiple AWS-specific runtimes).
- SigV4 implementation is validated against the RFC 4231 HMAC-SHA256
  test vector + an internal determinism test that re-signs the same
  request twice and asserts byte-identical output.
- ConverseStream uses AWS's binary event-stream framing (NOT SSE).
  Each frame: 4 bytes `total_len` + 4 bytes `headers_len` + 4 bytes
  `prelude_crc` + variable `headers` + variable `payload` + 4 bytes
  `message_crc`. Headers are a packed `(name_len, name, value_type,
  value)` sequence. We extract the `:event-type` string header
  (value-type `7`) and project the JSON payload into
  `MessageDelta` via the same `current_tool` stitching pattern used
  by Anthropic + OpenAI.
- Date-formatting for the SigV4 `x-amz-date` header is implemented
  via Howard Hinnant's days-from-civil algorithm (proleptic
  Gregorian) — no `chrono` / `time` dep needed.

## Test coverage

35 new tests across 4 files:

- `tests/llm_openai.rs` (8 tests) — round-trip, tool-use, 401/429,
  budget pre-check, budget mid-stream, SSE chunk parsing, fixture
  replay.
- `tests/llm_gemini.rs` (6 tests) — same shape but for Gemini's
  `candidates[].content.parts` wire shape.
- `tests/llm_bedrock.rs` (6 tests) — SigV4-signed wiremock
  round-trip, bearer-token round-trip, tool-use, 429, budget,
  binary-event-stream fixture replay.
- `tests/fixtures/llm_sse/{openai_text,openai_tools,gemini_text,gemini_tools,bedrock_text}.sse`
  — 5 new captured wire-shape fixtures.

Provider-internal unit tests (`#[cfg(test)] mod tests` blocks)
extended by another 10 (SSE parsers, SigV4 against RFC vectors,
event-stream parser, role mapping, tool-choice serialisation).

Anthropic baseline (`llm_anthropic.rs`, 9 tests) still passes — the
only change needed there was using the fully-qualified
`StreamExt::next(&mut stream)` to dodge the inherent `next()`
method that Track E (QoL gaps slice) added on `MessageStream`,
which shadows the trait method.

## Strategic decisions

- **No `aws-sigv4` dep.** Inline implementation is ~150 LoC; the
  alternative pulls in the entire `aws-smithy-*` ecosystem (runtime,
  http, types) which would balloon the workspace's dep tree by
  ~40-50 crates for one helper function.
- **No new HTTP abstraction.** Each provider has its own inline
  `hyper` + `tokio-rustls` client. The duplication is small (~150
  LoC per provider) and keeps every module self-contained. A shared
  helper module is a v0.28 refactor candidate once the API surfaces
  stabilise.
- **Bedrock event-stream CRC not validated.** TLS already covers
  end-to-end integrity; re-validating CRC-32 on every frame adds CPU
  overhead with no security benefit. Documented in-line.
- **Gemini tool-call IDs synthesised.** Gemini doesn't issue ids for
  function calls but the typed `ToolUse` shape carries one. We
  generate `gem_<name>_<index>` so the next user-turn's
  `ToolResult.tool_use_id` has something stable to bind against.

## Wire-shape compatibility table

See `docs/reference/stdlib/llm.md` for the comprehensive
provider-quirks table that documents how Anthropic / OpenAI / Gemini
/ Bedrock differ on endpoint, auth, role mapping, tool schema, and
streaming envelope. The typed `Message` / `ContentBlock` / `Tool`
shapes serialise into all four backends through provider-specific
adapters; callers never see the wire-format differences.

## What Track D + F can consume

- All four `*Client::from_env()` constructors now produce fully
  functional clients (no stub markers). Track D's `@tool` macro
  registers tools through `LlmProvider::schema_for_tool`, which
  routes to the correct per-provider serialiser.
- Track F's demos can now construct an OpenAI / Gemini / Bedrock
  client and get real completions — the v0.26 demo 07 was
  Anthropic-only because the three skeletons returned stub text.
- The trait surface (`LlmProvider`, `CompletionRequest`,
  `MessageStream`) is unchanged, so any consumer that wrote against
  v0.26's Anthropic now works against the other three providers via
  the same code path.

## Forward compat (v0.28+ candidates)

- Shared HTTP client helper across providers (current duplication
  is intentional but ripe for extraction once we have ~6 months of
  signal on what each provider's edge cases look like).
- Bedrock event-stream CRC validation (low priority; TLS covers it).
- Bedrock prompt-caching support — the Converse API gained explicit
  cache-control directives in 2024 but the typed `Message` shape
  doesn't yet expose them.
- Gemini's longer-running `:countTokens` endpoint as a separate
  client method (useful for pre-flight budgeting).
