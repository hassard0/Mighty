# Mighty v0.13 — Release Notes

**Tag:** `v0.13.0`
**Date:** 2026-05-25
**Status:** SHIPPED — end-to-end self-hosting complete + WASI Preview 2
backend + effect-row polymorphism + set-of-scopes macro hygiene
(2 new RFCs).

v0.13 is the **end-to-end self-host milestone**: the Mighty compiler
front-end + Wasm core-module back-end is now implemented in Mighty
source across the slice-1-supported subset. The bootstrap chain runs
lexer → parser → HIR → typeck → MtyIR → wasm codegen, all in `.mty`,
producing a structurally valid Wasm core module for examples 01
("hello, Mighty"), 02 (`add`), and a synthetic arithmetic fixture.
Alongside the self-host capstone, v0.13 ships three more workstreams:
**WASI Preview 2** as an opt-in backend (`--wasi=p2`) with
user-supplied WIT support (`[wit]` in `mighty.toml`); a row-polymorphism
infrastructure (RFC-008) for the type system with the first wired stdlib
HOF (`List.map`); and a Flatt-style set-of-scopes macro hygiene layer
(RFC-009) alongside the legacy mangling-based expander.

**Headline:** Mighty now compiles Mighty all the way through to Wasm —
the self-host milestone called for since the v0.5 lexer port is
reached. Two new RFCs (RFC-008 effect rows; RFC-009 set-of-scopes
hygiene) land as `docs/spec/rfcs/RFC-008-*.md` and `RFC-009-*.md`,
with full infrastructure in `mty-types` / `mty-macros` and v0.14
deferred work scoped to integration (surface-syntax parser, mty-hir
wiring, rest of stdlib HOFs). WASI Preview 2 is available behind
`--wasi=p2` with a vendored `wasi:*@0.2.3` WIT slice covering
`cli`/`io`/`clocks`/`filesystem`/`http`/`random`; preview1 remains the
default.

