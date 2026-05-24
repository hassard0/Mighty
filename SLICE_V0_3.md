# Mighty v0.3 — Complete

**Tag:** `v0.3.0`
**Date:** 2026-05-25
**Status:** SHIPPED — third milestone release, soundness hardening across
the borrow checker, type/effect checker, runtime, and the v0.2 cleanup
backlog.

v0.3 was built by a four-agent autonomous swarm (borrow / effect-cap /
runtime / v0.2-cleanup) over a single session, then integrated through
this slice document. Where v0.2 lit up every surface (LSP, pkg, doc,
stdlib, debuginfo, Wasm CM), v0.3 turns the screws on what the compiler
actually guarantees.

## What landed

### Borrow hardening — `mty-borrow` (commits `d888ef2`, `8b98669`)

- **A54 — Place algebra (field-level borrows).** A `Place` is a rooted
  projection path (`root: String, projs: Vec<Proj>` with `Proj` =
  `Field | Index | Deref`). Two Places overlap iff one is a prefix
  of the other. `BorrowLedger` keys live borrows by Place rather than
  whole-local, so `s.a` and `s.b` no longer conflict.
- **A55 — NLL last-use deactivation.** A per-fn `nll::Pre` pre-pass
  walks the typed HIR assigning a monotone `ProgramPoint` to every
  `Path` use, recording each local's highest-point use. The main
  walker calls `maybe_decay_after_use(name)` after each Path use;
  past the last use, ledger records borrowed by that name are dropped
  and the root local's ownership state is recomputed. Pins the
  canonical `let r = &x; use(r); let m = &mut x` chain.
- **A56 — Precise MT3009 for `move *ref`.** `Unary { op: Deref, rhs }`
  in a Use position whose pointee is non-Copy emits MT3009; Copy
  pointees still load cleanly. Distinguished from MT3001/MT3008.
- **Lambda body snapshot.** Lambda bodies snapshot/restore the
  `BorrowLedger` alongside locals, so borrows established inside a
  closure body don't leak out.

Soundness gaps documented (and deferred to v0.4) in
`BORROW_V0_3_NOTES.md`: two-phase borrows, deeper field paths
(`s.a.b` truncates to `s.a`), index-aware disjointness, loop
back-edge borrows, conditional-branch joins.

### Effect / cap hardening — `mty-types` (commit `e239b30`)

Closes three slice-3/4/5 loose ends:

- **A65 — Scope-aware permissive/strict tolerance.** Slice-3's A21
  permissive fresh-var fallback for unresolved names now only fires
  in permissive scopes (TopLevelFn, ExternBlock, Macro, Unsafe,
  Arena, Budget, Sandbox). Strict scopes (AgentBody, HandlerBody,
  SupervisorBody, CapNarrowBody) promote unresolved → MT2021 with a
  scope-labeled note. SupervisorBody and CapNarrowBody mark strict
  for framework consistency but keep `tolerance_open=true` until
  slice-7 wires real cap-name resolution.
- **A65.b — Sendable trait.** Cross-agent message-arg shape is now a
  formal trait (`crate::sendable`): Copy ∨ (owned + Sized + no internal
  refs) ∨ `#[derive(Sendable)]`. Refs / caps / dyn / transitive non-
  Sendable compounds fail with MT3011 at every `!Msg(...)` / `?Msg(...)`
  site. `derive(Sendable)` is a pure marker.
- **A65.c — MT4031 local-protocol param-type check.** Handlers of
  locally-declared protocols bind params to fresh inference vars,
  body-check normally, then unify each inferred type against the
  protocol-declared type. Mismatch → MT4031 with both spans. External
  protocols continue to MT2026-warn (preserves canonical examples 13
  and 19).
- **A65.d — Explicit MT4002 `alloc-in-core` ratification.** Strict
  `profile = "core"` check now has an explicit unit test +
  conformance shape (`tests/conformance/effect_checking/05_strict_core_profile/`).

See `EFFECTS_V0_3_NOTES.md` for the six interpretation calls
(IC1..IC6) — most notably the supervisor/cap-narrow strict-but-open
posture and the generic-vars-permissive Sendable rule.

### Runtime polish — `mty-runtime` (commit `6600f4c`)

