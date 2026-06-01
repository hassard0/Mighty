# `dev/history/` — index

Working artifacts preserved from the slice-driven Stardust → Mighty
build. Grouped by category; one-line summary of each file so readers
can navigate without opening anything they don't need.

## Slice plans (`slices/`)

The slice-level work plans. Each file scopes a slice (or version
slice), enumerates exit criteria, and tracks what shipped / deferred.

| File | Slice |
|---|---|
| `SLICE1.md` | Parser, formatter, HIR, CLI, examples (v0.1 phase 1) |
| `SLICE2.md` | Per-node formatter, lambdas, if-let, turbofish |
| `SLICE3.md` | Type checker, generics MVP, `?`-propagation |
| `SLICE4.md` | Borrow / ownership / affine / arena |
| `SLICE5.md` | Effects, capabilities, traits, derives |
| `SLICE6.md` | MtyIR + interpreter |
| `SLICE7.md` | Runtime MVP (scheduler, mailboxes, supervisors) |
| `SLICE8.md` | Cranelift + Wasm backends |
| `SLICE_V0_2.md`, `SLICE_V0_2_PKG.md` | v0.2 omnibus + pkg sub-slice |
| `SLICE_V0_3.md` | v0.3 soundness hardening |
| `SLICE_V0_4.md` | v0.4 dogfood + ecosystem |
| `SLICE_V0_5.md` | v0.5 self-hosting + dogfood completion |
| `SLICE_V0_6.md` | v0.6 multi-core + benchmarks + self-host parser |
| `SLICE_V0_8.md` | v0.8 loose-ends + self-host HIR + perf + spec RC |
| `SLICE_V0_9.md` | v0.9 RC-prep + freeze-readiness |

(There is no `SLICE_V0_7.md`: v0.7 was a naming-only rebrand release
captured in the notes directory.)

## Release notes (`releases/`)

The unabridged per-release notes; the repo-root
[`CHANGELOG.md`](../../CHANGELOG.md) summarises each one.

The table below covers v0.1–v0.9 (the slice-driven era). For v0.10
and later (the track-driven era) consult the individual
`RELEASE-v0.NN.md` files alongside the
[CHANGELOG entries](../../CHANGELOG.md). Highlights since v0.9:

- v0.18 — replay (byte-identical) + cluster (Tier 4.1) + cabi_realloc extract
- v0.26–v0.30 — LLM-agent stack (`std.llm`, `std.swarm`, `std.memory`,
  taint types, `std.observe`, computer use)
- v0.31–v0.34 — VS Code v2 (cost CodeLens, DAP debug, quick-fix lightbulb),
  `mty find`, `mty hooks`, fix engines for 80+ MTxxxx codes
- v0.35 — PGO release binaries (3 platforms), multi-arch Docker, homebrew
  runbook, stdlib hover-catalog drift gate
- v0.36 — native codegen fixes (U8 + dynamic log), `extern c` + extern_libs,
  String position/range ops, Stardust→Mighty rename compat finished, Windows
  `cli-min` install + macOS PGO re-enabled
- v0.42 — Mighty IDE blocker closure: numeric casts, parser diagnostics,
  native computed `log()`, formatter safety, and Vec-liveness regression locks
- v0.43 draft — IDE dogfooding correctness rollup: short-circuit lowering,
  interpreter mutator writeback, top-level `const` formatting, prefix-call
  parsing, and native link diagnostics
- v0.44 draft — public Mighty milestone versioning, parser resilience for
  long agent-generated `else if` command ladders, and default `mty run`
  fallback for host-backed `std.fs` calls

| File | Headline |
|---|---|
| `RELEASE-v0.1.md` | First feature-complete release (376 tests) |
| `RELEASE-v0.2.md` | LSP, pkg, doc, DWARF, Wasm CM, stdlib (550 tests) |
| `RELEASE-v0.3.md` | Soundness hardening (623 tests) |
| `RELEASE-v0.4.md` | Dogfood + ecosystem (692 tests) |
| `RELEASE-v0.5.md` | Self-hosting lexer + dogfood completion (839 tests) |
| `RELEASE-v0.6.md` | Multi-core + benchmarks + self-host parser (885 tests) |
| `RELEASE-v0.7.md` | Stardust → Mighty rebrand (885 tests, byte-identical) |
| `RELEASE-v0.8.md` | Loose-ends + self-host HIR + perf + spec v1.0-RC (927 tests) |
| `RELEASE-v0.9.md` | Spec v1.0-RC2 + fuzz + self-host MtyIR (955 tests) |