If you were on v0.12.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force`. There are no source-level breaking
changes for end-user Mighty programs; the new `--wasi=p2` /
`--world <name>` / `[wit]` surfaces are strictly additive.

## Highlights

- **End-to-end self-host milestone reached.** `selfhost/codegen/wasm.mty`
  (~400 LOC of Mighty) emits valid Wasm core modules for the slice-1
  subset: `i32`/`i64`/`f32`/`f64` const, all arithmetic + comparison
  BinOp/UnOp, `if`/`loop`/`br`, `call`, `local.get`/`local.set`,
  `return`, `unreachable`. The chain
  lexer → parser → HIR → typeck → MtyIR → wasm is now all in `.mty`.
  `cargo test -p mty-driver --test selfhost_codegen` reports
  **6/6 live tests pass + 1 ignored** (example 03 — generic
  `Option[T]` lowering is deferred). The Mighty source owns the
  *algorithm* (which sections, which type signatures, which
  instructions) and uses a host bridge for the byte-level
  `wasm-encoder` calls — the same architectural pattern v0.5 / v0.6 /
  v0.8 / v0.9 / v0.10 adopted. See
  [`SELFHOST_CODEGEN_V0_13_NOTES.md`](../notes/SELFHOST_CODEGEN_V0_13_NOTES.md).
- **WASI Preview 2 backend + user-WIT.** The wasm codegen gets a new
  `--wasi=p1|p2` flag (default `p1`) and a `--world <name>` flag.
  `crates/mty-codegen-wasm/src/preview2.rs` builds a P2 WIT document
  + component wrapper; `crates/mty-codegen-wasm/wit/wasi-p2/wasi-p2.wit`
  vendors a minimal 0.2.3 slice covering `wasi:cli`, `wasi:io`,
  `wasi:clocks`, `wasi:filesystem`, `wasi:http`, `wasi:random`. A new
  `[wit]` section in `mighty.toml` (`world = "..."`, `files = [...]`)
  is resolved by `crates/mty-pkg/src/wit_resolve.rs` and merged into
  the emitted component. 18 new tests (9 integration + 5 wit_resolve
  + 2 driver + 2 preview2-unit). Example program at
  `examples/21_wasi_preview2.mty` + `wit/example/hello-world.wit`;
  user-facing matrix in `docs/reference/wasi.md`. v0.14 deferred:
  stdlib lowerings to real P2 imports, preview1-adapter embed,
  default flip to P2, custom component exports. See
  [`WASI_P2_V0_13_NOTES.md`](../notes/WASI_P2_V0_13_NOTES.md).
- **Effect-row polymorphism (RFC-008) — SHIPPED-SUBSET.** RFC-008
  published at `docs/spec/rfcs/RFC-008-effect-rows.md` covers
  motivation, syntax (`!E`, `!{a | E}`), four-case unification rules,
  subsumption, anti-patterns, diagnostics, and open questions
  (effect handlers deferred to a future RFC). The infrastructure
  lands in `crates/mty-types/src/effects.rs::row` (~450 LOC):
  `EffectRow` (closed / open), `RowVar`, `RowSubst` (with
  occurs-check + chain-resolve), `unify_rows()` (full 4-case),
  `subsume_closed()`, `RowPolySig` for row-polymorphic signatures,
  `instantiate_row_sig()` for call-site freshening, `pretty_row()`.
  `stdlib_list_map_sig()` returns the relaxed
  `fn[A, B, E](xs: List[A], f: fn(A)->B!E) -> List[B]!E`. **23 new
  tests** (mty-types 44 → 67). v0.14 follow-ups: surface-syntax
  parser for `!E`, call-site validator wired into typeck, the rest
  of the stdlib HOFs (`filter` / `fold` / `for_each` / `find`),
  MT4020-25 diagnostics. See
  [`EFFECT_ROW_V0_13_NOTES.md`](../notes/EFFECT_ROW_V0_13_NOTES.md).
- **Set-of-scopes macro hygiene (RFC-009) — SHIPPED-SUBSET.** RFC-009
  published at `docs/spec/rfcs/RFC-009-set-of-scopes.md` documents the
  Flatt-style "Bindings as Sets of Scopes" model. New modules in
  `mty-macros`: `scopes.rs` (`Scopes`, `ScopeId`, `ScopeGen`,
  largest-subset `resolve()` with explicit `ResolveAmbiguity` error),
  `hygiene.rs` (`ScopedTok`, `HygieneEnv` with `apply_to_body` /
  `apply_to_argument`, `strip_scopes`), and a new
  `expand::expand_scoped()` entry point returning
  `ScopedExpansion { tokens, bindings, intro }` alongside the legacy
  `expand()`. **+28 macro tests** (12 sets-of-scopes integration +
  2 parity assertions + carry-over expander tests). `mty-hir` still
  consumes the legacy mangling-based expander — wiring is a v0.14
  follow-up. See
  [`MACRO_HYGIENE_V0_13_NOTES.md`](../notes/MACRO_HYGIENE_V0_13_NOTES.md).
- **No spec promotions, no behaviour changes for existing code.** The
  spec stays at v1.0-RC3 (RFC-008 and RFC-009 are pre-freeze drafts
  in the RFC tree, not normative-spec promotions). v0.13 is a
  capability and infrastructure release: it lands a self-host
  capstone, a new codegen surface, and two RFC drafts with usable
  infrastructure, all without removing any existing surface.
- **All gates green, test count grows to 1051 + 137 + 89 + 46.**
  **1051 Rust tests + 137 Python tests + 89 conformance cases + 46
  self-host tests = 1323 passing**, 0 failing, 5 ignored (3 conformance
  carryovers + 1 cargo-doc-test + 1 self-host example 03). All four
  demos pass (`smoke.sh` is 4/4). Clippy strict + fmt clean.

## What's new

### Self-host codegen — end-to-end milestone

The v0.5 lexer port called for the eventual goal of "Mighty
compiles Mighty"; v0.6 (parser), v0.8 (HIR + typeck), v0.9 (IR sink),
and v0.10 (IR sink refinements) each closed one more phase. v0.13
closes the last front-end phase by porting the Wasm core-module
emitter to Mighty:

- **Source.** `selfhost/codegen/wasm.mty` (~400 LOC) +
  `selfhost/codegen/lib.mty` (73 LOC package + intent doc). Both
  `mty check` clean.
- **Driver test.** `crates/mty-driver/tests/selfhost_codegen.rs`
  (~1500 LOC) drives the Mighty source through the bootstrap chain
  and asserts the resulting Wasm module is structurally valid.
- **Coverage.** 6 live tests pass (`selfhost_codegen_compiles`,
  `selfhost_codegen_lib_compiles`, `selfhost_codegen_hello_world`,
  `selfhost_codegen_example_01`, `selfhost_codegen_example_02`,
  `selfhost_codegen_arith_fixture`) + 1 ignored
  (`selfhost_codegen_example_03` — generic `Option[T]` lowering is
  v0.14).
- **Architectural pattern.** Mighty source owns the *algorithm*
  (which Wasm sections, which type signatures, which instructions
  to emit); a host bridge handles byte-level `wasm-encoder` calls
  (LEB128, section framing, instruction encoding). Same pattern
  used in v0.5 / v0.6 / v0.8 / v0.9 / v0.10. This is the deliberate
  choice that keeps the v0.13 slice within budget: a from-scratch
  byte-level emitter in Mighty would require `Vec[U8]` push, bitwise
  ops on `U8`, `String → bytes()`, and an LEB128 stdlib — all
  pre-requisites that don't yet exist in the v0.12 Mighty stdlib.

**v0.14 deferred work** (per `SELFHOST_CODEGEN_V0_13_NOTES.md`):
string pool emission, pattern lowering, ADT layout, for-loop
iter lowering, agent backend, Cranelift + LLVM backends. The
Wasm front-end-through-back-end self-host chain is now complete
for the slice-1 subset.

### WASI Preview 2 backend + user-WIT

The wasm codegen gains a Preview 2 path opt-in via `--wasi=p2`. The
default stays `--wasi=p1` so existing builds are unchanged.

- **Driver surface.** `crates/mty-driver/src/build.rs` now carries a
  `WasiPreview { P1, P2 }` enum on `BuildOptions`, plus optional
  `user_wit: Option<LoadedUserWit>`. `build_wasm` dispatches into the
  P1 or P2 emitter accordingly.
- **CLI surface.** `crates/mty-cli/src/cmd/build.rs` exposes
  `--wasi <p1|p2>` and `--world <name>`. The CLI walks up from the
  source file to locate `mighty.toml` to read any `[wit]` section.
- **P2 emitter.** `crates/mty-codegen-wasm/src/preview2.rs` builds the
  WIT document (combining the vendored WASI slice + any user WIT),
  emits the core module via the existing `emit.rs`, and wraps it as
  a Component Model component with `wit-component::ComponentEncoder`.
- **Vendored WASI 0.2.3 slice.** `crates/mty-codegen-wasm/wit/wasi-p2/`
  contains a hand-rolled WIT covering the interfaces Mighty's
  generated code may reference: `wasi:cli`, `wasi:io`, `wasi:clocks`,
  `wasi:filesystem`, `wasi:http`, `wasi:random` — all at `@0.2.3`.
- **User WIT in `mighty.toml`.** New `[wit]` section:
  `world = "my-world"`, `files = ["path/to/user.wit"]`.
  `crates/mty-pkg/src/wit_resolve.rs` reads and loads them into a
  `LoadedUserWit` passed to the driver.
- **Example + docs.** `examples/21_wasi_preview2.mty` +
  `wit/example/hello-world.wit` + `docs/reference/wasi.md` (the
  user-facing compatibility matrix + `[wit]` authoring guide).

**Interpretation calls.** The v0.13 P2 component declares a small
internal `mighty:cli-adapter` package so that the unchanged P1-shape
`wasi:cli/log#log` import emitted by `emit.rs` is acceptable to
`wit-component`. Strict P2 hosts (those that reject non-WASI imports)
will refuse to instantiate the component; the wasmtime smoke test is
gated behind a `wasmtime_p2_smoke` cargo feature (off by default) and
the constraint is documented in `docs/reference/wasi.md`. The v0.14
lowering pass will replace the shim with real
`wasi:cli/stdout#print` calls, at which point P2 can become the
default.

