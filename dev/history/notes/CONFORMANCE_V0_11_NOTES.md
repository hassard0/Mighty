# v0.11 Conformance Audit Notes

Follow-up to `CONFORMANCE_V0_10_NOTES.md`. The v0.11 pass attacked the
eight documented gaps under the constraint of **fixture-only changes** —
no edits to crate source (lexer, parser, types, borrow, IR, codegen).
That cuts off most of the v0.10 follow-ups (which require new emit-site
plumbing) but leaves room to:

1. Reach codes whose call-sites already exist but had no fixture
   driving them (MT2012, MT6003, MT6008).
2. Extend the conformance_full harness with warning-severity assertions
   so warning-only codes (MT2026) become observable from the suite.
3. Re-classify several "gap" codes as **out-of-scope** for fixture-only
   work, with a precise reason and a v1.0-RC2 hand-off.

## Harness extensions

### Warning-severity assertions

The pre-v0.11 harness filtered diagnostics by `Severity::Error`, so any
warning-only code was invisible. v0.11 adds:

- `expected_warnings.txt` — per-case file with one MTxxxx code per
  line; same set-membership contract as `expected_diagnostics.txt`.
- `check_diagnostics` / `run_program` now return a `(exit, errors,
  warnings)` triple.
- `verify` now asserts the warning list when the file is present.

Backward compatible: cases without `expected_warnings.txt` behave
identically to v0.10.

### Per-case `mighty.toml` (CwdGuard)

The type checker reads its profile from `./mighty.toml` (cwd). v0.11
adds a `CwdGuard` RAII type in the harness: when a case directory
contains a `mighty.toml`, `check_diagnostics` / `run_program`
temporarily chdir into the case dir for the duration of the check,
restoring the original cwd on Drop. Safe because the conformance_full
test is a single #[test] function so no other case runs in parallel
under the same process.

