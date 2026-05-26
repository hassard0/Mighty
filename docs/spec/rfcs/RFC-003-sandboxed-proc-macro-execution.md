# RFC-003 — Sandboxed proc-macro execution

**Status:** Draft (v0.9 spec-freeze prep).
**Tracks amendment:** A94 (procedural macros parse + store + purity
check, MT6006 gates every call site in v1.0).
**Target release:** v1.1.
**Owner:** *unassigned* — design owner needed before promotion.

## Implementation Status

**NOT YET SHIPPED.** Forward-looking RFC. The v1.0 contract — parse
+ store + purity check, MT6005 / MT6006 active emit at every call
site — is still in force.

Adjacent v0.13..v0.23 work that lowers but does not close this RFC's
backlog:

* The sandbox-budget constants — `PROC_MACRO_WALL_MS`,
  `PROC_MACRO_MEM_BYTES`, `PROC_MACRO_STEPS` — already live in
  `sdust_macros::proc` so the spec and implementation cannot drift
  when the sandbox executor lands.
* The deterministic-replay infrastructure (v0.17 → v0.19) gives the
  v1.1 sandbox executor a ready-made budget-enforcement substrate;
  reusing it would shrink the RFC's implementation work.

Diagnostic codes MT6005 (impurity) and MT6006 (call-site gate)
remain the only catch in v1.0. The window opened 2026-05-26
(close 2026-06-25) is **substantive**.

Cross-references:

* [`v1.0-rc.md`](../v1.0-rc.md) §20.3 — proc macros v1.0 contract.
* [`RFC_DASHBOARD.md`](RFC_DASHBOARD.md) — live window status.

## Summary

Replace v1.0's `MT6006 proc_macro_unsupported_v0_5` call-site gate
with a real **sandboxed proc-macro execution surface**. Proc macros
declared as `proc macro Name(input: TokenStream) -> TokenStream { ... }`
execute inside a child MtyIR interpreter at compile time, with
deterministic budget enforcement (CPU steps, wall, memory) and a
restricted capability set (no fs, no net, no clock — only token-stream
manipulation primitives).

Include a formal **TokenStream marshalling protocol** that lets a
proc macro see (and reshape) opaque token trees from the call site
without breaking the v0.5 hygiene model (A92).

## Motivation

The v1.0 contract preserves source survival (declarations parse and
store; call sites are replaced by sentinel literal `0`) so post-v1.0
work can lift the gate without re-parsing every Mighty source. But
this means **no proc macro actually runs in v1.0**. Two consequences:

1. **Standard macros that need expression-level reshaping cannot exist.**
   `format!("..., {x}, ...")` style works fine via the v0.5 declarative
   surface, but `derive(Json)` (which inspects a struct's fields and
   synthesises an impl block) requires proc-macro execution. v1.0
   workaround: hand-write the impl. v1.1+: proc macro derives it.
2. **External proc-macro packages can't be authored.** The macro
   ecosystem outside the stdlib is gated; the v0.5 macro library is
   the only proc-macro consumer in v1.0.

A sandboxed execution surface unblocks both.

## Detailed design

### Execution model

A proc macro runs in a **sub-interpreter** — a fresh `Interp` instance
spawned by the macro expander layer, distinct from the runtime
interpreter that executes the host program. The sub-interpreter:

- Loads the proc macro's MtyIR (compiled by the same compiler that's
  expanding the call site — bootstrapping is handled per "Adoption
  plan" below).
- Runs with a **deterministic** budget tracker pre-loaded with
  `PROC_MACRO_WALL_MS = 5000`, `PROC_MACRO_MEM_BYTES = 64 MiB`,
  `PROC_MACRO_STEPS = 10_000_000`. (Constants already reserved as
  `sdust_macros::proc::PROC_MACRO_*` since v0.5.)
- Sees **no `Host` capability methods** beyond the token-stream
  manipulation built-ins (see below). Effect inference's
  `tolerance_open=false` for the proc-macro body's strict-scope mode.

Budget breach emits `MT6010 proc_macro_budget_exceeded` at the call
site (with the breached dimension named) and replaces the expansion
with the same `0` sentinel as v1.0, preserving source survival.

### Sandbox capability model

The proc-macro sandbox is a stricter cousin of `sandbox Name with
{...} { body }` (A43):

- **No `fs.*`, `net.*`, `clock.*`, `model.*`, `rand.*`.** Calls to
  these emit `MT6005 proc_macro_impure` at decl time (existing v1.0
  check, retained). At execution time, attempting any of them
  triggers `MT5015 sandbox_violation` from the budget tracker.
- **No agent spawn / send / ask.** The sub-interpreter has no
  scheduler; `HirExpr::Spawn` / `Send` / `Ask` traps with
  `MT6011 proc_macro_concurrency`.
- **No FFI.** `extern { fn ... }` resolves to `MT8005` at the call
  site inside a proc-macro body.
- **Pure token-stream manipulation.** A new built-in module
  `mighty.macros.tokens` exposes:
  - `TokenStream` — opaque ADT (Sendable, Copy via interning).
  - `tokens.parse(s: Str) -> TokenStream` — lex a string into tokens
    (no parser; just the lexer).
  - `tokens.concat(a, b)` — append.
  - `tokens.split(s, sep) -> Vec[TokenStream]` — split at top-level
    `sep` token.
  - `tokens.iter(s) -> Iter[Token]` — token-level iteration.
  - `tokens.span_of(s) -> Span` — propagate hygiene info (see below).