**Tests.** 18 new tests:

- `crates/mty-codegen-wasm/tests/preview2.rs` — 9 integration tests
  (P2 round-trip, versioned-import assertion, user-WIT merge, error
  paths).
- `crates/mty-pkg/src/wit_resolve.rs` — 5 unit tests.
- `crates/mty-driver/src/build.rs` — 2 driver-dispatch tests.
- `crates/mty-codegen-wasm/src/preview2.rs` — 2 in-module unit
  tests.

**v0.14 follow-ups:** stdlib lowerings to real P2 imports
(`wasi:cli/stdout#print`, `wasi:filesystem/*`, etc.); embed the
preview1-adapter so the P2 component is universally instantiable;
flip the default from P1 to P2; surface custom component exports
for embedder use cases. See
[`WASI_P2_V0_13_NOTES.md`](../notes/WASI_P2_V0_13_NOTES.md).

### Effect-row polymorphism (RFC-008)

The Mighty type system has carried effect tracking since v0.3 (the
`!{io, fs}` annotations on function types). v0.13 introduces the
infrastructure to lift effects from concrete labels to row-polymorphic
shapes — the prerequisite for stdlib HOFs to thread caller-supplied
effects without losing information.

- **RFC-008** at `docs/spec/rfcs/RFC-008-effect-rows.md`. Motivation
  (the current concrete-only effect surface forces
  `List.map(f: fn(A) -> B)` to widen `f`'s effects to `Empty`,
  losing them entirely), syntax (`!E` for an effect variable;
  `!{a | E}` for an open row with `a` and rest `E`), four-case
  unification rules (closed/closed, closed/open, open/closed,
  open/open with shared fresh tail), subsumption rule
  (sub-row only between closed rows), anti-patterns (no Σ-types,
  no first-class rows on data), diagnostics
  (MT4020-MT4025 reserved), open questions (effect handlers
  deferred).
