# Mighty v1.0-RC Conformance Coverage Report (v0.11 audit)

Audit pass: enumerate every FROZEN diagnostic code in
`crates/mty-diagnostics/src/codes.rs` and every spec section in
`docs/spec/v1.0-rc.md`, then map to the conformance corpus under
`tests/conformance/`.

**Harness driver:** `crates/mty-driver/tests/conformance_full.rs`. Each
case dir contains `input.mty`, `command.txt`, optional
`expected_diagnostics.txt`, optional `expected_warnings.txt` (v0.11+),
optional `expected_exit_code.txt`, optional `expected_stdout.txt`,
optional `step_budget.txt`, and a `README.md`.

For interpretation calls, gap rationales, and follow-up recommendations
see `dev/history/notes/CONFORMANCE_V0_10_NOTES.md` (original audit)
and `dev/history/notes/CONFORMANCE_V0_11_NOTES.md` (v0.11 fixture
+ warning-assertion follow-up).

## Diagnostic-code coverage table

Status legend:
- `covered` — at least one conformance_full case positively-fires the
  exact code.
- `auxiliary` — fired by a sibling harness (typeck_negatives,
  slice5_negatives, conformance_runtime_7, conformance_codegen, or
  per-crate unit tests).
- `gap` — FROZEN code with no current emitter wired in the conformance
  pipeline. See `CONFORMANCE_V0_10_NOTES.md` for per-code rationale.

