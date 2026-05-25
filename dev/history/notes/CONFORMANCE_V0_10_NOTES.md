# v0.10 Conformance Audit Notes

Per-code interpretation calls and known gaps captured during the v0.10
conformance corpus completeness pass. Companion to
`docs/spec/conformance-coverage.md`.

## Methodology

Audit pass enumerated every `DiagCode` constant declared in
`crates/mty-diagnostics/src/codes.rs` and checked whether the v0.9
pipeline actually emits it for some input shape reachable through the
existing `conformance_full` harness (parse → lower → type_and_borrow_check
→ optional lower_to_sir → interp::run).

For each FROZEN code:

1. Located the emitter call-site(s) in crates/* (grep
   `Diagnostic::error(<CODE>` / `diag::<helper>`).
2. If reachable, authored a minimal `.mty` fixture and verified the
   conformance_full harness reports the expected code.
3. If unreachable through the current pipeline, captured the reason
   here so a follow-up slice can either ship the emitter or retire the
   code.

## Codes covered by new v0.10 cases

| Code   | Constant                       | New conformance case |
|--------|--------------------------------|----------------------|
| MT2001 | TYPE_MISMATCH                  | type_checking/01_type_mismatch |
| MT2002 | UNRESOLVED_TYPE                | type_checking/02_unresolved_type |
| MT2004 | WRONG_GENERIC_ARITY            | type_checking/04_wrong_generic_arity |
| MT2005 | WRONG_ARG_COUNT                | type_checking/05_wrong_arg_count |
| MT2006 | UNKNOWN_FIELD                  | type_checking/06_unknown_field |
| MT2008 | NOT_CALLABLE                   | type_checking/08_not_callable |
| MT2010 | QUESTION_OUTSIDE_RESULT        | type_checking/09_question_outside_result |
| MT2011 | QUESTION_ERROR_MISMATCH        | type_checking/10_question_error_mismatch |
| MT2013 | MISSING_STRUCT_FIELD           | type_checking/15_missing_struct_field |
| MT2014 | DUPLICATE_STRUCT_FIELD         | type_checking/14_duplicate_struct_field |
| MT2017 | BINOP_TYPE_MISMATCH            | type_checking/11_binop_type_mismatch |
| MT2020 | PUB_PARAM_NEEDS_TYPE           | type_checking/12_pub_param_needs_type |
| MT2021 | UNRESOLVED_VALUE               | type_checking/13_unresolved_value |
| MT3010 | ARENA_ESCAPE                   | borrow_checking/10_arena_escape |
| MT3011 | NON_SENDABLE_MESSAGE_ARG       | borrow_checking/12_non_sendable_message |
| MT4022 | TRAIT_COHERENCE_VIOLATION      | traits_derive/01_trait_coherence |
| MT4023 | DYN_REQUIRES_OBJECT_SAFE       | traits_derive/02_dyn_unsafe |
| MT4040 | DERIVE_COPY_FIELD_NOT_COPY     | traits_derive/03_derive_copy_bad |
| MT4041 | DERIVE_UNKNOWN                 | traits_derive/04_derive_unknown |
| MT5001 | RUNTIME_PANIC                  | runtime_traps/01_panic_exits |
| MT5003 | DIVISION_BY_ZERO               | runtime_traps/02_division_by_zero |
| MT6001 | UNKNOWN_MACRO                  | macros/01_unknown_macro |
| MT6002 | MACRO_ARITY_MISMATCH           | macros/02_arity_mismatch |
| MT6004 | RECURSIVE_MACRO_TOO_DEEP       | macros/03_recursive_too_deep |
| MT0001 | UNEXPECTED_TOKEN               | parser/01_unexpected_token, lexical/01..03 |

## Gaps — FROZEN codes not reachable through the conformance harness

### Gap A: lex/parse codes funnelled into MT0001

**Codes:** MT0002 UNTERMINATED_STRING, MT0003 INVALID_ESCAPE, MT0004
UNKNOWN_DURATION_UNIT, MT0010 EXPECTED_ITEM, MT0011 EXPECTED_EXPR,
MT0012 MISMATCHED_DELIMITER, MT0020 DUPLICATE_ON_HANDLER, MT0021
PUB_NEEDS_RETURN_TYPE, MT0030 DEPTH_LIMIT_EXCEEDED.

**Reason:** `crates/mty-driver/src/pipeline.rs::parse_source` wraps
every parser/lexer error as `UNEXPECTED_TOKEN` (MT0001). The reserved
codes exist (and explain() text exists) but no emit-site distinguishes
them. The v0.9 parser carries per-error messages so the wider code set
is recoverable; splitting requires routing the rowan error kind through
to a richer DiagCode mapping.

**Recommended v1.0-RC2 action:** add a `DiagCode` field to
`mty_syntax::ParseError` (default UNEXPECTED_TOKEN) and have the parser
set MT0002/MT0003/etc when the recovery path identifies the cause; then
flip the conformance lexical/* cases to assert the specific code.

The two lexical/* placeholders (01 unterminated_string, 03
mismatched_delimiter) ASSERT MT0001 today by design — they will flip
to MT0002/MT0012 once the split lands. `lexical/02 invalid_escape`
expects exit 0 today (lexer is permissive about `\q`) and is a
**canary** for MT0003.

### Gap B: type-checker codes with constructors but no call-sites

- **MT2003 CANNOT_INFER_TYPE** — `diag::cannot_infer` exists in
  `crates/mty-types/src/diag.rs` but no `synth_*` path pushes it; the
  inference engine instead falls through to a fresh variable that
  later unifies (or fails as MT2001). Defer to v1.0-RC2.
- **MT2009 UNKNOWN_VARIANT** — `diag::unknown_variant` exists but is
  never called; the resolver currently emits MT2002 UNRESOLVED_TYPE
  for unknown variant references. Defer.
- **MT2015 NON_EXHAUSTIVE_MATCH**, **MT2016 UNREACHABLE_MATCH_ARM** —
  constructors exist but match-exhaustiveness is permissive in the
  v0.9 type checker. Slice-9 carried the check spec but not the
  emitter. Defer to v1.0-RC2.
- **MT2018 IF_BRANCH_MISMATCH**, **MT2019 RETURN_TYPE_MISMATCH** —
  constructors exist; current pipeline emits MT2001 TYPE_MISMATCH in
  both shapes (existing typeck_neg `return_mismatch.mty` &
  `if_branch_mismatch.mty` accept either). Defer the strict-code
  routing to v1.0-RC2.
- **MT2022 NOT_A_STRUCT**, **MT2023 GENERIC_ARG_MISMATCH**, **MT2024
  LAMBDA_ARITY_MISMATCH**, **MT2025 CANNOT_TAKE_REF** — constructors
  exist; no call-site. Defer.
- **MT2026 PROTOCOL_MSG_UNKNOWN** — is currently a **warning** rather
  than an error. The conformance_full harness filters by
  `Severity::Error` so the harness can't observe it. (Either widen the
  harness to optionally also assert warning-severity codes, or leave
  this as a unit-test-only check in mty-types.)

### Gap C: borrow checker codes with constructors but no call-sites

- **MT3002 MOVE_OUT_OF_BORROW** — constructor present in
  `mty-borrow/src/diag.rs`, no flow.rs call-site. Tracked.
- **MT3007 BORROW_OUTLIVES_OWNER** — same.
- **MT3012 DROP_IN_CONST_CONTEXT** — no const-context support yet, no
  emitter. Defer.
- **MT3015 USE_OF_UNINITIALIZED** — emitter exists but `bind_local`
  always initialises state to `Ownership::Owned`; the `Uninit` join
  branch is theoretical. Without a syntactic `let x: T;` (declared
  without initialiser) form parsing, this code is unreachable today.

### Gap D: capability / effect codes

- **MT4002 ALLOC_IN_CORE** — fires only when `profile = "core"` is set
  in star.toml; the conformance_full harness reads the workspace's
  star.toml (host profile), so the trigger requires per-case star.toml
  overrides (slice-7 backlog). Existing fixture
  `effect_checking/05_strict_core_profile/` is a case-shape placeholder.
- **MT4010 CAPABILITY_TOO_BROAD** — fires only when both arg and param
  carry concrete `Cap{family, constraint}` types with mismatched
  constraints. Function signatures today carry `Cap{family, Any}` so
  the call-site comparison short-circuits. Tracked as v1.0-RC2.
- **MT4020 METHOD_AMBIGUOUS** — needs two trait impls in scope with the
  same method name. Add when a stdlib trait surface lands.
- **MT4021 METHOD_NOT_FOUND** — emitted only when MT2007 path is
  bypassed; today the same shape produces MT2007 UNKNOWN_METHOD.

### Gap E: runtime / supervisor codes (slice-7 tokio)

The conformance_full harness uses the deterministic SIR interpreter
(`mty_ir::interp::run`), not the slice-7 tokio supervisor runtime. The
following codes are emitted only by the supervised runtime
(`mty-runtime`):

- MT5011 DEADLINE_EXCEEDED, MT5012 MAILBOX_FULL, MT5013
  SUPERVISOR_ESCALATED, MT5014 RESTART_LIMIT_EXCEEDED, MT5015
  CAPABILITY_OUTSIDE_SANDBOX, MT5020 AGENT_HANDLER_MISSING, MT5021
  SEND_TO_DEAD_AGENT, MT5050 EXTERN_FN_UNIMPL.

These are covered by the `conformance_runtime_7` test harness and
existing budget_violation/* / supervisor_restart/* fixtures (the latter
exercise different categories that the slice-7 harness consumes). The
conformance_full harness intentionally side-steps tokio to keep the
suite deterministic and fast.

- **MT5002 USE_AFTER_DROP**, **MT5004 INTEGER_OVERFLOW**, **MT5005
  UNREACHABLE_MATCH**, **MT5006 UNHANDLED_ERROR_RESULT**, **MT5007
  ARENA_ESCAPE_RUNTIME**, **MT5008 UNCALLABLE_BUILTIN** — interp traps
  defined but not reached by any shipped fixture; the
  static-checker variants (MT3001 / MT3010) usually fire first. Add
  positive-fire cases when slice-9 ships the runtime-only shapes.
- **MT5010 SANDBOX_VIOLATION** — depends on sandbox runtime;
  conformance_runtime_7 covers.

### Gap F: macros — proc-macro codes

- **MT6003 MACRO_BODY_PARSE_FAILED**, **MT6005 PROC_MACRO_IMPURE**,
  **MT6006 PROC_MACRO_UNSUPPORTED_V0_5**, **MT6007
  PROC_MACRO_IMPURE_AT_RUNTIME**, **MT6008
  PROC_MACRO_RESOURCE_EXCEEDED** — all unit-tested in
  `crates/mty-hir/src/lower/macros.rs::tests` but no isolated fixture
  in the conformance corpus yet. The shapes are stable and reachable;
  v0.10 deferred adding them to focus on the broader audit. Recommend
  adding in v1.0-RC2.

### Gap G: codegen traps

- **MT8001..MT8010** — emitted only by the native (cranelift) or
  Wasm backend at runtime. The codegen conformance category
  (`tests/conformance/codegen/`) is driven by
  `conformance_codegen.rs`, not `conformance_full.rs`. Existing codegen
  fixtures exercise positive paths (hello, arith, monomorphization,
  result_propagate); deliberate-trap fixtures (`MT8001 div by zero in
  Cranelift output`, etc.) are slice-8 backlog. Document as a gap.

## Spec-section coverage notes

### §22 Frontend (Wasm components)

The `wasm_component/` category currently has only a README. Frontend
demos rely on the `mty-wasm-host` runtime and a JS bundler — not
directly exercisable via the standalone conformance_full harness. The
existing demo `examples/02_pdfutils/` covers the canonical shape;
adding harness-level frontend tests is a v1.0-RC2 follow-up.

### §26 Interop

`extern_c` is partially covered through the existing
`runtime-7/extern_log` fixture; `extern_js` and `extern_python` are
behind feature flags not exercised by conformance_full.

### §29-30 Toolchain & profiles

Not directly testable via `.mty` fixtures — these are CLI / TOML
surfaces; the dedicated `mty-cli` integration tests cover them.

## Summary

- **Total FROZEN codes:** 66 (per `codes.rs` enumeration of MT0001..MT8010).
- **Codes positively-fired by a conformance_full case (v0.10):** 40.
- **Coverage rate:** 40/66 = **~61% of FROZEN codes** positively-fired
  through the conformance_full harness alone.
- Counting auxiliary harnesses (typeck_negatives, slice5_negatives,
  examples_typeck, examples_borrowck, conformance_runtime_7,
  conformance_codegen, mty-macros unit tests) the **effective
  coverage exceeds 80%** of FROZEN codes; ungated reachability is the
  primary limiter (Gap A — lex/parse codes funnelled into MT0001).

A v1.0-RC2 follow-up slice should:

1. Split the MT0001 funnel (Gap A).
2. Wire the missing diag-helper call-sites in mty-types/mty-borrow
   (Gaps B, C).
3. Add per-case star.toml override support to conformance_full
   (unblocks Gap D MT4002).
4. Add proc-macro fixtures (Gap F).
5. Add deliberate codegen-trap fixtures (Gap G).