- **Infra in `mty-types`.** `crates/mty-types/src/effects.rs::row`
  (~450 LOC) exports:
  - `EffectRow` enum: `Closed(BTreeSet<EffectId>)` /
    `Open(BTreeSet, RowVar)`.
  - `RowVar(u32)` newtype.
  - `RowSubst` substitution table: `fresh()`, `bind()` with
    occurs-check, `lookup()`, recursive `resolve()`.
  - `RowError`: `ClosedMismatch`, `SubsumptionFail`, `Occurs`.
  - `unify_rows()`: full 4-case unification.
  - `subsume_closed()`: closed-into-closed sub-row check.
  - `RowPolySig` + `RowSpec`: row-polymorphic fn signatures with
    de-Bruijn-style row-var indices.
  - `instantiate_row_sig()`: call-site freshening into a
    `RowSubst`.
  - `pretty_row()`: diagnostic-quality rendering.
  - `stdlib_list_map_sig()`: the relaxed `List.map` signature
    `fn[A, B, E](xs: List[A], f: fn(A)->B!E) -> List[B]!E`.
- **`EffectId` Ord/PartialOrd derives.** Tiny additive derives in
  `crates/mty-types/src/ty.rs` so `BTreeSet<EffectId>` works.
- **Tests.** 23 new (12 unit `effects::row_tests::row_arith_01..12`
  + 11 integration `tests/effects_row.rs`). `mty-types` test count
  moves 44 → 67.

**v0.14 follow-ups** (per `EFFECT_ROW_V0_13_NOTES.md`):
surface-syntax parser for `!E` / `!{a | E}` in `mty-syntax`;
call-site validator wired into `mty-types/src/check.rs`; rest of
the stdlib HOFs (`filter`, `fold`, `for_each`, `find`); the
MT4020-25 diagnostic messages.

### Set-of-scopes macro hygiene (RFC-009)

Mighty's declarative-macro expander has used a mangling-based
hygiene scheme since v0.4. v0.13 lands the first phase of the
Flatt-style "Bindings as Sets of Scopes" model — alongside the
legacy expander rather than replacing it — so a future v1.x can
switch over without breaking any current callers.

