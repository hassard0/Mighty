# Mighty v0.3 — Release Notes

**Tag:** `v0.3.0`
**Date:** 2026-05-25
**Status:** SHIPPED — third milestone release, soundness hardening
across the borrow checker, type/effect checker, and runtime; closes
the v0.2 cleanup backlog.

Mighty v0.1 walked the spec §31 ladder end-to-end (parser through
codegen). v0.2 lit up every surface the v0.1 deferral list named
(LSP, package manager, doc generator, stdlib, debug info, Wasm
Component Model). v0.3 turns the screws: the borrow checker grows
NLL last-use and field-level Places, the type checker grows
scope-aware tolerance and a formal Sendable trait, the runtime grows
cooperative mid-turn cancellation and OTLP telemetry, and the v0.2
loose ends close (stdlib install, 6/20 wasm-CM gaps, 3 of 5
INTENTIONALLY_IGNORED conformance cases).

## What you can do (new in v0.3)

```bash
# `mty run` now exercises real std.* semantics
mty run examples/01_hello.sd

# OTLP telemetry — point at any collector
STARDUST_OTLP_ENDPOINT=http://localhost:4317 mty run my-agent.sd

# 20/20 wasm-Component builds — examples with helpers + no top-level
# main now ship with --no-component-friendly _-prefixed helpers
mty build --target wasm32-wasi examples/14_extern_c.sd

# Borrow checker accepts disjoint field borrows
# (compiles where v0.2 over-rejected)
let r1 = &s.a
let r2 = &mut s.b
use_both(r1, r2)

# Strict agent body rejects unresolved names with MT2021
# (was permissive fresh-var fallback in v0.2)
agent Worker: Hi {
  on Greet(name) -> {
    unknown_helper(name)  # → MT2021 (was silent in v0.2)
  }
}
```

Everything from v0.2 still works the same way.

## The four swarm agents

v0.3 was built by 4 autonomous swarm agents working disjoint crate
boundaries, then integrated through this release:

| Agent | Crates / files | Commits |
|---|---|---|
| borrow | `mty-borrow`, borrow-model spec, conformance | `d888ef2`, `8b98669` |
| effect/cap | `mty-types`, Sendable, A65 spec | `e239b30` |
| runtime | `mty-runtime`, OTLP, slab pool, cancel | `6600f4c` |
| v0.2-cleanup | CLI bridge, 6 example mains, 3 ignored→passing | `955291d` |

Plus one prep commit (`7ff5629`) that added the OpenTelemetry +
tokio-util workspace deps before the runtime swarm began.

## Headline numbers

