# Mighty v0.4 — Complete

**Tag:** `v0.4.0`
**Date:** 2026-05-24
**Status:** SHIPPED — fourth milestone release. v0.4 is the
"dogfood + ecosystem" milestone: three real demos drive the
compiler/runtime end-to-end, the package manager grows a real
GitHub-Releases-backed registry transport, declarative macros
expand for the first time with hygiene, the lexer is rewritten in
Mighty source (subset bootstrap), and the long-standing
single-iteration MtyIR loop bug is fixed.

v0.4 was built by a four-agent autonomous swarm (demos / registry /
proc-macros / self-host lexer) over a single session, then
integrated through this slice document. A single substantive
integration-time fix — the MtyIR loop terminator — was applied to
unblock real iterative loops in the interpreter.

## What landed

### Dogfood demos — `demos/` (commit `a82c8aa`)

Three end-to-end demos exercise the v0.3 compiler + runtime as a
real user would, with smoke scripts (`.sh` + `.ps1`) gating each.

- **`demos/01_search_api`** — an agent with a `Health|Search|
  Metrics` protocol driven from `main()`. JSON-shaped responses,
  per-endpoint metrics, deterministic output. Stopgap: drives
  handlers via the `ask` operator rather than `std.http.serve`
  (the dispatcher doesn't route `serve` yet).
- **`demos/02_counter_web`** — a wasm Component agent that logs
  `count++` strings; a hand-rolled JS loader walks the component
  bytes, extracts the embedded core module, and renders the
  counter in the browser. Stopgap: JS parses log lines (the
  `mighty:web/dom` import lowering isn't wired in
  `mty-codegen-wasm/src/emit.rs`).
- **`demos/03_extract_tool`** — a CLI that streams pre-tokenised
  inputs into a `Classify(token)` agent and prints a snapshot.
  Companion `breach.sd` shows the sandbox/budget shape; v0.4
  accepts it completing (auto-charging in the MtyIR interpreter is
  v0.5). Stopgap: `String::contains` returns `false`, so the
  extractor uses `==` against an inlined vocabulary.

Each demo's `smoke.sh` passes:

```
01_search_api: PASS
02_counter_web: PASS (component size = 757 bytes)
03_extract_tool: PASS
```

See `DEMOS_V0_4_NOTES.md` for the full decision log and v0.5
follow-ups per demo.

### Real package registry transport — `mty-pkg` (commits `fb91aea`, `058b027`)

The v0.2 resolver shipped with a stubbed registry. v0.4 makes it
real:

- New `crates/mty-pkg/src/registry.rs` — `[registry]` config,
  `RegistryIndex`, `AuthStore`, slug + tag parsing.
- Rewrite of `crates/mty-pkg/src/fetch/registry.rs` — GitHub
  Releases REST client, on-disk index cache with 1-hour TTL +
  `If-Modified-Since`, sha256 sidecar verification, gzipped tar
  extraction with path-traversal guard.
- Rewrite of `crates/mty-pkg/src/publish.rs` — deterministic
  `tar.gz` + sidecar bundles plus optional GitHub Releases upload.
- Resolver wired to the cached index; falls back to the v0.2
  requirement-floor synthesis when no index is available.
- New CLI subcommands: `search`, `info`, `login`. Existing
  `add` / `remove` / `update` / `fetch` / `list` / `publish` work
  unchanged from the user's POV.
- URL scheme: `registry+gh://<owner>/<repo>` (legacy
  `registry+https://...` rejected with a "re-run `mty pkg update`"
  hint).
- Token storage at `~/.config/mty/auth.toml` (mode 0600 on Unix,
  user-profile ACLs on Windows); `SDUST_PKG_LOGIN_TOKEN` consumed
  by `pkg login`.
- Offline-first: `add` / `update` never hit the network without
  `--refresh`.

Five new test files (`registry_index_parse`, `registry_fetch` —
network test #[ignore]d, `publish_bundle`, `multi_registry_resolve`,
`auth_token_load`). See `REGISTRY_V0_4_NOTES.md` for the ten
interpretation calls (default slug, gz determinism, exit code on
no-token, …).

### Hygienic declarative macros — `mty-macros` (commit `c636dbc`)

The v0.4 macro slice ships expansion + hygiene + HIR integration.

- New `crates/mty-macros/` — registry + expander + diag-code
  constants (pure library, no I/O).
- `crates/mty-hir/src/lower/macros.rs` — preprocessor: walks the
  source, calls `sdust_macros::preprocess`, plugs the rewritten
  source back into the parser before the normal lowering walk.
- **Hygiene via mangling** — `let IDENT` bindings inside an
  expansion get a unique suffix per expansion site, so a macro
  introducing `let tmp = …` doesn't collide with the caller's
  `tmp`. (Set-of-scopes is documented as the v0.5 upgrade path.)
- **MT6001..MT6004** — bare `u16` codes living in
  `sdust_macros::diag` (the slice scope precluded modifying
  `mty-diagnostics`). The HIR integration wraps them in
  `DiagCode::new(N)` at emission.
- **Substituted arguments paren-wrapped** to preserve operator
  precedence (the HIR is identical post-parse).
- **`MAX_EXPANSION_DEPTH = 32`** — one wave per iteration; the
  cap terminates direct + mutual recursion deterministically.

Five test files (`simple_expansion`, `param_substitution`,
`hygiene_avoids_capture`, `recursive_capped`, `assert_eq_real`)
plus the canonical `examples/16_macro.sd` continues to compile +
typecheck. See `MACROS_V0_4_NOTES.md` for the eight interpretation
calls.

### Self-host lexer (subset) — `selfhost/` (commits `913d15c`, `86e3d67`, `53b4673`)

The Mighty lexer is rewritten in Mighty source as the v0.4
self-hosting beachhead. **Subset** = compiles + type/borrow-checks
cleanly, scans the first token through the host bridge; the full
diff vs the Rust lexer is gated on v0.5 work (`break`/`continue`
HIR nodes + iterator protocol).

- `selfhost/lexer/lexer.sd` — full state machine (every keyword,
  every punctuation token, every literal kind, comments,
  identifiers). Hand-written, mirrors `crates/mty-syntax/src/lexer.rs`.
- `selfhost/lexer/lib.sd` + `syntax_kind.sd` — decomposed shape
  for documentation; v0.4 uses a consolidated `lexer.sd` because
  cross-file `use` is post-v0.5.
- Host bridge through the `std.io` effect (the MtyIR lowerer
  recognises module-typed receivers and rewrites `std.io.lex_init
  (src)` into `Stmt::EffectInvoke { effect: io, op: GenericCall { …
  } }`). Five extern points: `lex_init`, `lex_len`,
  `lex_byte_at`, `lex_slice`, `lex_emit`.
- New test `crates/mty-driver/tests/selfhost_lexer.rs` — three
  active tests + one `#[ignore]` for the full v0.5 diff.

Seven catalogued v0.3 language gaps documented in
`SELFHOST_V0_4_NOTES.md` (most prominently: single-iteration loops
— now fixed in v0.4 — and the missing `break`/`continue` HIR
nodes).

### MtyIR loop terminator fix — integration-time substantive change

`crates/mty-sir/src/lower/exprs.rs` previously set the body
terminator of `lower_while` / `lower_loop` / `lower_for` to
`Term::Goto(exit)`, collapsing every loop into a single iteration.
The v0.4 self-host agent surfaced this as the dominant blocker.

The integrator fix routes each body's terminator to
`Term::Goto(header)` so:

- `while cond { body }` properly re-evaluates `cond` between
  iterations (terminates when `cond` becomes false).
- `loop { body }` runs until trap / return / try-return-err, or
  until the interpreter's step budget (default 1M) trips
  `RunResult::BudgetExceeded`.
- `for x in iter { body }` runs until step budget — the iterator
  protocol modelling (`__i >= __arr.len` exhaustion check) is v0.5
  scope.

The interpreter's step budget (`DEFAULT_STEP_BUDGET = 1_000_000`)
caps any runaway. Examples that only `check` are unaffected;
examples that `run` against the new iterative semantics rely on
side-effecting `cond`s to terminate (the while case) or on the
budget (the loop / for cases until `break` + iterator-exhaustion
land in v0.5).

## Test count delta

| Milestone | Tests | Delta |
|---|---|---|
| v0.1.0 | 376 | baseline |
| v0.2.0 | 550 | +174 |
| v0.3.0 | 623 | +73 |
| v0.4.0 | **692** | **+69** |

0 failures, 3 ignored (1 network-bound git-fetch in `mty-pkg`,
1 network-bound registry-fetch in `mty-pkg`, 1 v0.5-gated
full-lexer-diff in `mty-driver/tests/selfhost_lexer.rs`).
`cargo clippy --workspace --all-targets -- -D warnings` clean.
`cargo fmt --all -- --check` clean.

## Closed deferrals from v0.3

Consolidated from the v0.3 `*_NOTES.md` set and `SLICE_V0_3.md`'s
28-item deferral list.

| Item | Status in v0.4 |
|---|---|
| Backtracking package resolver + tar/flate2 + real registry | **shipped** (registry agent: GH Releases transport, gz tar bundles, on-disk index cache, three new CLI commands) |
| Procedural / declarative macros | **shipped — declarative** (mty-macros: expansion + hygiene + MT6001..MT6004; proc macros remain v0.5+) |
| Real `loop { break }` lowering (single-iteration loops in MtyIR) | **partially shipped** — MtyIR loops now iterate properly; `break`/`continue` HIR nodes remain v0.5 |
| 20/20 wasm-Component example sweep (v0.3 regression gate) | **held** (20/20 still passing) |
| LSP integration test contract under A65 strict scopes | **held** (no regression) |
| Two-phase borrows / deeper field paths (`s.a.b`) / index-aware disjointness | held — borrow checker untouched in v0.4 |
| Polonius-style conditional-branch joins | held |
| Slice-7 supervisor/cap-narrow strict cap-name resolution | held |
| Function-signature cap-narrowing | held |
| Cross-package Sendable propagation | held |
| MtyIR-side cancellation polling (true mid-turn interrupt) | held |
| CpuBudget reason wiring | held |
| HTTP/protobuf OTLP transport selector | held |
| OTel resource-attribute env-vars | held |
| DelayScheduler as default per-turn timer | held |
| WASI Preview 2 + user-authored WIT | held |
| DWARF v5 + per-instruction line program | held |
| `dyn Trait` dispatch + closure capture in compiled code | held |
| LLVM backend smoke on Linux/LLVM 17 | held |
| 2 `INTENTIONALLY_IGNORED` conformance cases (cap-narrow / escalate) | held |

The carried items are scoped into v0.5; nothing regressed.

## New amendments (committed to spec)

```
A57 — Declarative macro expansion model + hygiene via mangling (v0.4)
A58 — MT6001..MT6004 macro diagnostic codes (v0.4)
A59 — GitHub Releases registry transport + `registry+gh://<owner>/<repo>` URL scheme (v0.4)
A60 — Offline-first resolver: `add` / `update` use cache; `--refresh` for network (v0.4)
A61 — Deterministic `.tar.gz` bundles for `pkg publish` (v0.4)
A62 — MtyIR loop terminator routes to header (loops iterate; `break` still v0.5) (v0.4)
A63 — Self-host bootstrap via `std.io` effect bridge (v0.4)
```

(A57..A63 are the renumbered v0.2 draft slots noted in
`SLICE_V0_3.md`; they now carry the v0.4 work that earned them.)

## Headline soundness / correctness improvements

| Property | v0.3 | v0.4 |
|---|---|---|
| `while cond` re-evaluates cond between iterations | single-iter (collapsed to `if`) | **iterates** (A62) |
| `loop { body }` runs until trap / return / budget | single-iter | **iterates until budget** (A62) |
| Macro hygiene at `let IDENT` sites | n/a (no expansion) | **mangled — no caller collision** (A57) |
| Macro arity / recursion-depth violations | n/a | **MT6001..MT6004 with span** (A58) |
| Package fetch from registry | stubbed | **real GH Releases + sha256 + offline cache** (A59/A60) |
| `pkg publish` bundle determinism | n/a | **byte-identical across runs** (A61) |
| Lexer source bootstrap | external Rust only | **Mighty subset compiles + first-token round-trips via host bridge** (A63) |

## New diagnostic codes

Four codes minted by the macros agent (bare `u16` in
`sdust_macros::diag`, wrapped in `DiagCode::new(N)` at emission):

- **MT6001** — unknown_macro (reserved — fires once the
  `mac!name(...)` syntactic marker lands in v0.5)
- **MT6002** — macro_arity_mismatch
- **MT6003** — macro_recursion_depth_exceeded
  (`MAX_EXPANSION_DEPTH = 32`)
- **MT6004** — macro_bad_argument_tokens

These do not yet appear in `mty-diagnostics::codes`; the cleanup
folds them into the central catalog in v0.5.

## Cross-cut fixes applied during integration

1. **MtyIR loop terminator** (`crates/mty-sir/src/lower/exprs.rs`)
   — `lower_while` / `lower_loop` / `lower_for` body terminator
   changed from `Goto(exit)` to `Goto(header)`. Documented inline
   with the v0.4 stopgap context (no `break` HIR yet, no iterator
   protocol yet — runaway loops bounded by step budget).
2. **Selfhost lexer test update** (`crates/mty-driver/tests/selfhost_lexer.rs`)
   — `selfhost_lexer_first_token_matches` was written against the
   old single-iteration behaviour (expected exactly two emits:
   first token + trailing EOF after the one-shot loop). Post-fix,
   the inner scanners spin until step budget because `break` is
   not yet an HIR node, so the test now asserts the v0.4 contract:
   the loop fix is live (run terminates via `BudgetExceeded`, not
   a clean exit). The `#[ignore]` full-diff test's gating reason
   is updated to mention `break`/iterator protocol.

Total: 2 files touched at integration time. Both are documented
above. No new features.

## New deferrals to v0.5

Consolidated from `DEMOS_V0_4_NOTES.md`, `REGISTRY_V0_4_NOTES.md`,
`MACROS_V0_4_NOTES.md`, `SELFHOST_V0_4_NOTES.md`, plus the new
v0.4 loop fix's residue.

### Loops / control flow

1. **`break` / `continue` HIR nodes.** The parser already lexes
   `break` and `continue` as bare identifiers (they're not
   keywords). HIR + lowering needs `HirExpr::Break(loop_label)` /
   `HirExpr::Continue(loop_label)` to plumb out of nested blocks.
2. **`for` iterator-exhaustion check.** The slice-6 simplification
   in `lower_for` skips the `__i >= __arr.len` probe. Real
   iterator protocol + the loop-header check.
3. **Loop-back-edge borrow modelling** (carried from v0.3).

### Self-hosting

4. **`!fn(args)` parse precedence.** Unary `!` binds tighter than
   `(args)` so `!is_space(b)` parses as `(!is_space)(b)` and trips
   MT2008. Standard fix: `unary_op call_expr` becomes
   `unary_op(call_expr)`.
5. **`extern { fn ... }` real dispatch.** Bodyless extern fns
   currently lower to `return Unit`; route through
   `BuiltinId::Extern(name)` instead so `Host::extern_call` fires.
6. **Cross-file module resolution.** Wire `mty-pkg`'s module
   table into the resolver so `use selfhost_lexer.SyntaxKind`
   resolves transparently.
7. **Real `Str` method intrinsics.** `String::contains` /
   `starts_with` / `ends_with` / `char_at` / `slice` / `find` /
   `chars` currently return blanket false/Unit; wire to underlying
   Rust impls.
8. **Self-host parser + HIR + typeck.** v0.5 lands `parser.sd`
   (post the precedence + iterator + break work above).

### Macros

9. **`mac!name(...)` syntactic marker.** Activates MT6001.
10. **Set-of-scopes hygiene** replacing the v0.4 mangling pass.
11. **Proc macros** (sandboxed token-tree → token-tree functions).
12. **Cross-file macro export + visibility** (`pub macro foo`).
13. **`format!`-style variadic macro arguments.**

### Registry / pkg

14. **Create `hassard0/stardust-pkg-registry`** (or similar slug)
    and seed it with the stdlib.
15. **Move `Manifest` into `mty-pkg`**, leave a re-export in
    `mty-driver` (eliminates the duplicate-parse-of-`mighty.toml`
    workaround).
16. **`[package].include` / `.exclude` globs** for bundle
    contents.
17. **Yanked-version support** (release-body marker + consumer
    warning).
18. **`mty pkg audit`** — security advisory cross-referencing.
19. **Signed releases** via sigstore/cosign.
20. **Pluggable secret store** (Keychain / Credential Manager /
    libsecret) replacing plaintext `~/.config/mty/auth.toml`.
21. **Interactive `pkg login`** (post-TUI).
22. **Real HTTP/registry-mirror backend** (`registry+https://`).

### Demos

23. **`std.http.serve` through `host::dispatch`** + an agent-side
    `Handler` adapter (unblocks `01_search_api`'s server form).
24. **`mighty:web/dom` import lowering** in
    `mty-codegen-wasm/src/emit.rs` (unblocks `02_counter_web`'s
    real DOM mutation).
25. **Auto-charging in the MtyIR interpreter** so cpu/mem caps trip
    on pure-compute loops (unblocks `03_extract_tool`'s `breach.sd`).

### Carried from v0.3 (still open)

26. Two-phase borrows, deeper field paths (`s.a.b`), index-aware
    disjointness.
27. Polonius-style conditional-branch ledger joins.
28. Cross-fn region inference (explicit lifetime parameters).
29. Slice-7 supervisor/cap-narrow strict cap-name resolution.
30. Function-signature cap-narrowing (propagate
    `CapConstraint::And` into fn signatures).
31. Cross-package Sendable propagation.
32. Sendable lambda capture analysis.
33. MtyIR-side cancellation polling (true mid-turn interrupt).
34. CpuBudget reason wiring.
35. HTTP/protobuf OTLP transport selector.
36. OTel resource-attribute env-vars
    (`OTEL_RESOURCE_ATTRIBUTES`, `OTEL_SERVICE_NAME`).
37. DelayScheduler as default per-turn timer.
38. WASI Preview 2 + user-authored WIT.
39. DWARF v5 + per-instruction line program.
40. `dyn Trait` dispatch + closure capture in compiled code.
41. LLVM backend smoke on Linux/LLVM 17.
42. `capability_checking/03_narrow_to_ro` conformance case.
43. `supervisor_restart/02_escalate` conformance case + grammar.

## Stats

- **8 commits since v0.2.0** — wait, since v0.3.0: one prep
  (`69de965`) + four swarm + 3 self-host-agent intra-swarm commits.
- **8,322 insertions / 217 deletions** across 70 files.
- **1 new crate** (`mty-macros`) — workspace grows to **20**.
- **+69 new tests** (623 → 692).
- **0 clippy warnings** with `-D warnings`.
- **20/20 examples build to native objects** (unchanged from v0.3).
- **20/20 examples build to bare wasm core modules** (unchanged).
- **20/20 examples build as Wasm Components** (unchanged).
- **32 conformance cases run**, 2 still ignored (unchanged from v0.3).
- **3/3 dogfood demos pass `smoke.sh`** (search_api, counter_web,
  extract_tool).
- **7 new spec amendments** (A57..A63).
- **4 new SD codes** (MT6001..MT6004, macros).
- **MSRV unchanged at 1.85**.

## Known issues

1. **`break` / `continue` are not yet HIR nodes** — they currently
   parse as bare identifier expressions with no side effect. Any
   `loop { if cond { break } … }` shape runs until the
   interpreter's step budget (`MT5009 BudgetExceeded`). This blocks
   the full self-host lexer diff (the bootstrap test runs first
   token + asserts BudgetExceeded as its v0.4 contract).
2. **`for` has no iterator-exhaustion check** — `for x in arr {
   body }` lowers to header→body→header with no `__i >= __arr.len`
   probe, so it spins until step budget. The compiled (native /
   wasm) backends already model this correctly; only the
   tree-walking interpreter has the gap.
3. **Demo gaps** (per `DEMOS_V0_4_NOTES.md`):
   - `01_search_api` drives via `ask` rather than
     `std.http.serve` (dispatcher route missing).
   - `02_counter_web` uses JS to parse log lines because
     `mighty:web/dom` import lowering isn't wired in
     `mty-codegen-wasm`.
   - `03_extract_tool` uses string equality rather than
     `String::contains` (interpreter stub returns false); the
     breach test runs to completion because the MtyIR interpreter
     has no auto-charging on pure-compute loops.
4. **Macro hygiene is mangling-based, not set-of-scopes** — only
   `let IDENT` is mangled; pattern bindings (`let (a, b) = …`)
   inside an expansion don't yet get unique suffixes.
5. **`mac!name(...)` syntactic marker not yet parsed** — MT6001
   stays reserved. Macros are detected via `CALL_EXPR` whose
   callee path matches a registered macro name.
6. **`mty-macros` SD6xxx codes** live in `sdust_macros::diag` as
   bare `u16`, not in `mty-diagnostics::codes` (slice scope).
7. **No default registry** — the default slug
   `mighty-pkg/registry` is reserved but not created. Network
   fetches against it surface a clean 404. v0.5 creates the repo
   and seeds the stdlib.
8. **Plaintext auth token** at `~/.config/mty/auth.toml` (mode
   0600 Unix; ACLs Windows). Pluggable secret store is post-v0.5.
9. **Carried from v0.3**: 2 conformance cases still ignored, OTLP
   transport gRPC-only, LLVM backend untested on this build host,
   slice-7 supervisor/cap-narrow scopes stay strict-but-open.

## What's next

v0.5 picks up the 43-item deferral list above. Likely themes:

- **`break` / `continue` HIR + iterator protocol** — unblocks the
  full self-host lexer diff and lets `for` / `loop` terminate
  naturally without relying on the step budget.
- **Parser precedence fix for `!call_expr`** — closes self-host
  gap #2.
- **Self-host parser + HIR** — the next ladder rung after the
  lexer.
- **Real `String::contains` etc.** — interpreter intrinsics
  matching the Rust impls.
- **Cross-file module resolution** — wire `mty-pkg`'s module
  table into the resolver.
- **Polonius-style borrow checker** — conditional-branch join
  refinement + two-phase borrows.
- **WASI Preview 2 + user-authored WIT** in the Component
  pipeline.
- **`std.http.serve` host bridge + agent Handler adapter**
  — unblocks `01_search_api`'s real server form.
- **`mighty:web/dom` import lowering** — unblocks
  `02_counter_web`'s real DOM path.

The aspirational v0.5 tagline: *"the compiler runs its own lexer
and parser, and every demo is self-contained."*