## Workstream notes (`notes/`)

Per-workstream agent notes — interpretation calls, gap catalogs,
follow-ups, benchmarking call-outs.

| File | Topic |
|---|---|
| `BENCHMARKS_V0_6_NOTES.md` | v0.6 benchmark interpretation calls |
| `BENCHMARKS_V0_8_NOTES.md` | v0.8 benchmark interpretation calls |
| `BORROW_V0_3_NOTES.md` | Borrow-checker v0.3 (NLL + field Places) notes |
| `CLEANUP_V0_10_NOTES.md` | v0.10 production-grade replacement of v0.9 stubs |
| `CODEGEN_V0_2_NOTES.md` | v0.2 codegen-completion interpretation calls |
| `CONFORMANCE_V0_3_NOTES.md` | v0.3 conformance triage |
| `CONFORMANCE_V0_10_NOTES.md` | v0.10 conformance audit + coverage report |
| `DEBUGINFO_V0_2_NOTES.md` | DWARF + wasm source-map interpretation notes |
| `DEMOS_V0_4_NOTES.md` | v0.4 dogfood-demo implementation notes |
| `DOC_V0_2_NOTES.md` | mty-doc interpretation calls |
| `DOGFOOD_V0_5_NOTES.md` | v0.5 dogfood-completion notes |
| `EFFECTS_V0_3_NOTES.md` | v0.3 soundness-hardening interpretation log |
| `FUZZ_V0_9_NOTES.md` | v0.9 fuzz harness + bug bash |
| `LOOPS_V0_5_NOTES.md` | v0.5 loop / break / continue agent notes |
| `LOOSE_ENDS_V0_8_NOTES.md` | v0.8 4-of-5 loose-end closures |
| `LSP_V0_5_NOTES.md` | LSP advanced (semantic tokens, rename, etc.) |
| `MACROS_V0_4_NOTES.md` | Hygienic declarative macros v0.4 |
| `MACROS_V0_5_NOTES.md` | Macros completion v0.5 (`name!(args)`, hygiene) |
| `PARSER_AUDIT_V0_9.md` | v0.9 non-progress-guard family fix + audit sweep |
| `PERF_V0_8_NOTES.md` | v0.8 perf agent log (parse +27%, mailbox +7%) |
| `POLISH_V0_10_NOTES.md` | v0.10 CI hardening + mkdocs `--strict` cleanup |
| `RC_PREP_V0_9_NOTES.md` | v0.9 RC-prep overnight swarm agent log |
| `REBRAND_NOTES.md` | Stardust → Mighty v0.7 rebrand interpretation log |
| `REGISTRY_V0_4_NOTES.md` | GH-Releases registry transport v0.4 |
| `RENAME_LOG.md` | The full identifier-rename log for the v0.7 rebrand |
| `RUNTIME_V0_3_NOTES.md` | Runtime v0.3 (mid-turn cancel + OTLP + slab pool) |
| `SCHEDULER_V0_6_NOTES.md` | v0.6 multi-core scheduler interpretation |
| `SELFHOST_HIR_V0_8_NOTES.md` | v0.8 self-host HIR + minimal typeck |
| `SELFHOST_IR_V0_9_NOTES.md` | v0.9 self-host MtyIR + bootstrap test |
| `SELFHOST_PARSER_V0_6_NOTES.md` | v0.6 self-host parser phase notes |
| `SELFHOST_V0_4_NOTES.md` | v0.4 self-host lexer + language gap catalog |
| `SELFHOST_V0_10_NOTES.md` | v0.10 self-host examples 04+05 closures |
| `SPEC_CONSOLIDATION_V0_8_NOTES.md` | v0.8 spec consolidation → v1.0-RC |
| `SPEC_FREEZE_V0_9_NOTES.md` | v0.9 spec freeze prep → v1.0-RC2 + 6 RFCs |
| `STDLIB_V0_2_NOTES.md` | v0.2 real stdlib implementation notes |
| `V0_2_CLEANUP_NOTES.md` | v0.3 cleanup-of-v0.2 follow-ups |
| `WASM_CM_V0_2_NOTES.md` | v0.2 Wasm Component Model integration notes |

## Original brainstorming / spec docs (`superpowers/docs/`)

The pre-implementation plan + spec documents that guided slices 1-8.
These came out of the brainstorming-skill flow and are read-only
historical references; the code they describe has since shipped.

| Subdir | Contents |
|---|---|
| `plans/` | Eight slice plans (slice1 phase1 through slice8 codegen) |
| `specs/` | The matching spec/design docs |