- **A70 — Cooperative mid-turn cancellation** (closes A41). The
  per-turn deadline now races the synchronous `run_handler_isolated`
  call (wrapped in `spawn_blocking`) against a `CancellationToken`.
  On cancel the blocking thread detaches; its wall time is bounded
  by the MtyIR step budget (1M). `ask` callers see exactly-once notify
  via a shared reply slot (`Arc<Mutex<Option<oneshot::Sender>>>`).
- **A71 — OTLP wire-format telemetry** (closes A38). New `OtlpHandle`
  feature-gated behind `otlp` (default on). `STARDUST_OTLP_ENDPOINT`
  routes spans/metrics to a collector via tonic-gRPC; init failure
  silently falls through to the JSON sink + one diagnostic line.
- **A72 — Slab-pool mailbox frames** (closes A40). Per-mailbox
  `SlabPool` reuses pre-allocated `MessageFrame` slots (matches
  mailbox capacity 1:1 by default). API byte-for-byte compatible —
  the new `_slab: Option<PooledFrame>` field is `pub(crate)`.
- **A73 — Batched per-turn deadline scheduler.** New `DelayScheduler`
  ships as the batched building block; the per-turn timer keeps the
  single-tokio-spawn shape for v0.3, slated for migration once
  supervisors run >10 children.

See `RUNTIME_V0_3_NOTES.md` for the eight interpretation calls (no
mty-sir edits, reply via shared slot, slab-per-mailbox, etc.) and
the seven open follow-on items.

### v0.2 cleanup (commit `955291d`)

- **Stdlib host install.** `mty-cli::main` installs
  `sdust_runtime::host_std::install_dispatcher(cli_std_dispatch)`
  before clap parses. Dispatcher wraps `sdust_stdlib::host::dispatch`
  with a `std.`-prefix retry so MtyIR-lowered paths like `["json"]`
  route correctly. `pipeline::run_file` swapped from inert `RealHost`
  to `StdHost`. Outcome: `mty run` invocations of `std.json.parse`
  now return real `serde_json` data, not `Value::Unit`.
- **6 example mains + private-helper convention.** Examples
  05/06/11/14/15/17 gained `fn main() { log("...") }`. Non-main
  top-level identifiers (helpers, externs, exports) prefixed with `_`
  so `is_exportable_fn` filters them out of the Component wrapper's
  WIT world. Outcome: 20/20 wasm-Component (was 14/20 in v0.2).
- **Conformance 5→2.** `budget_violation/03_wall_timeout` and
  `supervisor_restart/03_rate_limit_exhausted` were over-conservative
  ignores — already passed and got un-ignored.
  `budget_violation/02_step_budget_exceeded` was rewritten from the
  broken `loop {}` shape to recursion + per-case `step_budget.txt`
  knob; now traps cleanly with MT5009. Remaining 2:
  `capability_checking/03_narrow_to_ro` (needs mty-types
  cap-narrowing) and `supervisor_restart/02_escalate` (needs
  mty-syntax grammar expansion).
- **LLVM backend install docs beefed up** (install itself out of
  scope without a Linux/LLVM-17 host).

See `V0_2_CLEANUP_NOTES.md` for full Option-A-vs-B rationale.

## Test count delta

| Milestone | Tests | Delta |
|---|---|---|
| v0.1.0 | 376 | baseline |
| v0.2.0 | 550 | +174 |
| v0.3.0 | **623** | **+73** |

0 failures, 1 ignored (network-bound git-fetch test in `mty-pkg`).
`cargo clippy --workspace --all-targets -- -D warnings` clean.
`cargo fmt --all -- --check` clean.

## Closed deferrals from v0.2

| Item | Status in v0.3 |
|---|---|
| Stdlib host install (driver dep cycle) | **fixed** (CLI-side bridge via `host_std::install_dispatcher`) |
| 6/20 wasm-CM build failures (`main`-less examples) | **fixed** (20/20 Components) |
| 5 `INTENTIONALLY_IGNORED` conformance cases | **3 of 5 closed**; 2 remain (cap-narrowing + escalate) |
| Strict OTLP wire format | **shipped** (A71) |
| Slice-7 cancellation (A41 between-turns only) | **shipped** (A70 mid-turn) |
| Slab/free-list mailbox frames (A40 deferred) | **shipped** (A72) |
| Field-level borrow tracking | **shipped** (A54) |
| True NLL (last-use deactivation) | **shipped** (A55 — hand-rolled, not Polonius) |
| LLVM backend untested | **install docs improved**; smoke deferred (no LLVM 17 host) |

## Closed amendments

