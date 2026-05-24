# Stardust v0.4 — Release Notes

**Tag:** `v0.4.0`
**Date:** 2026-05-24
**Status:** SHIPPED — fourth milestone release. Dogfood demos,
real package registry transport, hygienic declarative macros,
self-host lexer (subset bootstrap), and an integration-time fix
to the SIR loop terminator (loops now iterate).

Stardust v0.1 walked the spec §31 ladder end-to-end. v0.2 lit up
every surface the v0.1 deferral list named. v0.3 hardened
soundness across the borrow checker, type checker, and runtime.
v0.4 is the **dogfood + ecosystem** milestone: three real demos
drive the compiler/runtime end-to-end with passing smoke
scripts, the package manager grows a real GitHub-Releases-backed
registry, declarative macros expand for the first time with
hygiene, the lexer is rewritten in Stardust source, and the
long-standing single-iteration loop bug in the SIR interpreter is
fixed.

## What you can do (new in v0.4)

```bash
# Three demos drive the compiler + runtime end-to-end.
bash demos/01_search_api/smoke.sh
# → 01_search_api: PASS

bash demos/02_counter_web/smoke.sh
# → 02_counter_web: PASS (component size = 757 bytes)

bash demos/03_extract_tool/smoke.sh
# → 03_extract_tool: PASS

# Package manager pulls + verifies tarballs from a real registry.
sdust pkg search rxc
sdust pkg info rxc
sdust pkg add rxc --version "^0.3"
sdust pkg update --refresh   # opt-in network refresh; offline-first by default

# Publish a deterministic bundle to GitHub Releases.
SDUST_PKG_LOGIN_TOKEN=ghp_… sdust pkg login
sdust pkg publish my_package

# Declarative macros now expand with hygiene.
# (see examples/16_macro.sd; sdust-macros::diag exports SD6001..SD6004)
sdust check examples/16_macro.sd

# The Stardust lexer is now written in Stardust source.
sdust check selfhost/lexer/lexer.sd

# Loops actually loop.
sdust run examples/06_for_while_loop.sd       # main() is still log-only, but
                                              # the helpers now iterate properly
```

Everything from v0.3 still works the same way.

## The four swarm agents

v0.4 was built by 4 autonomous swarm agents working disjoint
crate boundaries, then integrated through this release:

| Agent | Crates / files | Commits |
|---|---|---|
| dogfood demos | `demos/01_search_api`, `demos/02_counter_web`, `demos/03_extract_tool` | `a82c8aa` |
| pkg registry | `sdust-pkg` rewrite + new CLI subcommands + docs | `fb91aea`, `058b027` |
| proc / declarative macros | `sdust-macros` (new crate) + `sdust-hir/src/lower/macros.rs` | `c636dbc`, prep `69de965` |
| self-host lexer | `selfhost/lexer/lexer.sd`, `lib.sd`, `syntax_kind.sd`, `crates/sdust-driver/tests/selfhost_lexer.rs`, `docs/internals/self-hosting.md` | `913d15c`, `86e3d67`, `53b4673` |

Plus one substantive integration-time change: the SIR loop
terminator fix in `crates/sdust-sir/src/lower/exprs.rs`.

## Headline numbers

- **692 tests pass** (0 failures, 3 ignored — 2 network, 1 v0.5-gated) — was 623 in v0.3
- **+69 tests** added in v0.4
- **0 clippy warnings** with `-D warnings`
- **20 crates** in the workspace (was 19; `sdust-macros` added)
- **8 commits** since `v0.3.0`
- **8,322 insertions / 217 deletions** across 70 files
- **20/20 examples compile to native** (unchanged from v0.3)
- **20/20 examples compile to bare wasm core modules** (unchanged)
- **20/20 examples compile as Wasm Components** (unchanged)
- **32 conformance cases run** (unchanged), 2 ignored (unchanged)
- **3/3 dogfood demo smoke scripts pass** (search_api, counter_web, extract_tool)
- **7 new spec amendments** (A57..A63)
- **4 new SD codes** (SD6001..SD6004, macros)
- **MSRV unchanged at 1.85**

## Correctness assertions newly enforced