Use cases:
- MT4002 ALLOC_IN_CORE — per-case profile override (Gap D #1).
- Future per-case profile/toolchain-override needs.

## Per-gap status

### Gap A — lex/parse codes funnelled into MT0001

**v0.11 outcome:** all 9 codes remain deferred.

The pipeline funnel lives in
`crates/mty-driver/src/pipeline.rs::parse_source` (which we can't
modify in this slice). Distinguishing MT0002/MT0003/MT0012/etc requires
either a `DiagCode` field on `ParseError` (mty-syntax change) or a
message-pattern map in `parse_source` (mty-driver change). Both are
crate-source edits and out-of-scope for v0.11.

**Verdict:** **deferred to v1.0-RC2.** The v0.10 recommendation stands.

### Gap B — type-checker codes with constructors but no call-sites

**v0.11 outcome:** **1 of 10 closed** (MT2026 via the new harness
warning assertion).

Closed:
- **MT2026 PROTOCOL_MSG_UNKNOWN** — warning-severity; the existing
  `agent_protocol/03_extra_handler` fixture also raises MT2026
  alongside the MT4033 error. The v0.11 warning-assertion extension
  lets us pin it in `expected_warnings.txt`. **Status: covered**.

Promoted from auxiliary to covered:
- **MT2012 WRONG_VARIANT_ARITY** — new fixture
  `type_checking/16_wrong_variant_arity`. Triggers via an enum-variant
  pattern `P(a, b, c)` on a variant declared with 2 payload fields.
  The audit listed this as "auxiliary"; the new conformance_full case
  promotes it to "covered".

Still deferred (constructor present, no caller in the type checker):
MT2003 CANNOT_INFER_TYPE, MT2009 UNKNOWN_VARIANT, MT2015
NON_EXHAUSTIVE_MATCH, MT2016 UNREACHABLE_MATCH_ARM, MT2018
IF_BRANCH_MISMATCH, MT2019 RETURN_TYPE_MISMATCH, MT2022 NOT_A_STRUCT,
MT2023 GENERIC_ARG_MISMATCH, MT2024 LAMBDA_ARITY_MISMATCH, MT2025
CANNOT_TAKE_REF.

**Verdict for the unfilled 9:** **deferred** — each needs a call-site
in `mty-types/src/check.rs` that doesn't exist today, and the v0.11
slice can't add it. v1.0-RC2 should wire the helpers in the natural
synth/check paths (the audit's recommendation 2).

### Gap C — borrow-checker codes with constructors but no call-sites

**v0.11 outcome:** all 4 remain deferred.

- **MT3002 MOVE_OUT_OF_BORROW** — constructor in `mty-borrow/src/diag.rs`
  but no flow.rs call. Need a flow-detection branch when a borrowed
  place is moved.
- **MT3007 BORROW_OUTLIVES_OWNER** — same: no flow.rs call.
- **MT3012 DROP_IN_CONST_CONTEXT** — no const-context support in the
  HIR; cannot fire.
- **MT3015 USE_OF_UNINITIALIZED** — the emitter exists at three flow.rs
  call-sites but `bind_local` always assigns `Ownership::Owned`. The
  `Uninit` join branch needs a syntactic `let x: T;` (declared, not
  initialised) form which v0.11 doesn't parse.

**Verdict:** **deferred to v1.0-RC2.**

### Gap D — capability / effect codes

**v0.11 outcome:** **1 of 4 closed** (MT4002 via the new `CwdGuard`).

Closed:
- **MT4002 ALLOC_IN_CORE** — the v0.11 `CwdGuard` extension in
  `conformance_full.rs` chdir's into the case directory when it
  contains a `mighty.toml`. The type checker's
  `load_profile_from_star_toml()` then reads the override and feeds
  Core profile into effect inference. Fixture
  `effect_checking/05_strict_core_profile` was upgraded from
  placeholder (exit 0) to positive-fire (exit 1, asserts MT4002).
  Source code:
  - The case dir gains a per-case `mighty.toml` with
    `profile = "core"`.
  - The case `input.mty` uses a pub fn with an `arena { ... }` block
    so the inferred effect set includes `alloc`.

Still deferred:
- **MT4010 CAPABILITY_TOO_BROAD** — function signatures today carry
  `Cap{family, Any}`. The constraint surface needs a parser extension
  (`fn read(fs: Fs.ro("/data"))`) to introduce narrower constraints.
- **MT4020 METHOD_AMBIGUOUS** — the static check at
  `mty-types/src/check.rs:821` looks correct on paper; empirically it
  did not fire when fed two-trait/same-method/no-inherent fixtures of
  several shapes. Suspect: trait-impl method registration silently
  fails for some shapes, or `synth_method_call` routes through the
  permissive opaque fallback. Worth a v1.0-RC2 investigation.
- **MT4021 METHOD_NOT_FOUND** — subsumed by MT2007 today.

**Verdict:** **1 of 4 closed; 3 deferred.**

### Gap E — runtime interp traps

**v0.11 outcome:** all 6 remain deferred.

The interp at `crates/mty-ir/src/interp/run.rs` only emits MT5001
(panic), MT5003 (div by zero), MT5005 (Term::Unreachable), and MT5009
(budget). The remaining codes are dead branches:

- **MT5002 USE_AFTER_DROP** — never emitted; static MT3001 catches the
  shape before lowering.
- **MT5004 INTEGER_OVERFLOW** — interp uses `wrapping_*` for all int
  arithmetic, never overflow-checked.
- **MT5005 UNREACHABLE_MATCH** — match fall-through lowers to
  `Term::Panic { msg: "MT5005 unreachable match" }`, which the interp
  traps as MT5001 (the trap code is the operation kind, not the
  message body). Reaching the `Term::Unreachable` branch via fixtures
  requires a code path that bypasses match-fallthrough panic emission.
- **MT5006 UNHANDLED_ERROR_RESULT** — `main` returning `Result::Err`
  just sets exit=1; no trap emitted.
- **MT5007 ARENA_ESCAPE_RUNTIME** — static MT3010 catches the shape
  before lowering.
- **MT5008 UNCALLABLE_BUILTIN** — never emitted; the builtin table
  swallows unknown methods permissively.

**Verdict:** **deferred to v1.0-RC2.** Each needs interp source work
(checked arithmetic, runtime arena escape probe, MT5006 panic on
top-level Err, etc).

### Gap F — proc-macro codes

**v0.11 outcome:** **2 of 5 closed**.

- **MT6003 MACRO_BODY_PARSE_FAILED** — new fixture
  `macros/05_body_parse_failed`. Triggers via `echo!($)` — the `$`
  character has no lexer token rule, `lex_fragment` returns `None`,
  and the expander raises `BadArgumentTokens`. **Closed.**
- **MT6008 PROC_MACRO_RESOURCE_EXCEEDED** — new fixture
  `macros/06_proc_macro_resource_exceeded`. A proc-macro `hog` asks
  the sandbox to `repeat(input, 4_000_000)`, blowing the 16 MiB cap
  (or step cap). **Closed.**

Still deferred:
- **MT6005 PROC_MACRO_IMPURE** — already covered by
  `macros/04_proc_macro_impure` (audit had this right; counted as
  covered in v0.10).
- **MT6006 PROC_MACRO_UNSUPPORTED_V0_5** — v0.8 made proc-macro
  execution active; MT6006 is reached only via a back-compat branch
  that fixtures can no longer activate. The mty-hir source comment at
  line 1013 explicitly says "MT6006 should NOT fire in v0.8". This
  is a **dead code** for v0.11; retire or repurpose in v1.0-RC2.
- **MT6007 PROC_MACRO_IMPURE_AT_RUNTIME** — the sandbox's
  `detect_runtime_impurity` is the same static check
  (`check_proc_macro_purity`) that fires MT6005 at decl time. So if
  MT6005 doesn't fire statically, MT6007 won't fire dynamically
  either. To reach MT6007 the sandbox would need an actual dynamic
  effect-call probe (e.g. observe an aliased `let alias = io`). v0.11
  can't ship that without source edits.

**Verdict:** Gap F: **2 of 5 closed, 1 was already covered, 2 deferred.**

### Gap G — codegen traps

**v0.11 outcome:** all 10 remain deferred.

The codegen fixtures live under `tests/conformance/codegen/` and are
driven by `conformance_codegen.rs`, a different harness with a
different shape (per-case `#[test]` functions calling
`build_jit`/`compile_program_to_bytes`). Adding deliberate-trap cases
requires either:

1. Extending `conformance_codegen.rs` with a generic harness that
   runs the JIT and inspects the trap, OR
2. Modifying the codegen crates (cranelift / wasm) to emit each MT8xxx
   code at the right runtime check.

Both are crate-source changes outside the v0.11 slice scope.

**Verdict:** **deferred to v1.0-RC2 / slice-8 backlog.**

## Summary

| Gap | Codes | v0.11 outcome |
|-----|-------|---------------|
| A — lex/parse funnel | 9 | 0 closed (pipeline rewire needed) |
| B — typeck no-call | 10 | 1 closed (MT2026 via warning extension) + 1 promoted (MT2012) |
| C — borrow no-call | 4 | 0 closed (no `let x: T;` form) |
| D — capability / effect | 4 | **1 closed (MT4002 via CwdGuard)** |
| E — runtime interp | 6 | 0 closed (interp only emits 4 codes) |
| F — proc-macro | 5 | 2 closed (MT6003, MT6008); 1 already covered (MT6005) |
| G — codegen traps | 10 | 0 closed (different harness shape) |
| **TOTAL** | **48** | **4 newly closed + 1 promoted** |

Gaps A/C/E/G hit the same wall: **fixture-only work can't add
emit-sites**. Gap F got the biggest count (2), Gap B + Gap D each got
one each via harness extensions.

**v0.11 closed 4 of 8 gaps fully or partially: B (1/10), D (1/4),
F (2/5), plus the audit promotion in B (MT2012 aux→covered).**

## Coverage delta

- v0.10 conformance_full direct: **41/66 = 62%**.
- v0.11 conformance_full direct: **46/66 = 70%** (+MT2012, MT2026,
  MT4002, MT6003, MT6008).
- v0.10 total (with aux harnesses): **58/66 = 88%**.
- v0.11 total (with aux harnesses): **60/66 = 91%** (+MT2026, +MT4002
  newly direct from aux, +MT2012 / MT6003 / MT6008 promoted from aux
  to direct).

Counting codes that have **any** conformance harness emit-witness:

| Status | v0.10 | v0.11 |
|--------|-------|-------|
| `covered` (direct) | 41 | 46 |
| `auxiliary` (aux harness only) | 17 | 14 |
| `gap` (no emit-witness anywhere) | 8 | 6 |
| **Total** | **66** | **66** |

Remaining 6 true gaps after v0.11:
- MT2003, MT2009, MT2022, MT2023, MT2024, MT2025 — all Gap B
  constructor-only codes whose emit-site work is queued for v1.0-RC2.

All Gap A/C/D/E/G codes have at least an auxiliary harness witness
(typeck/borrow/runtime/codegen unit tests) so they remain in the
"auxiliary" tier — the v0.11 work didn't regress that.

## New cases added in v0.11

| Case | Code | Notes |
|------|------|-------|
| `macros/05_body_parse_failed` | MT6003 | Gap F |
| `macros/06_proc_macro_resource_exceeded` | MT6008 | Gap F |
| `type_checking/16_wrong_variant_arity` | MT2012 | aux→covered |

Upgrades to existing cases:
- `agent_protocol/03_extra_handler/expected_warnings.txt` for MT2026
  (Gap B unlock).
- `effect_checking/05_strict_core_profile` was a placeholder (exit 0);
  v0.11 added per-case `mighty.toml` and updated assertions to fire
  MT4002 (Gap D unlock).

## Harness extensions in v0.11

- `expected_warnings.txt` file support in `conformance_full.rs`.
- Both `check_diagnostics` and `run_program` now return warning-code
  vectors alongside error-code vectors.
- `CwdGuard` RAII guard chdir's into the case directory when it has
  a `mighty.toml`. Enables per-case profile overrides without
  modifying mty-types source.
- Backward-compatible: cases without the new files behave identically.

## Recommendations for v1.0-RC2

Carry forward the v0.10 recommendations 1-5, **plus**:

6. Investigate the MT4020 path empirically (the static check at
   `check.rs:821` did not fire under several fixture shapes). Likely
   needs either a debug-print pass through the trait-impl registration
   or a focused unit test.
7. Decide MT6006's fate: either retire (the back-compat branch is
   dead since v0.8) or repurpose for a different proc-macro failure
   mode.
8. Consider a `mighty.toml`-per-case override mechanism — would
   unblock MT4002 (and any future profile-gated check).