The v0.2 SLICE_V0_2 doc proposed drafts A54..A60 covering 7 v0.2
interpretation calls (Manifest.deps, Wasm CM canonical imports,
DWARF v4, stdlib dispatcher, MtyIR `run_subfn`, Component default).
Those drafts were never written to the spec. v0.3 reused the
A54..A56 number range for the borrow checker work; the v0.2
interpretation calls remain documented in the v0.2 slice docs but
are not formal amendments. v0.4 will renumber the v0.2 drafts to
A57..A63 if/when they need spec status.

Actual v0.2 deferred amendments now closed in v0.3:

- **A38** (telemetry JSON-only) → superseded by **A71** (OTLP)
- **A40** (mailbox defaults, no slab pool) → superseded by **A72**
- **A41** (cancellation between turns only) → superseded by **A70**

## New amendments (committed to spec)

```
A54 — Field-level borrow tracking via Place algebra (v0.3)
A55 — NLL last-use deactivation (v0.3)
A56 — Precise MT3009 for `move *ref` (v0.3)
A65 — Scope-aware permissive/strict type-check policy (v0.3)
A65.b — Sendable trait at cross-agent message sites (v0.3)
A65.c — MT4031 strict handler param-type check, local-protocol only (v0.3)
A65.d — `core` profile rejects `alloc` (v0.3 conformance)
A70 — Cooperative mid-turn cancellation (v0.3)
A71 — OTLP wire-format telemetry (v0.3)
A72 — Slab-pool mailbox frames (v0.3)
A73 — Batched per-turn deadline scheduler (v0.3)
```

Total: 11 new amendments in `docs/spec/v0.1-amendments.md`.

## Soundness assertions (what's now strictly enforced)

| Property | v0.2 status | v0.3 status |
|---|---|---|
| Last-use deactivates a borrow | whole-local lexical | **per-Place NLL** (A55) |
| Field borrows of disjoint fields | conflated | **disjoint via Place algebra** (A54) |
| `move *ref` of non-Copy | partial (MT3001/MT3008 in some shapes) | **explicit MT3009** (A56) |
| Cross-agent send carries non-Sendable | informal | **formal Sendable trait + MT3011 at every site** (A65.b) |
| Unresolved name in agent / handler body | A21 fresh-var (silent) | **strict MT2021** (A65) |
| Handler param matches protocol decl (local) | trust-the-decl | **post-check + MT4031 on mismatch** (A65.c) |
| Cancellation mid-turn | between-turns only (A41) | **cooperative mid-turn cancel** (A70) |
| OTLP telemetry shape | JSON sidecar | **real OTLP/gRPC** (A71) |
| Lambda body borrow leaks | possible | **snapshot/restore prevents leak** |

## New diagnostic codes

No new SD codes were minted; existing codes were re-aimed:

- **MT2021** (unresolved_value) — text + note updated for strict
  scope context
- **MT3009** (move_out_of_ref) — newly precise for `*ref` of
  non-Copy
- **MT3011** (non_sendable_message_arg) — rule formalized; reason
  note tied to Sendable definition
- **MT4031** (protocol_param_type_mismatch) — newly activated for
  local protocols
- **MT4041** (derive_unknown) — text now lists `Sendable` as
  supported derive

## Cross-cut fixes applied during integration

1. **LSP integration test `diagnostics_type_error_produces_at_least_one`**
   (`crates/mty-lsp/tests/integration.rs`) — the test relied on
   `definitely_not_a_real_fn(...)` in `fn main()` producing MT2021,
   which A65 reclassifies as a permissive TopLevelFn scope (silent
   fresh-var fallback). Rewrote the test to invoke the unresolved
   call inside an agent handler body (strict HandlerBody scope) so
   the diagnostic fires reliably and the test pins a stable contract
   under future tolerance-policy changes.
2. **20/20 wasm-Component example sweep gate** added as new test
   `all_examples_compile_wasm_component` in
   `crates/mty-driver/tests/conformance_codegen.rs`. The two
   prior sweeps (`all_examples_compile_native`,
   `all_examples_compile_wasm`) verified only the bare-core wasm
   path; the new test exercises the full
   `wrap_as_component(core, wit)` path and validates the resulting
   component bytes via `wasmparser`. Pins the v0.2-cleanup agent's
   "20/20 wasm-CM" claim as a permanent regression gate.

Total: 2 files touched. Both are test-side; no substantive agent
work was modified.

## New deferrals to v0.4

