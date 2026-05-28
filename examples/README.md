# Mighty Examples

Thirty-six canonical one-file examples — one per language feature
or stdlib surface — that compile cleanly under `mty check` on
every commit. The examples are the **canonical reference** for
"what does this surface look like in real source"; the
[tour](../docs/tour/README.md) walks them in reading order, the
[spec](../docs/spec/v1.0-rc.md) tells you what they normatively
mean.

Every example is exercised by the per-PR CI: `cargo test
--workspace` runs every `*.mty` file through `mty check` + `mty
fmt --check` and asserts a clean exit. The CI sweep gates the
`example sweep green` row — when a release lands, all 36 examples
have to compile.

## Run every example

```bash
for f in examples/*.mty; do mty check "$f"; done
```

PowerShell:

```powershell
Get-ChildItem examples/*.mty | ForEach-Object { mty check $_.FullName }
```

Every example should print `ok: <path>`. The two `@compile-error`
examples (where a deliberate-error marker is present) instead
expect a specific MT-code; those are skipped by the default sweep
and exercised by the example-error CI lane.

## Basics

| # | File | Teaches |
|---|------|---------|
| 01 | [`01_hello.mty`](01_hello.mty) | `fn main` + `log()` — the smallest program. |
| 02 | [`02_struct_enum.mty`](02_struct_enum.mty) | `struct` / `enum` / type aliases + pattern matching. |
| 03 | [`03_generic_fn.mty`](03_generic_fn.mty) | Generic functions with monomorphisation; `Option[&T]`. |
| 04 | [`04_result_propagation.mty`](04_result_propagation.mty) | `T!E` Result sugar + `?` propagation. |
| 05 | [`05_match_expr.mty`](05_match_expr.mty) | `match` expressions; pattern exhaustiveness. |
| 06 | [`06_for_while_loop.mty`](06_for_while_loop.mty) | `for` / `while` / `loop`; iterator protocol. |

## Agents + protocols

| # | File | Teaches |
|---|------|---------|
| 07 | [`07_agent_echo.mty`](07_agent_echo.mty) | The smallest agent: a `protocol` + an `agent` that implements it. |
| 08 | [`08_agent_state.mty`](08_agent_state.mty) | Stateful agents; per-handler state mutation. |
| 09 | [`09_send_ask_deadline.mty`](09_send_ask_deadline.mty) | `!Msg` send vs `?Msg @deadline` ask. |
| 10 | [`10_supervisor.mty`](10_supervisor.mty) | `supervisor` declaration; restart strategies. |
| 25 | [`25_agent_array.mty`](25_agent_array.mty) | Agent fields with fixed-size array types (`[U32; 200]`). |
| 28 | [`28_agent_with_llm_field.mty`](28_agent_with_llm_field.mty) | Opaque-ADT constructors inside handler scope (v0.27 Track B). |

## Capabilities + sandboxes

| # | File | Teaches |
|---|------|---------|
| 11 | [`11_budget_block.mty`](11_budget_block.mty) | `budget` block — bounded CPU / wall / mem / mailbox. |
| 12 | [`12_arena.mty`](12_arena.mty) | Arena allocation; scoped lifetimes. |
| 13 | [`13_capabilities.mty`](13_capabilities.mty) | Capabilities as parameters (`fs: Fs`, `net: Net`). |
| 17 | [`17_unsafe.mty`](17_unsafe.mty) | `unsafe` blocks; audit metadata. |
| 18 | [`18_sandbox.mty`](18_sandbox.mty) | Top-level `sandbox` block; spec §16.1. |

## Externs + interop

| # | File | Teaches |
|---|------|---------|
| 14 | [`14_extern_c.mty`](14_extern_c.mty) | C ABI extern declarations. |
| 15 | [`15_extern_js.mty`](15_extern_js.mty) | JS ABI extern declarations (wasm32-web). |
| 16 | [`16_macro.mty`](16_macro.mty) | Hygienic macros (v0.8); set-of-scopes resolution. |
| 21 | [`21_wasi_preview2.mty`](21_wasi_preview2.mty) | WASI Preview 2 + user-WIT (v0.13). |

