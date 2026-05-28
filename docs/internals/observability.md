# `std.observe` — LLM cost + latency observability

**v0.30 Track D.** Production agent development is mostly "why is
this slow / expensive". Mighty owns that workflow with `std.observe`:
auto-record every `Member.ask(...)` / `LlmProvider::complete(...)`
under `MTY_OBSERVE=1`, read it back with `mty inspect --cost`.

## TL;DR

```sh
# 1. Run anything that hits an LLM under the observe env flag.
MTY_OBSERVE=1 mty run examples/28_agent_with_llm_field.mty

# 2. Read it back — total $$, per-provider breakdown, p50/p95/p99.
mty inspect --cost

# 3. Flex the window + grouping.
mty inspect --cost --since 7d --by model --top 5

# 4. Override pricing for a contracted rate.
MTY_PRICING_OVERRIDE=./pricing.toml mty inspect --cost
```

## Environment variables

| Variable | Default | Effect |
| --- | --- | --- |
| `MTY_OBSERVE` | unset | When `=1`/`on`/`true`, every `complete()` records to the active store. |
| `MTY_OBSERVE_DB` | `~/.mty/observations.sqlite` | SQLite path the recorder writes to + `mty inspect --cost` reads from. |
| `MTY_OBSERVE_OTEL` | unset | When set to an OTLP/HTTP base URL, recording redirects to the OTel exporter (Phase 2 stub in v0.30). |
| `MTY_PRICING_OVERRIDE` | unset | TOML file that overlays the baked-in cost table. |

`MTY_OBSERVE=0` / `=false` / `=off` is treated as off, same as unset.

## SQLite schema

```sql
CREATE TABLE observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    cost_cents INTEGER NOT NULL,         -- integer cents; never float
    latency_ms INTEGER NOT NULL,
    started_at TEXT NOT NULL,            -- ISO-8601 UTC, second precision
    started_at_ms INTEGER NOT NULL,      -- unix ms; what queries filter on
    agent_id INTEGER,                    -- NULL for top-level calls
    error_kind TEXT,                     -- NULL on success
    tool_calls TEXT NOT NULL DEFAULT '[]',  -- JSON [{name, latency_ms, failed}]
    recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_obs_started ON observations(started_at_ms);
CREATE INDEX idx_obs_provider ON observations(provider);
CREATE INDEX idx_obs_model ON observations(model);
```

`cost_cents` is `INTEGER` (mapped to Rust `i64`) so summing across a
million rows doesn't accumulate float drift. The cost math itself
multiplies first / divides last for the same reason.

## Pricing table (2026-05 snapshot)

Source: each provider's published pricing page on 2026-05-27.

| Model prefix | Input ($/Mtok) | Output ($/Mtok) | Source |
| --- | --- | --- | --- |
| `claude-opus` | 15 | 75 | claude.com/pricing |
| `claude-sonnet` | 3 | 15 | claude.com/pricing |
| `claude-haiku` | 1 | 5 | claude.com/pricing |
| `anthropic.claude-opus` (Bedrock) | 15 | 75 | same as upstream |
| `anthropic.claude-sonnet` (Bedrock) | 3 | 15 | same as upstream |
| `anthropic.claude-haiku` (Bedrock) | 1 | 5 | same as upstream |
| `gpt-5` | 5 | 15 | **placeholder — TBD by OpenAI** |
| `gpt-4.1` | 2 | 8 | platform.openai.com/pricing |
| `gpt-4o` | 2.50 | 10 | platform.openai.com/pricing |
| `gpt-4o-mini` | 0.15 | 0.60 | platform.openai.com/pricing |
| `gemini-2.5-pro` | 1.25 | 5 | ai.google.dev/pricing |
| `gemini-2.5-flash` | 0.075 | 0.30 | ai.google.dev/pricing |
| (unknown) | 15 | 75 | conservative frontier-class fallback |

The match is **longest-prefix-wins**, so `gpt-4o-mini` resolves to its
own row before `gpt-4o`. Unknown models fall back to the
conservative `(1500, 7500)` cents/Mtok rate — the dashboard
*over-* rather than *under-*estimates so a forgotten model still
gets flagged.

> Pricing is a snapshot and drifts. Pass `MTY_PRICING_OVERRIDE` to
> pin a contracted rate.

## `MTY_PRICING_OVERRIDE` file format

A simple TOML schema, parsed inline by `mty-stdlib` (no full `toml`
crate dep). Each `[[model]]` block overlays one prefix; matching
prefixes replace the baked-in row, unknown prefixes are added.

```toml
# pricing.toml

[[model]]
prefix = "claude-opus"
input_cents_per_million = 1000
output_cents_per_million = 5000

[[model]]
prefix = "gpt-5"
input_cents_per_million = 200
output_cents_per_million = 800

[[model]]
prefix = "internal-model-v9"
input_cents_per_million = 50
output_cents_per_million = 100
```

Comments (`# ...`) and blank lines are allowed. Unknown keys are a
parse error so typos can't silently produce wrong numbers.