- **623 tests pass** (0 failures, 1 network-bound ignored) — was 550 in v0.2
- **+73 tests** added in v0.3
- **0 clippy warnings** with `-D warnings`
- **19 crates** in the workspace (unchanged — v0.3 hardened, didn't add)
- **5 commits** since `v0.2.0` (tight: each agent touched disjoint crates)
- **7,507 insertions / 238 deletions** across 133 files
- **20/20 examples compile to native** (unchanged from v0.2)
- **20/20 examples compile to bare wasm core modules** (unchanged)
- **20/20 examples compile as Wasm Components** (was 14/20 in v0.2)
- **32 conformance cases run** (was 25 in v0.2), 2 ignored (was 5)
- **11 new spec amendments** (A54..A56, A65/A65.b/A65.c/A65.d, A70..A73)
- **MSRV unchanged at 1.85**

## Soundness assertions newly enforced

| Property | v0.2 | v0.3 |
|---|---|---|
| Last-use deactivates borrow | whole-local lexical | per-Place NLL (A55) |
| Disjoint field borrows | conflated | disjoint via Place algebra (A54) |
| `move *ref` of non-Copy | partial | explicit MT3009 (A56) |
| Non-Sendable cross-agent arg | informal | formal Sendable + MT3011 (A65.b) |
| Unresolved name in agent body | silent fresh-var | strict MT2021 (A65) |
| Handler param vs protocol decl | trust-the-decl | post-check + MT4031 (A65.c) |
| Mid-turn cancellation | between-turns only | cooperative mid-turn (A70) |
| OTLP telemetry | JSON sidecar | real OTLP/gRPC (A71) |
| Mailbox frame allocation | per-message alloc | slab pool (A72) |
| Lambda body borrow leaks | possible | snapshot/restore prevents |

## Closed deferrals from v0.2

All four "loose ends" from v0.2 cleanup brief are addressed:

- **Stdlib host install** — fixed via CLI-side bridge
  (`sdust_runtime::host_std::install_dispatcher` in
  `mty-cli::main`); `mty run` now executes real `std.*` calls
- **6/20 wasm-CM build failures** — closed (all 20 examples wrap as
  Components; non-main top-level identifiers prefixed with `_`)
- **5 INTENTIONALLY_IGNORED conformance cases** — 3 closed; the
  remaining 2 (`capability_checking/03_narrow_to_ro`,
  `supervisor_restart/02_escalate`) need work in mty-types /
  mty-syntax that's outside v0.3 scope
- **LLVM backend untested** — install docs improved; smoke deferred
  (no LLVM 17 host available)

Plus three v0.1 amendments superseded:

- **A38** (telemetry JSON-only) → **A71** OTLP
- **A40** (mailbox defaults, no slab pool) → **A72** slab pool
- **A41** (cancellation between turns only) → **A70** mid-turn

## Spec amendments (11 new)

```
A54   — Field-level borrow tracking via Place algebra
A55   — NLL last-use deactivation
A56   — Precise MT3009 for `move *ref`
A65   — Scope-aware permissive/strict type-check policy
A65.b — Sendable trait at cross-agent message sites
A65.c — MT4031 strict handler param-type check, local-protocol only
A65.d — `core` profile rejects `alloc`
A70   — Cooperative mid-turn cancellation
A71   — OTLP wire-format telemetry
A72   — Slab-pool mailbox frames
A73   — Batched per-turn deadline scheduler
```

All committed to `docs/spec/v0.1-amendments.md`.

## Diagnostic codes

No new SD codes minted; existing codes were re-aimed:

- **MT2021** (unresolved_value) — text + scope-labeled note
- **MT3009** (move_out_of_ref) — newly precise for `*ref` of non-Copy
- **MT3011** (non_sendable_message_arg) — rule formalized
- **MT4031** (protocol_param_type_mismatch) — newly activated for
  local protocols
- **MT4041** (derive_unknown) — text lists `Sendable`

`mty explain SDxxxx` carries the updated text for each.

## Toolchain

- **MSRV: Rust 1.85** (unchanged from v0.2)
- New default-on `otlp` feature on `mty-runtime` (build with
  `--no-default-features` to strip the OpenTelemetry exporter for
  minimum-binary builds)
- All-platform: Windows, macOS, Linux
- Cargo workspace; no `build.rs` magic

## Deferred to v0.4 / post-v0.3

The full deferral catalogue (28 items) lives in `SLICE_V0_3.md`.
Highlights:

- **Borrow**: two-phase borrows, deeper field paths, index-aware
  disjointness, loop back-edge borrows, conditional-branch join
  refinement (Polonius-style), cross-fn region inference
- **Effects / caps**: real cap-name resolution wiring (slice-7),
  function-signature cap-narrowing, cross-package Sendable
  propagation, Sendable lambda capture analysis
- **Runtime**: MtyIR-side cancellation polling (true mid-turn
  interrupt), CpuBudget reason wiring, HTTP/protobuf OTLP transport
  selector, OTel resource-attribute env-vars, DelayScheduler as
  default per-turn timer
- **Carried from v0.2**: WASI Preview 2 + user-authored WIT, DWARF
  v5 + per-instruction line program, backtracking pkg resolver +
  tar/flate2 + real registry, `dyn` dispatch + closure capture in
  compiled code, real `loop { break }` lowering, LLVM backend smoke
  on Linux+LLVM 17

## Known issues

1. **2 conformance cases still `INTENTIONALLY_IGNORED`**:
   - `capability_checking/03_narrow_to_ro` — needs mty-types
     `Fs.ro` cap narrowing
   - `supervisor_restart/02_escalate` — needs mty-syntax
     `escalate` grammar
2. **Borrow checker conservatism**: see `BORROW_V0_3_NOTES.md` for
   six over-restriction patterns (two-phase, deeper fields, indices,
   loop back-edges, conditional joins, cross-fn regions). These are
   sound — they reject programs Rust would accept — and are tracked
   for v0.4.
3. **Supervisor / cap-narrow scopes mark strict but keep
   `tolerance_open=true`** until slice-7 wires real cap-name
   resolution. The ScopeKind framework is in place; flipping the
   toggle activates MT2021 automatically when names land.
4. **OTLP transport is gRPC-only** (`STARDUST_OTLP_PROTO` selector
   deferred). OTel resource-attribute env-vars not honoured yet.
5. **LLVM backend code paths shipped but not exercised on this
   build host** (carried from v0.2 — no LLVM 17 installed).

## Backwards compatibility

v0.3 is a minor-version bump from v0.2. Source compatibility is
preserved for slice 1-5 + v0.2 surfaces. **Notable behavior
changes**:

- **Strict scopes now hard-error unresolved names (MT2021).** Code
  that relied on the slice-3 A21 fresh-var fallback inside agent /
  handler / supervisor / cap-narrow bodies needs to either declare
  the missing name or move the reference into a permissive scope
  (top-level fn, extern block, unsafe / arena / budget block).
  Existing well-typed agent code is unaffected.
- **Cross-agent sends now check Sendable (MT3011).** Code that
  silently passed `&T` or `Cap` through a `!Msg` will now error.
  Convert to owned, derive(Sendable), or move via a handle.
- **`move *ref` of non-Copy now errors with MT3009** (was silently
  accepted in some shapes). Use `*ref.clone()` or unwrap the
  reference first.

Diagnostic codes (MT0001..MT8010) are unchanged in number; MT2021 /
MT3009 / MT3011 / MT4031 / MT4041 carry updated text. CLI shape is
unchanged.

## Acknowledgments

v0.3 is the second Mighty release built by autonomous parallel
agents. The four swarm agents shipped tightly because each touched
disjoint crates — borrow vs types vs runtime vs CLI/cleanup — and
the integrator only needed to apply two test-side cross-cuts (LSP
diagnostic test update for A65, new `all_examples_compile_wasm_component`
regression gate). The agents stood on the slice-1..8 + v0.2
foundations: the Cranelift / wasm backends, the diagnostic
infrastructure, the tower-lsp wiring, and the conformance harness
all carried forward without rewrites.

Big thanks to the `opentelemetry`, `tonic`, `tokio-util`, `dashmap`,
`gimli`, and `tower-lsp` teams — Mighty v0.3 stands on those
shoulders too.

## What's next

v0.4 picks up the 28-item deferral catalogue: Polonius-style borrow
checking, real cap-name resolution wiring, MtyIR-side cancellation,
WASI Preview 2, DWARF v5, backtracking pkg resolver. The
aspirational v0.4 tagline: *"every soundness assertion the spec
makes is enforceable end-to-end, and the compiler tells you why
when it isn't."*