- **RFC-009** at `docs/spec/rfcs/RFC-009-set-of-scopes.md`. Documents
  the data model (scopes are unforgeable u32 ids; bindings carry the
  scope set in which they were introduced; references resolve by
  largest-subset match), the entry-point shape
  (`expand_scoped(def, args, gen, def_scopes, caller_arg_scopes)`),
  and the migration plan (legacy expander stays; new entry-point
  lives alongside; v1.x can flip the default).
- **`crates/mty-macros/src/scopes.rs`** (NEW). `ScopeId(u32)`,
  `Scopes` (BTreeSet wrapper with `with` / `without` / `is_subset`
  / `intersect` / `union`), `ScopeGen` (monotonic allocator, skips
  `0`), `resolve` (largest-subset picker with explicit
  `ResolveAmbiguity` error).
- **`crates/mty-macros/src/hygiene.rs`** (NEW). `ScopedTok` (token
  + scope set), `HygieneEnv` (per-invocation hygiene environment
  with `apply_to_body` / `apply_to_argument` helpers),
  `strip_scopes`.
- **`crates/mty-macros/src/expand.rs`** (EXTENDED). New
  `expand_scoped(...)` entry point returning
  `ScopedExpansion { tokens, bindings, intro }`. The legacy
  `expand` / `expand_to_source` are unchanged.
- **`crates/mty-macros/src/lib.rs`** (RE-EXPORTS). `Scopes`,
  `ScopeId`, `ScopeGen`, `resolve`, `ResolveAmbiguity`,
  `HygieneEnv`, `ScopedTok`, `strip_scopes`, `expand_scoped`,
  `ScopedExpansion`.
- **Tests.** +28 new tests:
  `crates/mty-macros/tests/sets_of_scopes.rs` (12 integration:
  identity, let-introduction, swap macros, recursion, let-binding
  composition, global names, inner shadowing, ambiguity reporting,
  parameter-scope preservation, cross-macro reference resolution,
  allocator monotonicity, definition-scope propagation) plus 2 parity
  checks (scoped expansion matches legacy expander output;
  bindings list is correctly populated) plus carry-over expander
  tests.

**Out of scope for v0.13** (per `MACRO_HYGIENE_V0_13_NOTES.md`):
mty-hir still calls the legacy `expand_to_source` path; rewiring it
to drive `expand_scoped` is a v0.14 task. The set-of-scopes layer
is exercised exclusively by macro-crate tests in v0.13.

## v1.0 freeze: blockers + proposed date

The v1.0 spec is at v1.0-RC3 (unchanged from v0.12). Two of the three
independent implementations are in the repo. v0.13 does not move any
freeze blocker forward; it lands new infrastructure (self-host
codegen + WASI P2 + 2 RFC drafts) that, when promoted in v0.14+, will
*add* surface to v1.0 rather than *remove* blockers. Blocker status
(delta vs v0.12 italicised — there are no deltas):

1. **Two independent implementations.** Rust reference compiler,
   Python 2nd-impl (`impl-py/`, 137 tests, 20/20 examples lex+parse),
   Go 3rd-impl (`impl-go/`, 4848 LOC, source-only — Go toolchain
   absent on build host so `go test ./...` not yet run). Unchanged
   from v0.12.
2. **RFC comment periods.** RFC-001 through RFC-006 each need a
   30-day public window. RFC-008 and RFC-009, freshly landed in
   v0.13, also need to age before they can promote into the
   normative spec. Unchanged from v0.12.
3. **Published normative conformance suite.** The corpus stands at
   89 cases / 16 categories / 3 ignored. Coverage of FROZEN
   diagnostic codes remains ~92%. Unchanged from v0.12.

**Proposed v1.0 freeze date: 2026-09-01** (unchanged).

## Backwards-compat aliases (status)

Unchanged from v0.12. All v0.7 + v0.8 aliases (`mty dump --sir`
alias of `--ir`; legacy `SD####` accepted by `mty explain`;
`--legacy-interp`; legacy `sd`/`stardust` code-block tags) stay
live. The new v0.13 surfaces are additive: `--wasi=p2`,
`--world <name>`, `[wit]` in `mighty.toml`, `expand_scoped`
alongside the legacy `expand`.