Consolidated from `BORROW_V0_3_NOTES.md`, `EFFECTS_V0_3_NOTES.md`,
`RUNTIME_V0_3_NOTES.md`, `CONFORMANCE_V0_3_NOTES.md`, and
`V0_2_CLEANUP_NOTES.md`.

### Borrow

1. Two-phase borrows (`vec.push(vec.len())`) — over-restrictive,
   not unsound.
2. Deeper field paths (`s.a.b` truncates to `s.a`).
3. Index-aware disjointness (`arr[0]` vs `arr[1]`).
4. Loop back-edge borrow modelling.
5. Conditional-branch ledger joins (one-arm borrow conservatively
   stays live).
6. Cross-fn region inference (no explicit lifetime parameters yet
   — likely v0.5+).
7. Move-out-of-deref-of-deref (one-step deref only in v0.3).

### Effects / caps

8. Slice-7 supervisor/cap-narrow name binding — flip
   `tolerance_open=false` when real cap-name resolution lands;
   MT2021 activates automatically.
9. Per-case `mighty.toml` in conformance harness (for direct MT4002
   conformance).
10. Function-signature cap-narrowing (propagate `CapConstraint::And`
    into fn signatures).
11. Cross-package Sendable propagation (external opaque ADTs
    permissively Sendable today).
12. Sendable lambda capture analysis (closure capture-set on Fn
    type).

### Runtime

13. MtyIR-side cancellation polling (interrupt mid-turn truly, not
    via detach).
14. CpuBudget reason wiring (variant exists, no firing path).
15. HTTP/protobuf OTLP transport selector (gRPC hardcoded today).
16. OTel resource-attribute env-var overrides
    (`OTEL_RESOURCE_ATTRIBUTES`, `OTEL_SERVICE_NAME`).
17. DelayScheduler as default per-turn timer (migrate when >10
    agents).
18. Slab pool benchmark (no `criterion` bench harness yet).
19. `Mailbox::with_pool` through `RuntimeBuilder`.

### Conformance

20. `capability_checking/03_narrow_to_ro` — needs mty-types
    `Fs.ro` cap narrowing.
21. `supervisor_restart/02_escalate` — needs mty-syntax `escalate`
    grammar.

### Carried over from v0.2 (still open)

22. WASI Preview 2 bindings + user-authored WIT.
23. DWARF v5 + per-instruction line program + symbol relocations.
24. Backtracking package resolver + tar/flate2 + real registry.
25. `dyn Trait` dispatch + closure capture in compiled code.
26. Real `loop { break }` lowering.
27. LLVM backend smoke testing on a host with LLVM 17.
28. PGO / ThinLTO, multi-core scheduler, distributed agents,
    procedural macros, effect-row polymorphism.

## Stats

- **5 commits since v0.2.0** (7ff5629 prep + 4 swarm + 1 cleanup +
  1 merge) — the swarm rolled up tightly because each agent touched
  disjoint crates.
- **7,507 insertions / 238 deletions** across 133 files (vs v0.2's
  20,861/400/295).
- **0 new crates** — workspace stays at 19. The swarm hardened
  existing surfaces.
- **+73 new tests** (550 → 623).
- **0 clippy warnings** with `-D warnings`.
- **20/20 examples build to native objects** (unchanged from v0.2).
- **20/20 examples build to bare wasm core modules** (unchanged).
- **20/20 examples build as Wasm Components** (was 14/20 in v0.2 —
  the 6 holdouts gained `main`s + `_`-prefixed helpers).
- **32 conformance cases run** (was 25 in v0.2), 2 ignored (was 5).
- **11 new spec amendments** (A54, A55, A56, A65, A65.b, A65.c,
  A65.d, A70, A71, A72, A73).
- **MSRV unchanged at 1.85**.

## What's next

v0.4 picks up the 28-item deferral list above. The headline themes
will likely be:

- Polonius-style borrow checker + conditional-branch join refinement
- Real cap-name resolution wiring (slice-7 plumbing → flips
  supervisor / cap-narrow to strict MT2021)
- MtyIR-side cancellation polling (true mid-turn interrupt)
- WASI Preview 2 + user-authored WIT in the Component pipeline
- LLVM backend smoke on a Linux host with LLVM 17
- Backtracking package resolver + tar/flate2 + real registry

The aspirational v0.4 tagline: *"every soundness assertion the spec
makes is enforceable end-to-end, and the compiler tells you why
when it isn't."*