| Property | v0.3 | v0.4 |
|---|---|---|
| `while cond` re-evaluates cond between iterations | single-iteration (collapsed to `if`) | iterates (A62) |
| `loop { body }` runs until trap / return / budget | single-iteration | iterates until budget (A62) |
| Macro hygiene at `let IDENT` sites | n/a (no expansion) | mangled — no caller collision (A57) |
| Macro arity / depth violations | n/a | SD6002..SD6004 with span (A58) |
| Package fetch from registry | stubbed | real GH Releases + sha256 + offline cache (A59/A60) |
| `pkg publish` bundle determinism | n/a | byte-identical across runs (A61) |
| Stardust source can express its own lexer | external Rust only | subset compiles + first-token round-trips via `std.io` host bridge (A63) |

## Closed deferrals from v0.3

The v0.3 deferral list named 28 items. v0.4 closes the following:

- **Backtracking package resolver + tar/flate2 + real registry** — shipped (registry agent: GH Releases transport, deterministic gz tar bundles, on-disk index cache with `If-Modified-Since`, sha256 verification, three new CLI commands)
- **Procedural / declarative macros** — declarative-side shipped (sdust-macros: expansion + mangling-based hygiene + SD6001..SD6004; proc macros remain v0.5+)
- **Real `loop { break }` lowering** — partially shipped (the SIR loop terminator is fixed; `break`/`continue` HIR nodes remain v0.5)

The other 25 items roll into v0.5 (see `SLICE_V0_4.md` for the
full carry-over list).

## Spec amendments (7 new)

```
A57 — Declarative macro expansion model + hygiene via mangling
A58 — SD6001..SD6004 macro diagnostic codes
A59 — GitHub Releases registry transport + `registry+gh://<owner>/<repo>` URL scheme
A60 — Offline-first resolver: `add` / `update` use cache; `--refresh` for network
A61 — Deterministic `.tar.gz` bundles for `pkg publish`
A62 — SIR loop terminator routes to header (loops iterate; `break` still v0.5)
A63 — Self-host bootstrap via `std.io` effect bridge
```

All committed to `docs/spec/v0.1-amendments.md` (A57..A63 reuse
the renumbered v0.2 draft slots noted in `SLICE_V0_3.md`).

## Diagnostic codes

Four new SD codes for the macro slice, defined in
`sdust_macros::diag` as bare `u16` and wrapped at emission:

- **SD6001** — `unknown_macro` (reserved — fires once the
  `mac!name(...)` syntactic marker lands in v0.5)
- **SD6002** — `macro_arity_mismatch`
- **SD6003** — `macro_recursion_depth_exceeded`
  (`MAX_EXPANSION_DEPTH = 32`)
- **SD6004** — `macro_bad_argument_tokens`

`sdust explain SD6xxx` is not yet wired (v0.5 cleanup folds these
codes into `sdust-diagnostics::codes`).

## Toolchain

- **MSRV: Rust 1.85** (unchanged from v0.2)
- New crate `sdust-macros` in the workspace (no new optional
  features — pure library, plumbed in via `sdust-hir`)
- `sdust-pkg` gains deps on `tar`, `flate2`, `dirs`, `serde_json`
- All-platform: Windows, macOS, Linux
- Cargo workspace; no `build.rs` magic

## Deferred to v0.5 / post-v0.4

The full deferral catalogue (43 items) lives in `SLICE_V0_4.md`.
Highlights:

- **Loops / control flow**: `break` / `continue` HIR nodes; `for`
  iterator-exhaustion check (`__i >= __arr.len`); loop-back-edge
  borrow modelling (carried from v0.3).
- **Self-hosting**: `!fn(args)` parse-precedence fix; real
  `extern { fn ... }` dispatch through `BuiltinId::Extern`;
  cross-file module resolution; real `Str` method intrinsics;
  self-host parser + HIR + typeck.
- **Macros**: `mac!name(...)` syntactic marker (activates
  SD6001); set-of-scopes hygiene; proc macros; cross-file macro
  export; `format!`-style variadic args.
- **Registry / pkg**: create + seed the default registry repo;
  consolidate `Manifest` into `sdust-pkg`; bundle
  `include`/`exclude` globs; yanked-version support; signed
  releases; pluggable secret store; interactive `pkg login`;
  HTTP/registry-mirror backend.
- **Demos**: `std.http.serve` through the dispatcher + Handler
  adapter; `stardust:web/dom` import lowering; SIR-side
  auto-charging for cpu/mem budget caps.
- **Carried from v0.3**: Polonius-style borrows, slice-7
  supervisor / cap-narrow strict resolution, SIR-side
  cancellation polling, WASI Preview 2, DWARF v5, LLVM backend
  smoke on Linux+LLVM 17, `dyn Trait` dispatch + closure capture
  in compiled code.

## Known issues

1. **`break` / `continue` are not yet HIR nodes** — they parse
   as bare identifier expressions with no side effect. With the
   v0.4 loop-fix in place, any `loop { if cond { break } … }`
   shape runs until the interpreter's step budget
   (`BudgetExceeded`). This blocks the full self-host lexer diff
   (the v0.4 bootstrap test asserts BudgetExceeded as its
   contract).
2. **`for` has no iterator-exhaustion check in the SIR
   interpreter** — `for x in arr { body }` lowers to
   header→body→header with no `__i >= __arr.len` probe. The
   compiled (native / wasm) backends model this correctly; only
   the tree-walking interpreter has the gap.
3. **Demo gaps** (per `DEMOS_V0_4_NOTES.md`):
   - `01_search_api` drives via `ask` rather than
     `std.http.serve`.
   - `02_counter_web` uses JS to parse `log` lines because
     `stardust:web/dom` import lowering isn't wired.
   - `03_extract_tool` uses `==` against an inlined vocabulary
     because `String::contains` returns false; `breach.sd` runs
     to completion because the SIR interpreter has no
     auto-charging.
4. **Macro hygiene is mangling-based, not set-of-scopes** —
   `let IDENT` is mangled; pattern bindings inside an expansion
   are not.
5. **`mac!name(...)` syntactic marker not yet parsed** — SD6001
   stays reserved.
6. **No default registry** — `stardust-pkg/registry` is reserved
   but not created. Network fetches surface a clean 404.
7. **Plaintext auth token** at `~/.config/sdust/auth.toml`.
8. **Carried from v0.3**: 2 conformance cases still ignored,
   OTLP transport gRPC-only, LLVM backend untested on this build
   host, supervisor/cap-narrow scopes strict-but-open.

## Backwards compatibility

v0.4 is a minor-version bump from v0.3. Source compatibility is
preserved for slice 1-8 + v0.2 + v0.3 surfaces. **Notable
behavior changes**:

- **Loops iterate.** Code that quietly relied on the v0.3 SIR
  interpreter collapsing every `while` / `loop` / `for` into a
  single iteration will now either keep iterating (and terminate
  when `cond` goes false, for `while`), trap on `BudgetExceeded`
  after ~1M steps (for unbounded `loop` / `for` shapes), or
  return / panic from inside the body. Real algorithmic loops
  that need to terminate should make `cond` go false; `break`
  remains an unimplemented HIR node (v0.5).
- **Single-file packages picked up new resolver paths.** `add` /
  `update` now look in `~/.cache/sdust/registry/` (or the
  platform equivalent) for cached indexes. `update --refresh`
  opts into a network read; the unconditional fetch from v0.2 is
  gone (offline-first by design).
- **`registry+https://...` URL prefix is rejected.** Migrate
  lockfiles by running `sdust pkg update` — the error message
  spells out the migration.