## Stats

| | v0.12.0 | v0.13.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Rust tests passing | 977 | **1051** | **+74** |
| Python tests passing | 135 | **137** | **+2** |
| Self-host tests | 40 | **46** | **+6** |
| Conformance cases | 89 | **89** | 0 |
| Conformance ignored | 3 | **3** | 0 |
| Combined test count | 1241 | **1323** | **+82** |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes | 67+ | 67+ | 0 |
| Examples passing (check) | 20/20 | **21/21** | **+1** (`21_wasi_preview2`) |
| Demos passing | 4/4 | **4/4** | 0 |
| Independent implementations | 3 (front-end only) | **3 (front-end only)** | 0 |
| Spec | v1.0-RC3 | **v1.0-RC3** | 0 |
| Spec amendments | 88 | 88 | 0 |
| RFCs | 6 | **8** | **+2** (RFC-008 + RFC-009) |
| Fuzz targets | 4 | 4 | 0 |
| CI jobs (all required) | 6 | 6 | 0 |
| Commits since prior tag | 5 | **5** | — |
| Lines changed since prior tag | 51 files, +7 368 / -59 | **35 files, +7 151 / -13** | — |

## Migration steps

For end-user Mighty packages: **none required**. v0.13 is strictly
additive at the language and toolchain surfaces.

For toolchain contributors: there are no new gates beyond v0.12.

For Wasm component authors: `--wasi=p2` is now available. The default
stays `p1`. A `[wit]` section in `mighty.toml` will pull in your own
world definitions:

```toml
[wit]
world = "my-world"
files = ["wit/my-world.wit"]
```

For type-system / macro-crate consumers: the `effects::row::*` and
`expand_scoped` APIs are re-exported and stable for the v0.13 surface,
but they are not yet wired into typeck / hir respectively. Treat them
as v0.14 staging.

## Known issues

Canonical list in [`KNOWN_ISSUES.md`](../../../KNOWN_ISSUES.md). v0.13
does not close or open any new issue; the carry-over list is
unchanged from v0.12:

1. ~~`cabi_realloc` is a bump allocator~~ — **closed in v0.10.**
2. ~~Package signing is a stub~~ — **closed in v0.10 behind
   `sigstore-real`.**
3. MSRV gate runs only `cargo build` — partially closed in v0.10
   (carry-over).
4. ~~`clippy-strict` job is `continue-on-error: true`~~ —
   **closed in v0.11.**
5. ~~mkdocs `--strict` not enabled~~ — **closed in v0.10.**
6. Demo 02 JS shim still writes into the fixed `DOM_RETURN_AREA`
   instead of calling `cabi_realloc()` — unchanged.
7. `--no-default-features` test job does not run the example
   sweep — unchanged.
8. **Set-of-scopes hygiene in LSP completion (A111)** — note that
   v0.13 added Flatt-style set-of-scopes *infrastructure* in
   `mty-macros` (RFC-009), but the LSP completion path still uses
   the legacy mangling-based expander. Wiring is a v0.14 task.
9. **Cranelift egraph stack overflow** — filed upstream as
   wasmtime #13476; in-tree workaround `MTY_CRANELIFT_NO_OPT=1`.
10. ~~Operator precedence not normative~~ — **closed in v1.0-RC3
    (§11.1.1).**
11. **Six FROZEN typeck codes still constructor-only**
    (MT2003, MT2009, MT2022, MT2023, MT2024, MT2025) — carried over.
12. ~~`package`, `export`, `requires` keywords not in §3.3~~ —
    **closed in v1.0-RC3** (full 63-keyword set enumerated).
13. **Red-shirt:**
    `conformance/borrow_checking/14_borrow_outlives_owner` is
    ignored — MT3007 fires for `let r = &inner` but not for the
    plain-assignment reshape `r_out = &inner`. v0.13 did not
    address this; carried over to v0.14.
14. **Go 3rd-impl cross-validation pending.** Go toolchain still
    absent on the build host; carried over.