## Effects + rows (RFC-008)

| # | File | Teaches |
|---|------|---------|
| 22 | [`22_effect_row.mty`](22_effect_row.mty) | Surface syntax for effect rows (`!{a, b | E}`). |
| 23 | [`23_multi_row.mty`](23_multi_row.mty) | Multi row-variable fn signatures (v0.17). |
| 24 | [`24_multi_row_full.mty`](24_multi_row_full.mty) | Full multi-row-var typecheck (v0.19). |

## Web + frontend

| # | File | Teaches |
|---|------|---------|
| 19 | [`19_backend_service.mty`](19_backend_service.mty) | Complete HTTP backend service (companion to demo 01). |
| 20 | [`20_frontend_component.mty`](20_frontend_component.mty) | wasm32-web frontend component (companion to demo 02). |

## Stdlib foundations

| # | File | Teaches |
|---|------|---------|
| 26 | [`26_string_vec.mty`](26_string_vec.mty) | `std.String` + `std.Vec[T]` (v0.25 Track E). |

## LLM agent stdlib

| # | File | Teaches |
|---|------|---------|
| 27 | [`27_tool_attr.mty`](27_tool_attr.mty) | `@tool(...)` decorator (v0.27 Track A); macro-expands into descriptor + invoker + register companions. |
| 29 | [`29_streaming.mty`](29_streaming.mty) | Source-level streaming with `MessageStream` (v0.27 Track E). |
| 30 | [`30_stream_consume.mty`](30_stream_consume.mty) | `while let Some(d) = stream.next()` (v0.29 Track D). |

## Eval + deterministic replay

| # | File | Teaches |
|---|------|---------|
| 31 | [`31_eval_agent.mty`](31_eval_agent.mty) | `std.eval` regression harness on top of byte-identical replay (v0.28 Track G). |
| 32 | [`32_eval_native.mty`](32_eval_native.mty) | Native replay backend — `Replay::with_provider` + `iter_llm_calls` (v0.29 Track F). |

## Taint types

| # | File | Teaches |
|---|------|---------|
| 33 | [`33_taint_basics.mty`](33_taint_basics.mty) | `Tainted[T]` — compiler-checked prompt-injection prevention (v0.30 Track A). |
| 34 | [`34_taint_untaint.mty`](34_taint_untaint.mty) | Untainting via the three approved exits (`Untaint::after`, sanitiser fn, `@taint_transparent`). |

## Observability

| # | File | Teaches |
|---|------|---------|
| 35 | [`35_observability_demo.mty`](35_observability_demo.mty) | `std.observe` + `mty inspect --cost` (v0.30 Track D); local SQLite-backed LLM-call store. |

## Computer use

| # | File | Teaches |
|---|------|---------|
| 36 | [`36_computer_use.mty`](36_computer_use.mty) | Anthropic Computer Use as a capability with typed sandbox bounds (v0.30 Track C). |

## Reading order

For first-time readers, the [tour](../docs/tour/README.md) walks
examples 01–20 in pedagogical order with a paragraph of context on
each. After chapter 15, the tour ends and the rest of the examples
(16–36) live here as a reference grouped by topic.

For LLM-agent readers (the v0.26+ stack), start at example 27
(`@tool`) and read forward through 36 (`@computer_use`) — those
ten examples are the differentiator surface.

## See also

- [`demos/README.md`](../demos/README.md) — nine end-to-end apps
  exercising the same surfaces composed into real programs.
- [`bench/swe/README.md`](../bench/swe/README.md) — SWE-bench
  Verified harness using the LLM-agent stack.
- [`docs/tour/README.md`](../docs/tour/README.md) — pedagogical
  walk-through of examples 01–20.
- [`docs/reference/README.md`](../docs/reference/README.md) — CLI
  + manifest + stdlib reference.