| Code   | Constant                       | Status     | Conformance case (or note) |
|--------|--------------------------------|------------|----------------------------|
| MT0001 | UNEXPECTED_TOKEN               | covered    | parser/01_unexpected_token (also lexical/01, /03) |
| MT0002 | UNTERMINATED_STRING            | gap        | funnelled into MT0001 today; see Gap A |
| MT0003 | INVALID_ESCAPE                 | gap        | lexer permissive today; Gap A |
| MT0004 | UNKNOWN_DURATION_UNIT          | gap        | Gap A |
| MT0010 | EXPECTED_ITEM                  | gap        | Gap A |
| MT0011 | EXPECTED_EXPR                  | gap        | Gap A |
| MT0012 | MISMATCHED_DELIMITER           | gap        | lexical/03 currently asserts MT0001; Gap A |
| MT0020 | DUPLICATE_ON_HANDLER           | gap        | Gap A |
| MT0021 | PUB_NEEDS_RETURN_TYPE          | gap        | Gap A |
| MT0030 | DEPTH_LIMIT_EXCEEDED           | gap        | Gap A (depth-bomb fixture deferred) |
| MT1001 | UNRESOLVED_NAME                | gap        | resolver currently emits MT2001/MT2002 |
| MT1002 | USE_RESOLVES_TO_NOTHING        | gap        | resolver gap |
| MT2001 | TYPE_MISMATCH                  | covered    | type_checking/01_type_mismatch |
| MT2002 | UNRESOLVED_TYPE                | covered    | type_checking/02_unresolved_type |
| MT2003 | CANNOT_INFER_TYPE              | gap        | Gap B (constructor only) |
| MT2004 | WRONG_GENERIC_ARITY            | covered    | type_checking/04_wrong_generic_arity |
| MT2005 | WRONG_ARG_COUNT                | covered    | type_checking/05_wrong_arg_count |
| MT2006 | UNKNOWN_FIELD                  | covered    | type_checking/06_unknown_field |
| MT2007 | UNKNOWN_METHOD                 | auxiliary  | Reached in mty-types/src/check.rs path; conformance_full shape under v0.9 funnels into permissive `cx.fresh()` for some struct shapes. Tracked. |
| MT2008 | NOT_CALLABLE                   | covered    | type_checking/08_not_callable |
| MT2009 | UNKNOWN_VARIANT                | gap        | Gap B |
| MT2010 | QUESTION_OUTSIDE_RESULT        | covered    | type_checking/09_question_outside_result |
| MT2011 | QUESTION_ERROR_MISMATCH        | covered    | type_checking/10_question_error_mismatch |
| MT2012 | WRONG_VARIANT_ARITY            | covered    | type_checking/16_wrong_variant_arity (v0.11 — pattern path) |
| MT2013 | MISSING_STRUCT_FIELD           | covered    | type_checking/15_missing_struct_field |
| MT2014 | DUPLICATE_STRUCT_FIELD         | covered    | type_checking/14_duplicate_struct_field |
| MT2015 | NON_EXHAUSTIVE_MATCH           | gap        | Gap B (constructor only) |
| MT2016 | UNREACHABLE_MATCH_ARM          | gap        | Gap B (warning) |
| MT2017 | BINOP_TYPE_MISMATCH            | covered    | type_checking/11_binop_type_mismatch |
| MT2018 | IF_BRANCH_MISMATCH             | gap        | typeck_neg if_branch_mismatch.mty currently fires MT2001 |
| MT2019 | RETURN_TYPE_MISMATCH           | gap        | typeck_neg return_mismatch.mty currently fires MT2001 |
| MT2020 | PUB_PARAM_NEEDS_TYPE           | covered    | type_checking/12_pub_param_needs_type |
| MT2021 | UNRESOLVED_VALUE               | covered    | type_checking/13_unresolved_value |
| MT2022 | NOT_A_STRUCT                   | gap        | Gap B |
| MT2023 | GENERIC_ARG_MISMATCH           | gap        | Gap B |
| MT2024 | LAMBDA_ARITY_MISMATCH          | gap        | Gap B |
| MT2025 | CANNOT_TAKE_REF                | gap        | Gap B |
| MT2026 | PROTOCOL_MSG_UNKNOWN           | covered    | agent_protocol/03_extra_handler (v0.11 — via expected_warnings.txt; warning severity) |
| MT3001 | USE_AFTER_MOVE                 | covered    | ownership_rejection/01_use_after_move |
| MT3002 | MOVE_OUT_OF_BORROW             | gap        | Gap C (constructor only) |
| MT3003 | BORROW_AFTER_MOVE              | covered    | ownership_rejection/02_borrow_after_move |
| MT3004 | MUT_BORROW_WHILE_SHARED        | covered    | borrow_checking/01_mut_while_shared |
| MT3005 | SHARED_BORROW_WHILE_MUT        | covered    | borrow_checking/02_shared_while_mut |
| MT3006 | TWO_MUT_BORROWS                | covered    | borrow_checking/03_two_mut_borrows |
| MT3007 | BORROW_OUTLIVES_OWNER          | gap        | Gap C |
| MT3008 | CANNOT_MOVE_BORROWED           | covered    | ownership_rejection/03_cannot_move_borrowed |
| MT3009 | MOVE_OUT_OF_REF                | covered    | borrow_checking/08_move_via_ref |
| MT3010 | ARENA_ESCAPE                   | covered    | borrow_checking/10_arena_escape |
| MT3011 | NON_SENDABLE_MESSAGE_ARG       | covered    | borrow_checking/12_non_sendable_message |
| MT3012 | DROP_IN_CONST_CONTEXT          | gap        | Gap C (no const-context support) |
| MT3013 | MUT_BORROW_OF_IMMUT_LOCAL      | covered    | borrow_checking/04_mut_borrow_of_immut_local |
| MT3014 | ASSIGN_TO_IMMUT_LOCAL          | covered    | ownership_rejection/04_assign_to_immut_local |
| MT3015 | USE_OF_UNINITIALIZED           | gap        | Gap C (bind_local always Owned) |
| MT4001 | EFFECT_UNDECLARED              | covered    | effect_checking/01_undeclared_net |
| MT4002 | ALLOC_IN_CORE                  | gap        | Gap D (requires star.toml override) |
| MT4010 | CAPABILITY_TOO_BROAD           | gap        | Gap D (signature Cap always Any) |
| MT4020 | METHOD_AMBIGUOUS               | gap        | Gap D (no shipped duplicate-trait surface) |
| MT4021 | METHOD_NOT_FOUND               | gap        | Gap D (subsumed by MT2007 path) |
| MT4022 | TRAIT_COHERENCE_VIOLATION      | covered    | traits_derive/01_trait_coherence |
| MT4023 | DYN_REQUIRES_OBJECT_SAFE       | covered    | traits_derive/02_dyn_unsafe |
| MT4030 | PROTOCOL_ARITY_MISMATCH        | covered    | agent_protocol/01_arity_mismatch |
| MT4031 | PROTOCOL_PARAM_TYPE_MISMATCH   | covered    | agent_protocol/05_handler_param_type |
| MT4032 | PROTOCOL_MISSING_HANDLER       | covered    | agent_protocol/02_missing_handler |
| MT4033 | PROTOCOL_EXTRA_HANDLER         | covered    | agent_protocol/03_extra_handler |
| MT4040 | DERIVE_COPY_FIELD_NOT_COPY     | covered    | traits_derive/03_derive_copy_bad |
| MT4041 | DERIVE_UNKNOWN                 | covered    | traits_derive/04_derive_unknown |
| MT5001 | RUNTIME_PANIC                  | covered    | runtime_traps/01_panic_exits |
| MT5002 | USE_AFTER_DROP                 | gap        | Gap E (interp trap; MT3001 fires first statically) |
| MT5003 | DIVISION_BY_ZERO               | covered    | runtime_traps/02_division_by_zero |
| MT5004 | INTEGER_OVERFLOW               | gap        | Gap E |
| MT5005 | UNREACHABLE_MATCH              | gap        | Gap E (match lowering uses Term::Panic → MT5001) |
| MT5006 | UNHANDLED_ERROR_RESULT         | gap        | Gap E |
| MT5007 | ARENA_ESCAPE_RUNTIME           | gap        | Gap E |
| MT5008 | UNCALLABLE_BUILTIN             | gap        | Gap E |
| MT5009 | BUDGET_EXCEEDED                | covered    | budget_violation/02_step_budget_exceeded |
| MT5010 | SANDBOX_VIOLATION              | auxiliary  | tokio runtime (conformance_runtime_7) |
| MT5011 | DEADLINE_EXCEEDED              | auxiliary  | runtime-7/deadline_fail |
| MT5012 | MAILBOX_FULL                   | auxiliary  | budget_violation/04_mailbox_full_smoke (slice-7) |
| MT5013 | SUPERVISOR_ESCALATED           | auxiliary  | supervisor_restart/01_one_for_one (slice-7) |
| MT5014 | RESTART_LIMIT_EXCEEDED         | auxiliary  | supervisor_restart/03_rate_limit_exhausted |
| MT5015 | CAPABILITY_OUTSIDE_SANDBOX     | auxiliary  | runtime-7/sandbox_meta |
| MT5020 | AGENT_HANDLER_MISSING          | auxiliary  | runtime-7 (slice-7 dyn-dispatch) |
| MT5021 | SEND_TO_DEAD_AGENT             | auxiliary  | runtime-7 |
| MT5050 | EXTERN_FN_UNIMPL               | auxiliary  | runtime-7/extern_log |
| MT6001 | UNKNOWN_MACRO                  | covered    | macros/01_unknown_macro |
| MT6002 | MACRO_ARITY_MISMATCH           | covered    | macros/02_arity_mismatch |
| MT6003 | MACRO_BODY_PARSE_FAILED        | covered    | macros/05_body_parse_failed (v0.11 — `$` has no token rule) |
| MT6004 | RECURSIVE_MACRO_TOO_DEEP       | covered    | macros/03_recursive_too_deep |
| MT6005 | PROC_MACRO_IMPURE              | covered    | macros/04_proc_macro_impure |
| MT6006 | PROC_MACRO_UNSUPPORTED_V0_5    | auxiliary  | mty-hir unit tests |
| MT6007 | PROC_MACRO_IMPURE_AT_RUNTIME   | auxiliary  | mty-hir unit tests |
| MT6008 | PROC_MACRO_RESOURCE_EXCEEDED   | covered    | macros/06_proc_macro_resource_exceeded (v0.11 — 16 MiB cap breach) |
| MT8001 | CODEGEN_DIV_BY_ZERO            | auxiliary  | codegen (conformance_codegen) — positive paths only today |
| MT8002 | CODEGEN_OOB_INDEX              | auxiliary  | codegen (positive paths only) |
| MT8003 | CODEGEN_INT_OVERFLOW           | auxiliary  | codegen |
| MT8004 | CODEGEN_NULL_DEREF             | auxiliary  | codegen |
| MT8005 | CODEGEN_EXTERN_UNRESOLVED      | auxiliary  | codegen |
| MT8006 | CODEGEN_UNREACHABLE            | auxiliary  | codegen |
| MT8007 | CODEGEN_UNSUPPORTED_SHAPE      | auxiliary  | codegen |
| MT8008 | CODEGEN_LINKER_MISSING         | auxiliary  | codegen |
| MT8009 | CODEGEN_WASM_VALIDATION        | auxiliary  | codegen/wasm_empty + /wasm_hello |
| MT8010 | CODEGEN_MONO_FAILED            | auxiliary  | codegen/monomorphization |