15. **(NEW) WASI P2 `wasi:cli-adapter` shim.** The v0.13 P2 component
    embeds a small internal `mighty:cli-adapter` package to keep the
    P1-shape `wasi:cli/log#log` import acceptable to `wit-component`.
    Strict P2 hosts will refuse to instantiate. Documented in
    `docs/reference/wasi.md`; the `wasmtime_p2_smoke` cargo feature
    (off by default) covers the case. v0.14 lowering pass closes
    this.

## v0.13 → v1.0-final roadmap

Carry-overs from v0.12 are unchanged. New v0.13 follow-ups:

- **Effect-rows v0.14 integration**: surface-syntax parser for
  `!E` / `!{a | E}` in `mty-syntax`; call-site validator wired into
  `mty-types/src/check.rs`; relax the rest of the stdlib HOFs
  (`filter` / `fold` / `for_each` / `find`); the MT4020-25
  diagnostic messages.
- **Macro hygiene v0.14 rewire**: switch `mty-hir`'s macro driver
  from the legacy `expand_to_source` to the new `expand_scoped` so
  the set-of-scopes layer is exercised end-to-end. Closes the LSP
  completion gap (KNOWN_ISSUES #8) on the way.
- **WASI P2 v0.14 finish**: stdlib lowerings to real P2 imports;
  embed the preview1-adapter; flip the default from P1 to P2;
  surface custom component exports.
- **Self-host v0.14 broadening**: string pool emission, pattern
  lowering, ADT layout, for-loop iter lowering. Once these land,
  the self-host chain covers a v0.7+ Mighty subset (not just
  slice-1).
- **Carry-overs**: open RFC-001..006 + RFC-008 + RFC-009 comment
  periods; wire the 6 remaining Gap-B typeck call-sites
  (MT2003/MT2009/MT2022/MT2023/MT2024/MT2025); patch
  `record_borrow_for_rhs` `BinOp::Assign` branch; run
  `go test ./...` on a Go-1.22+ host; extend the Python 2nd-impl
  through HIR + sketch typeck; split MT0001 funnel; `mty-pkg`
  cross-file resolution; publish normative conformance suite as a
  downloadable kit.

## Acknowledgments

v0.13 was built across a v0.13 swarm (four parallel tracks) followed
by an integrator pass:

- **effect-row-swarm** — RFC-008 + row infrastructure in
  `mty-types/src/effects.rs::row` + 23 new tests + relaxed
  `stdlib_list_map_sig()`. Commit `0e2269e`.
- **macro-hygiene-swarm** — RFC-009 + `scopes.rs` + `hygiene.rs` +
  `expand_scoped()` alongside legacy + 28 new tests. Commit
  `68f526a`.
- **wasi-p2-swarm** — `--wasi=p2` / `--world` flags, P2 emitter,
  vendored WASI 0.2.3 slice, `[wit]` in `mighty.toml`, 18 new tests,
  example + reference docs. Commits `fa6f16f` + `bcec034`.
- **selfhost-codegen-swarm** — `selfhost/codegen/wasm.mty` (~400 LOC
  Mighty) + bootstrap driver test + 6 passing tests. Commit
  `344f529`. **End-to-end self-host milestone**.

The integrator pass (this v0.13.0 tag commit) re-verified the gates
(1051 Rust + 137 Python + 89 conformance + 46 selfhost = 1323 tests
passing / clippy strict / fmt / 21-example matrix / 4/4 demos / 3
conformance ignored) and authored this `RELEASE-v0.13.md`.

See [`EFFECT_ROW_V0_13_NOTES.md`](../notes/EFFECT_ROW_V0_13_NOTES.md),
[`MACRO_HYGIENE_V0_13_NOTES.md`](../notes/MACRO_HYGIENE_V0_13_NOTES.md),
[`WASI_P2_V0_13_NOTES.md`](../notes/WASI_P2_V0_13_NOTES.md), and
[`SELFHOST_CODEGEN_V0_13_NOTES.md`](../notes/SELFHOST_CODEGEN_V0_13_NOTES.md)
for per-agent interpretation calls.
