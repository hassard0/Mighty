# Chapter 19 — Observability

Example:
[`35_observability_demo.mty`](https://github.com/hassard0/Mighty/blob/main/examples/35_observability_demo.mty).

Production agent development is mostly **"why is this slow /
expensive"**. Mighty owns that workflow with `std.observe`:
auto-record every LLM call under `MTY_OBSERVE=1`, read it back
with `mty inspect --cost`.

Shipped in v0.30 Track D.

## Two-line trial

```bash
# 1. Run anything that hits an LLM under the observe env flag.
MTY_OBSERVE=1 mty run examples/28_agent_with_llm_field.mty

# 2. Read it back — total $$, per-provider breakdown, p50/p95/p99.
mty inspect --cost
```

Sample output:

```
LLM cost summary (last 24h)
  total: $0.18  calls: 12
  by provider:
    anthropic:  $0.14   8 calls  p50= 1.8s  p95= 4.2s  p99= 5.1s
    openai:     $0.04   4 calls  p50= 1.1s  p95= 2.3s  p99= 2.4s
```

## Environment variables

| Variable | Default | Effect |
|---|---|---|
| `MTY_OBSERVE` | unset | When `=1` / `on` / `true`, every `complete()` records to the active store. |
| `MTY_OBSERVE_DB` | `~/.mty/observations.sqlite` | SQLite path the recorder writes to and `mty inspect --cost` reads from. |
| `MTY_OBSERVE_OTEL` | unset | When set to an OTLP/HTTP base URL, recording redirects to the OTel exporter. |
| `MTY_PRICING_OVERRIDE` | unset | Path to a TOML pricing override (your contracted rate, not the public rate card). |

## What gets recorded

Per LLM call, the recorder captures:

- Wall-clock timestamp (UTC).
- Provider, model, request ID.
- Input + output token counts.
- Cost in cents (derived from a built-in pricing table or
  `MTY_PRICING_OVERRIDE`).
- End-to-end latency in milliseconds.
- The active span / trace ID (if `MTY_OBSERVE_OTEL` is set).
- Any `Untaint::after` justifications fired on the response.

## Reading it back

```bash
# Default — last 24h, grouped by provider.
mty inspect --cost

# Different window + grouping.
mty inspect --cost --since 7d --by model --top 5

# Drill into a single agent.
mty inspect --cost --by-agent reviewer

# JSON output for piping.
mty inspect --cost --since 24h --json
```

See [`reference/cli/mty-inspect.md`](../reference/cli/mty-inspect.md)
for the full flag list.

## Try it

```bash
mty check examples/35_observability_demo.mty
MTY_OBSERVE=1 mty run examples/35_observability_demo.mty
mty inspect --cost
```

The third command should show the demo's recorded mock call.

## See also

- [`docs/internals/observability.md`](../internals/observability.md) —
  full implementation: SQLite schema, recorder hot-path, OTel
  export, pricing tables.
- The SWE-bench harness uses `std.observe` to populate the
  per-instance cost column in
  [`dev/history/benchmarks/swe-bench-smoke-v0.30.md`](https://github.com/hassard0/Mighty/blob/main/dev/history/benchmarks/swe-bench-smoke-v0.30.md).