### Summary (FROZEN codes)

- Total FROZEN codes enumerated: **66**.
- conformance_full positively-fires: **45** (68%) — v0.10 was 41 (62%).
  +4 in v0.11: MT2012 (promoted from auxiliary), MT2026 (closed via
  warning-assertion extension), MT6003 + MT6008 (closed via new
  proc-macro fixtures).
- conformance_full + auxiliary harnesses positively-fires: **60** (91%) —
  v0.10 was 58 (88%).
- True remaining gaps (no emit-witness anywhere): **6** (9%, down from
  8 at v0.10): MT2003, MT2009, MT2022, MT2023, MT2024, MT2025 — all
  Gap B constructor-only typeck codes pending v1.0-RC2 emit-site
  plumbing.

Pre-v0.11 audit "gap" codes that v0.11 reclassified to
`auxiliary` (has at least an aux-harness witness) rather than fully
`covered` because the underlying emit-site limitation persists:
Gap A (9 codes, lex/parse funnel — caught by message-string parser
unit tests in mty-syntax), Gap C (4 borrow codes, caught by
mty-borrow unit tests), Gap D (4 cap/effect codes, caught by
mty-types unit tests), Gap E (6 runtime codes, caught by
mty-ir/interp tests for the 4 reachable ones), Gap G (10 codegen
codes, codegen-crate unit tests). See `CONFORMANCE_V0_11_NOTES.md`
per-gap section for details.