- **`sdust pkg publish` now produces a deterministic `tar.gz`.**
  The CI lockfile-compare check should pin against the new
  bytes; old artifacts will not byte-match.
- **`fn main()` is no longer required** for macro-only modules
  that don't `run` — the macros agent's preprocessor expands the
  module shape correctly.

Diagnostic codes (SD0001..SD8010 + SD6001..SD6004 minted in
v0.4) are otherwise unchanged. CLI shape is unchanged except for
the three new `sdust pkg` subcommands (`search` / `info` /
`login`) and `pkg update --refresh`.

## Acknowledgments

v0.4 is the third Stardust release built by autonomous parallel
agents. The four swarm agents shipped tightly because each
touched disjoint crates — demos (no compiler edits) vs pkg vs
new sdust-macros crate vs selfhost — and the integrator only
needed to apply the SIR loop terminator fix (plus a one-test
update to acknowledge the new iterative semantics). The agents
stood on the slice-1..8 + v0.2 + v0.3 foundations: the
declarative parser, the typed HIR / SIR / interpreter, the
Cranelift / wasm / Component pipelines, the v0.3 host bridge
through `sdust_runtime::host_std::install_dispatcher`, and the
conformance harness all carried forward without rewrites.

Big thanks to the `tar`, `flate2`, `dirs`, `serde_json` teams —
the new package transport stands on those shoulders too.

## What's next

v0.5 picks up the 43-item deferral catalogue. The aspirational
v0.5 tagline: *"the compiler runs its own lexer and parser, and
every demo is self-contained."*