### TokenStream marshalling

When the macro expander reaches a call site `MyDerive!(input)`:

1. **Encode.** The opaque TOKEN_TREE child of the MACRO_CALL CST node
   is serialised as a flat `Vec<MarshalledToken>` (kind + text + span)
   and pushed onto the sub-interpreter's stack as the `input` arg.
2. **Execute.** `Interp::run_fn(proc_macro_fn, [input])` runs until
   budget breach or return.
3. **Decode.** The returned `TokenStream` value is decoded back to a
   `Vec<MarshalledToken>` and re-parsed as a CST subtree at the call
   site.
4. **Hygiene join.** Each emitted token carries a Span that's either
   `Span::Call` (use the call-site hygiene context) or `Span::Def`
   (use the proc-macro decl-site hygiene context). The v0.5 extended
   mangling pass (A92) joins these correctly. **No new hygiene model
   is required** — this RFC inherits A92's lexical mangling.

### Determinism

A proc macro is a pure function of its input TokenStream. Given the
same input, two compilations of the same project on different hosts
MUST produce byte-identical output. Enforcement:

- The sub-interpreter inherits deterministic-mode (A39): single-
  threaded current-thread tokio, seeded XorShift RNG (seed 0 by
  default; configurable via `mighty.toml [build].proc_macro_seed`),
  logical clock.
- All `tokens.*` built-ins are deterministic (e.g. `tokens.split`
  preserves order; `tokens.iter` uses a stable iterator).
- The `MarshalledToken` encoding is canonicalised: no floating-point
  payloads, no `HashMap` iteration ordered by hash, no system-clock
  reads.

A new conformance category `tests/conformance/macros/proc/determinism/`
exercises 10 canonical proc macros under two seeds and verifies
identical output.

### Caching

Proc-macro expansion is a build-time cost; cache by hashing
`(macro_id, marshalled_input)`. Cache lives in
`target/proc-macro-cache/`; invalidated whenever the macro's MtyIR
hash changes. Off by default in v1.1 (correctness first); on by
default v1.2.

## Drawbacks

- **Bootstrapping complexity.** The proc-macro sub-interpreter
  executes MtyIR compiled by the same compiler that's expanding the
  call site. A circular dependency is avoided because v0.6+ self-host
  parser proves the lower layer is stable; the compile order is
  (proc-macro crate) → (consumer crate).
- **Budget tuning.** The 5s wall / 64 MiB / 10M-step defaults are
  educated guesses. Real ecosystems often need 30s+ for complex
  derives. Override via `mighty.toml [build] proc_macro_wall_ms = N`.
- **Sandbox bypass via FFI.** None — the sandbox forbids FFI
  altogether. But a malicious proc macro could write to its own
  Vec / String, then arrange for the call site to read it via the
  returned TokenStream. The hygiene check should prevent this from
  leaking secrets; explicit threat-model review needed during
  v1.1-alpha.

## Alternatives considered

1. **Run proc macros in WebAssembly.** Better isolation but much
   slower (interpreter is already deterministic and bounded; Wasm
   adds JIT-warmup cost). Reconsider when v1.1+ ships a real Wasm
   AOT path.
2. **Run proc macros as separate processes (rust-style host).**
   Higher overhead (process spawn per macro), harder to determinise.
   Rejected.
3. **Restrict proc macros to `derive` only.** Simpler but defeats the
   ecosystem motivation; existing libraries want function-like macros
   too.
4. **Compile-time evaluation via const-fn (skip TokenStream).**
   Insufficiently expressive for derive-style macros.

## Unresolved questions

- Should `tokens.parse(s)` accept arbitrary Mighty source or be
  restricted to a smaller "token grammar"? Restricting is safer but
  surprises users.
- How are proc-macro errors surfaced? RFC-003 reserves
  `MT6020 proc_macro_user_error` with the macro's own message
  payload, but does not specify error structure beyond `(span, msg)`.
- Cross-crate macro caching: the cache key needs the importer crate's
  edition + feature set for soundness. Sketch only; design in alpha.
- IDE integration: should the LSP run proc macros for inlay hints?
  Probably yes for top-of-file derives, no for in-body expansions
  (latency).

## Adoption plan

1. **v1.1-alpha.1:** sub-interpreter ships behind
   `--experimental-proc-macro` flag; `MT6006` continues to fire
   without the flag. Built-in `mighty.macros.tokens` module.
2. **v1.1-alpha.2:** TokenStream marshalling protocol stabilised;
   v0.5 stdlib `assert.mty` ports to proc macros as the first
   migration.
3. **v1.1-beta:** flag flips; `MT6006` retired; A94 reclassified
   FROZEN.
4. **v1.1.0:** caching opt-in via `mighty.toml`.
5. **v1.2:** caching opt-out (default on); LSP inlay-hint integration
   under a workspace-level toggle.

A 30-day public comment window opens with v1.1-alpha.1, extended to
60 days if any v0.x ecosystem package surfaces a blocker.