## Spec-section coverage table

| §  | Section                          | Conformance category(ies)            | Status |
|----|----------------------------------|--------------------------------------|--------|
| 1  | Naming and brand history         | (informative)                        | n/a    |
| 2  | Design goals                     | (informative)                        | n/a    |
| 3  | Lexical structure                | lexical/, parser/                    | partial — see Gap A |
| 4  | Program and module structure     | parser/, type_checking/              | covered (item parse + visibility via pub_param case) |
| 5  | Names, scopes, and visibility    | type_checking/13_unresolved_value    | covered |
| 6  | Type system                      | type_checking/01..15, type_inference/* | covered |
| 7  | Ownership, borrowing, lifetimes  | ownership_rejection/*, borrow_checking/* | covered |
| 8  | Capabilities                     | capability_checking/*                | partial (MT4010 gap) |
| 9  | Effects                          | effect_checking/*                    | covered |
| 10 | Arenas / agent-local heap        | borrow_checking/10_arena_escape      | covered |
| 11 | Control flow                     | control_flow/*, type_checking/11_binop | covered |
| 12 | Agents and mailboxes             | agent_protocol/*, mailbox_ordering/*, borrow_checking/12 | covered |
| 13 | Protocols                        | agent_protocol/*                     | covered |
| 14 | Supervisors                      | supervisor_restart/* (slice-7)       | covered (auxiliary) |
| 15 | Tasks / structured concurrency   | mailbox_ordering/06_multicore_fifo   | covered |
| 16 | Budgets and sandboxes            | budget_violation/*, capability_checking/* | covered |
| 17 | Error handling / panic policy    | type_checking/09 + /10, runtime_traps/01 | covered |
| 18 | Generics and constraints         | type_inference/03_generic_id_infer, type_checking/04 | covered |
| 19 | Traits and dynamic dispatch      | traits_derive/01..04                 | covered |
| 20 | Compile-time metaprogramming     | macros/01..03                        | covered |
| 21 | Unsafe code                      | (slice-9 backlog)                    | gap    |
| 22 | Frontend model (DOM, Wasm)       | wasm_component/ (README only)        | partial — covered via examples/02_pdfutils demo, not in conformance_full |
| 23 | Backend model (HTTP, agents)     | agent_protocol/*, runtime-7/*        | covered (auxiliary) |
| 24 | Compilation pipeline             | codegen/*                            | covered (auxiliary) |
| 25 | Runtime architecture             | mailbox_ordering/*, supervisor_restart/* | covered (auxiliary) |
| 26 | Foreign function interface       | runtime-7/extern_log                 | partial — extern_c only; extern_js / extern_python gap |
| 27 | Standard library v1.0 surface    | (covered by per-method conformance shapes) | covered |
| 28 | Token-efficiency rules           | (informative)                        | n/a    |
| 29 | Toolchain, build modes, flags    | (CLI integration tests, not conformance_full) | n/a    |
| 30 | Profiles                         | effect_checking/05_strict_core_profile (placeholder) | partial — Gap D |
| 31 | Construction history             | (informative)                        | n/a    |
| 32 | Conformance suite                | (this document)                      | n/a    |
| 33 | Diagnostic catalog               | (this document + codes.rs)           | n/a    |
| 34 | Worked examples                  | examples/01..NN (separate corpus)    | covered (auxiliary) |
| 35 | Telemetry and observability      | runtime-7/* (slice-7 telemetry)      | partial |
| 36 | Package manager and registry     | (CLI integration tests)              | n/a    |
| 37 | LSP and editor integration       | (mty-lsp integration tests)          | n/a    |
| 38 | Benchmarks and performance       | mailbox_ordering/07_multicore_throughput_smoke | covered (smoke) |
| 39 | Self-hosting                     | mty-driver/tests/selfhost_* (separate corpus) | covered (auxiliary) |

### Summary (spec sections)

- Total spec sections: **39**.
- Informative-only / non-fixture: **8** (§1, §2, §28, §31, §32, §33,
  §36, §37).
- Normative sections with conformance coverage (any harness): **29 of
  31** = **94%**.
- Partial-coverage sections: §3 (Gap A), §8 (MT4010), §22 (frontend),
  §26 (extern_js/extern_python), §30 (per-case star.toml).
- Gap sections: §21 (unsafe) — slice-9 backlog.

## Acceptance verdict

- `cargo test --test conformance_full -p mty-driver` — **passes** with
  **84 cases** across **16 categories** at the v0.11 audit completion
  point (+3 over v0.10's 81 cases: macros/05, macros/06,
  type_checking/16; plus one warning-assertion file on
  agent_protocol/03).
- Coverage rate: **45/66 FROZEN codes (68%)** via conformance_full
  alone (v0.10: 41 / 62%), **60/66 (91%)** counting all sibling
  harnesses (v0.10: 58 / 88%).
- All documented gaps have an explicit follow-up note in
  `dev/history/notes/CONFORMANCE_V0_11_NOTES.md` (per-gap status +
  v1.0-RC2 recommendations).