## `mty inspect --cost`

```text
USAGE:
  mty inspect --cost [--since <DURATION>] [--by <KEY>] [--top <N>]
                     [--db <PATH>] [--json]

FLAGS:
  --cost            Switch from agent-snapshot mode to LLM-cost mode.
  --since <D>       Window: 500ms / 30s / 15m / 6h / 7d / all (default 24h).
  --by <KEY>        Group by provider / model / agent / none (default provider).
  --top <N>         Print the N most-expensive single calls (default 0).
  --db <PATH>       SQLite path (overrides MTY_OBSERVE_DB).
  --json            Emit JSON instead of the ASCII table.
```

Sample output:

```text
=== mty inspect --cost ("/home/me/.mty/observations.sqlite") ===
  window:           last 24h
  calls:            42
  total cost:       $4.71
  prompt tokens:    312_000 | completion tokens: 18_400
  latency p50/p95/p99: 250ms / 1820ms / 3100ms

--- by group ---
  key                              calls         cost     prompt-tok      compl-tok    p50/p95/p99
  anthropic                           28        $4.35        280_000         16_000  220/1900/3100ms
  openai                              10        $0.31         28_000          2_000  280/800/1100ms
  gemini                               4        $0.05          4_000            400  340/450/520ms

--- top 5 most expensive calls ---
     #  provider   model                              cost     tokens       ms  error
     1  anthropic  claude-opus-4-7                   $1.10     12_400     3100  -
     2  anthropic  claude-opus-4-7                   $0.95      8_300     2410  -
     3  anthropic  claude-opus-4-7                   $0.80      6_900     2100  rate_limit
     4  openai     gpt-5                             $0.18      1_200      900  -
     5  anthropic  claude-sonnet-4-6                 $0.15      2_400      450  -
```

Empty DB renders a helpful empty-state instead of an error so first-run
ergonomics stay clean.

## Manual instrumentation — `std.observe.span`

For non-LLM hot paths (tool dispatch, vector lookups, etc.) the
module exposes a span guard that records elapsed time into the same
DB:

```rust
let _guard = std::observe::span("vector_lookup");
let hits = vector_index.query(&q)?;
// Drop records `(provider="_span", model="vector_lookup",
// latency_ms=elapsed)` so `mty inspect --cost --by model` rolls it up
// alongside the LLM calls.
```

`provider="_span"` is the namespace so the LLM-cost rollups
(`--by provider` defaults) don't muddle their dollar totals with
zero-cost spans. Use `--by model` to see the span breakdown.

## OTel exporter (Phase 2 / v0.31)

`MTY_OBSERVE_OTEL=http://otel-collector:4318` swaps the SQLite store
for an [OTLP/HTTP][otlp] span exporter. v0.30 ships this as a
**documented stub** with one round-trip test
(`observe::otel::tests::otel_stub_buffers_records`); the real
exporter lands in v0.31. The wire format follows the
[OpenLLMetry conventions][ollm] so LangSmith / Arize / Honeycomb's
LLM panel light up out of the box:

- `span.kind = client`
- `span.name = "llm.{provider}.complete"`
- `gen_ai.system = provider`
- `gen_ai.request.model = model`
- `gen_ai.usage.input_tokens`
- `gen_ai.usage.output_tokens`
- `gen_ai.usage.cost` (dollars, float)
- `mty.agent_id`
- one `events[*]` per tool call

[otlp]: https://opentelemetry.io/docs/specs/otlp/
[ollm]: https://github.com/traceloop/openllmetry

## Failure semantics

**Observability code must never break the user's program.** The recorder:

- Returns silently when `MTY_OBSERVE` is off (one env lookup).
- Logs to stderr — never panics — when SQLite insert fails.
- Auto-creates the parent directory of `MTY_OBSERVE_DB` on first
  write (`~/.mty/` will not exist on a fresh machine).
- Bounds the OTel exporter's in-memory buffer at 1024 records to
  cap memory in long-running agents.
- Treats malformed `MTY_PRICING_OVERRIDE` as a warning + fall back
  to the baked-in table.

## v0.31 follow-ups

1. Real OTLP/HTTP exporter (not a buffered stub).
2. Per-`Member.ask` `agent_id` propagation from the runtime — v0.30
   leaves `agent_id` as `None` for the auto-record path because the
   provider trait has no agent-context hook; needs a `tokio::task_local!`
   or `CompletionRequest::agent_id` field.
3. Streaming-path coverage — the `complete_stream()` codepath gets
   the latency_ms but skips the usage record (token totals only land
   in the terminal `message_delta`).
4. Tool-call observation enrichment — record bytes-in / bytes-out
   per tool dispatch alongside `name` + `latency_ms`.
5. `mty inspect --cost --watch <MS>` polling mode mirroring the
   v0.16 inspect watch loop.
6. Per-tenant rollup for the SaaS multi-tenant case
   (`MTY_OBSERVE_TENANT_ID` + a `tenant` column).
