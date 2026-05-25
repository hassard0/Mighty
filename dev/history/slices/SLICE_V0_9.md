# Mighty v0.9 — Complete

**Tag:** `v0.9.0`
**Date:** 2026-05-24
**Status:** SHIPPED — ninth milestone release. v0.9 is the
**RC-prep + freeze-readiness** milestone: the v1.0 spec is promoted
to **v1.0-RC2** with all 10 OPEN amendments resolved (3 FREEZE-MVP,
7 DEFER-V1.1) and 6 first-draft RFCs landed, the fuzz harness is
brought up against four front-end targets (parser / typeck / fmt /
codegen), the **3 P0 v1.0 blockers** the fuzzer surfaced are fixed
in this slice along with an audit-sweep of every sibling parser
loop, the MtyIR lowering is partially self-hosted (joining lexer
+ parser + HIR + typeck), and the v0.9 RC-prep agent shipped the
demo 02 `cabi_realloc` fix, the GitHub Pages docs site, the
package-signing stub, CI hardening, and the release scripts.

v0.9 was built by a four-agent autonomous swarm (spec-freeze /
fuzz / self-host MtyIR / RC-prep) over a single overnight session,
then integrated through this slice. The integrator pass also closed
the **parser non-progress-guard family** of bugs reported by the
fuzz agent — three P0 OOM bugs (Bug 1 in `enum_decl`, Bug 2 inherited
in fmt, Bug 3 in `protocol_decl/msg`) plus seven audit-sweep extras
in sibling productions (struct/trait/impl/sandbox/match/supervisor/
extern). Full audit in [`PARSER_AUDIT_V0_9.md`](PARSER_AUDIT_V0_9.md).

## What landed

### Spec freeze prep v1.0-RC2 — spec-freeze swarm agent (commit `637312d`)

The v0.8 consolidation left 10 amendments OPEN. v0.9 resolves all 10
and promotes the v1.0 spec to v1.0-RC2:

- **3 FREEZE-MVP**: A15 (default-public visibility default), A31
  (capability resolution defaults), A49 (effect-call defaults).
- **7 DEFER-V1.1** (each with a follow-up RFC): A11 (anonymous error
  unions → RFC-001), A45 (legacy alias removal cadence), A47 (wasm
  component-model wrapper → RFC-002), A94 (sandboxed proc-macro
  execution → RFC-003), A97 (component-wrapper imports → RFC-002),
  A102 (affinity front-end syntax → RFC-005), A103 (lossless live
  agent migration → RFC-006).
- **Note on A100**: stays FROZEN; the per-call FsCap-manifest threading
  RFC is RFC-004 (v1.1+).

Six RFCs are first-drafts published under `docs/spec/rfcs/`. Each
ships with explicit "v1.0 status" + "v1.1+ scope" sections so that
adopters know exactly what the v1.0 contract is for each feature.

`docs/spec/v1.0-rc.md` is promoted in-place to v1.0-RC2 with appendix
§A.2 expanded to cover all 7 DEFER-V1.1 features. The amendments doc
gets the 10 new Status lines. The CHANGELOG records the v1.0-RC2
entry. **No crate source files were touched**, **no Cargo.toml was
touched** — this slice is pure spec.

See [`SPEC_FREEZE_V0_9_NOTES.md`](SPEC_FREEZE_V0_9_NOTES.md) for
per-amendment rationale + each RFC's design space.

### Fuzz harness bring-up — fuzz swarm agent (commits `d977ea6`, `9e4980b`, `99c8676`)

Four cargo-fuzz targets shipped, each with a 27-file seed corpus
(20 examples + 5 self-host sources + minimal + empty):

| Crate                              | Target            | Asserts                                     |
| ---------------------------------- | ----------------- | ------------------------------------------- |
| `crates/mty-syntax/fuzz`            | `parser_fuzz`     | `parse(s)` never panics                    |
| `crates/mty-types/fuzz`             | `typeck_fuzz`     | parse + HIR lower + `check_package_typed`  |
| `crates/mty-fmt/fuzz`               | `fmt_idempotence` | `format(format(x)) == format(x)`           |
| `crates/mty-codegen-cranelift/fuzz` | `codegen_fuzz`    | Cranelift lowering + object emit never panic |

5-minute smoke runs on `x86_64-pc-windows-msvc` nightly surfaced
four bugs, all triaged in [`FUZZ_V0_9_NOTES.md`](FUZZ_V0_9_NOTES.md):

- **Bug 1 (P0 v1.0)**: `enum_decl` OOM on malformed payload like
  `enum E { R(F>4)` — non-progress loop grew green tree to ~12 GB.
- **Bug 2 (P0 v1.0)**: fmt OOM, inherited from Bug 1 (fmt calls parse).
- **Bug 3 (P0 v1.0)**: same anti-pattern in `protocol_decl` /
  `protocol_msg`. 96-byte repro.
- **Bug 4 (P1)**: Cranelift egraph stack-overflow on generic-slice
  input. Upstream — not in our code.

Steady-state fuzzing docs live at `docs/internals/fuzzing.md`. CI
integration (PR fast path + nightly 5-min sweep + 30-min release
gate) is documented for v0.10 wiring.

### Parser audit fix — integrator pass (this commit, `4f85e8e`)

The v0.9 fuzz agent could not apply the parser fix (out of their
scope per the shared-tree concurrency rule). The integrator slice
closes Bug 1, Bug 2 (by inheritance), and Bug 3, plus an audit
sweep across every sibling loop in
`crates/mty-syntax/src/parser/{items, agents, types, exprs, stmts,
concurrency, extern_}.rs`.

The pattern: every `while !p.at(R_BRACE) && !p.at(EOF)` body that
calls a parsing helper which can fail to consume any tokens grows
green-tree nodes one per iteration on adversarial input until OOM.
The fix is a one-shot `let before = p.pos; … if p.pos == before {
error+bump_any }` guard applied everywhere.

Vulnerable loops fixed in this slice:

- `items.rs::enum_decl` (Bug 1)
- `agents.rs::protocol_decl` (Bug 3)
- `items.rs::struct_decl` (audit sweep)
- `items.rs::trait_decl` (defensive)
- `items.rs::impl_block` (defensive)
- `items.rs::sandbox_decl` (top-level form)
- `items.rs::attribute` derive-args loop
- `agents.rs::supervisor_decl`
- `stmts.rs::match_expr`
- `extern_.rs::extern_block`

Safe-by-construction loops audited and confirmed not vulnerable:
all of `types.rs`, the `exprs.rs` tuple/map/struct/args loops,
`concurrency.rs::{budget_block, sandbox_block}` (already pre-guarded
in v0.6), `stmts.rs::block`.

**Regression coverage**: 16 new tests in
`crates/mty-syntax/tests/parser_non_progress.rs`. 13 adversarial
inputs (one per fixed vulnerable loop + the two fuzz repros + the
parser_fuzz artifact replay), 2 well-formed sanity tests, plus
direct replay of the saved fuzz `oom-*` artifacts. Each adversarial
input took ~5 s + 12 GB pre-fix; all return in microseconds post-fix.

**Re-verification**: 60-second parser_fuzz smoke run completed
cleanly on Windows MSVC nightly after the fix — 13,859 mutations,
zero OOMs, zero panics. The non-progress family is closed.

Full audit in [`PARSER_AUDIT_V0_9.md`](PARSER_AUDIT_V0_9.md).

### Self-host MtyIR (subset) — selfhost-mtyir swarm agent (commits `cb38c8a`, `dc1538a`)

The v0.6 lexer (4/4), v0.6 parser (13/13), v0.8 HIR (5/7), and v0.8
typeck (5/7) self-host coverage is joined by the v0.9 MtyIR lowering:

- `selfhost/ir/lib.mty` (26 LOC) — package + intent doc.
- `selfhost/ir/nodes.mty` (122 LOC) — data-shape spec mirroring
  `crates/mty-ir/src/ir.rs`.
- `selfhost/ir/lower.mty` (~530 LOC) — `mty check` clean.

`cargo test -p mty-driver --test selfhost_ir` passes **7/9** live
tests on examples 01-03. Examples 04 + 05 are ignored with the
same kind of explanatory message as in v0.8 (Result-sugar return +
`?` operator + struct-literal expressions in 04; range patterns +
private-fn name mangling in 05).

The MtyIR lowering is **subset coverage by design** — the Rust
lowerer is ~2500 LOC across 5 files; replicating all of it would
exceed the v0.9 budget by 3-4x. The shipped subset covers every IR
production exercised by examples 01-03 (and partially 04-05). Gap
catalog in [`SELFHOST_IR_V0_9_NOTES.md`](SELFHOST_IR_V0_9_NOTES.md).

**Self-host running total: 4 + 13 + 5 + 5 + 7 = 34 tests.**

### RC release prep — rc-prep swarm agent (commit `69a3cb7`)

Six release-prep tasks shipped:

1. **`demos/02_counter_web` `cabi_realloc` fix.** Root cause:
   `wit-component::ComponentEncoder::encode()` requires every core
   module whose WIT world has imports returning owned heap values
   (`string`, `list<u8>`, …) to export `cabi_realloc(i32, i32, i32,
   i32) -> i32`. The Mighty wasm32-web world has had those since
   v0.5; the emitter never synthesised the export. Fix in
   `mty-codegen-wasm/src/emit.rs`: bump-allocator i32 global +
   align-up synthesised function, exported as `cabi_realloc`.
   Result: demo 02 smoke now PASSes (component size 1523 bytes).
2. **GitHub Pages site** at
   [hassard0.github.io/Mighty](https://hassard0.github.io/Mighty/).
   mkdocs-material, deploy on push to main with concurrency
   cancellation. `--strict` deliberately off (stale RFC + example
   source links from the v0.1 → v0.8 organic doc accretion); a
   v0.10 follow-up.
3. **Package-signing stub** (sigstore-style). The real `sigstore`
   Rust dep graph (Fulcio + Rekor + tonic) is too heavy for an MVP;
   we ship the shape (signing API + JSON manifest + signature file +
   verify path) with an explicit "STUB" mode so consumers can wire
   real Fulcio later without an API change.
4. **CI hardening**: nightly-matrix + minimal-version + strict + MSRV
   jobs added; cargo + rustup caches pinned for reproducible builds.
5. **Release scripts** (`scripts/release.sh` + `release.ps1`):
   reproducible tag + push + GH release + asset upload flow.
6. **`KNOWN_ISSUES.md`** (new): canonical list with v0.10-target
   resolutions and reproduction hints.

See [`RC_PREP_V0_9_NOTES.md`](RC_PREP_V0_9_NOTES.md) for per-task
interpretation calls.

## Verification

| Gate | v0.8.0 | v0.9.0 | Delta |
|---|---|---|---|
| `cargo build --workspace` | clean | clean | — |
| `cargo test --workspace` | 927 / 0 / 7 | **955 / 0 / 7** | +28 passed (parser non-progress regressions + new self-host MtyIR cases) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean | clean | — |
| `cargo fmt --all -- --check` | clean | clean | — |
| 20-example matrix (typeck) | 20/20 | 20/20 | — |
| 20-example matrix (borrowck) | 20/20 | 20/20 | — |
| Demo smoke | 2/3 | **3/3** | +1 (demo 02 fixed) |
| Conformance (`conformance_full`) | passes | passes | — |
| Self-host: lexer | 4 | 4 | — |
| Self-host: parser | 13 | 13 | — |
| Self-host: HIR | 5 | 5 | — |
| Self-host: typeck | 5 | 5 | — |
| Self-host: MtyIR | — | 7 | new |
| **Self-host total** | 27 | **34** | +7 |
| Parser fuzz smoke (1 min) | n/a (no fuzz harness yet) | **0 OOM / 0 panic / 13 859 runs** | — |

The 7 ignored tests are the same v0.8-deferred set (HIR/typeck
examples 04 + 05, plus selfhost_ir 04 + 05, plus the http_server
doctest ignore).

## v1.0 freeze blockers

The v1.0 spec is now feature-complete at v1.0-RC2. The remaining
blockers before the spec can be promoted to v1.0-final:

1. **Two independent implementations** — the spec freeze policy
   inherited from the Rust/Swift/Zig precedent requires at least
   two interoperable implementations of every normative feature
   before final-spec. Mighty has one (this compiler). A second
   implementation effort is opened in **RFC-007** (planned for
   v0.10).
2. **RFC comment periods** — each of RFC-001 through RFC-006 needs
   a 30-day public comment window before its DEFER-V1.1 resolution
   becomes binding for v1.1. Comment windows are scheduled to open
   on the v0.10 tag.
3. **Conformance suite gaps** — v0.9 ships the conformance corpus
   that was good enough for v0.8; v1.0-final demands a published
   normative conformance suite that any v1.x implementation must
   pass. Tracked for v0.10.
4. **`demos/02_counter_web` real allocator** — the v0.9 bump
   `cabi_realloc` is correct for `old_ptr == 0` only. A real
   free-list / wee_alloc-style allocator is a v0.10 follow-up
   (KNOWN_ISSUES.md#1).

**Proposed v1.0 freeze date: 2026-09-01** (~3 months from v0.9 tag).
This gives time for the comment periods, the second-implementation
RFC, and the v0.10 conformance suite to land.

## v0.9 deferrals → v0.10

- Second independent compiler implementation (RFC-007, planned).
- Real `cabi_realloc` allocator (KNOWN_ISSUES.md#1; bump → free-list).
- mkdocs `--strict` site build (KNOWN_ISSUES.md#5).
- CI nightly + release-gate fuzz wiring (the harness is ready; the
  jobs need a custom rust-nightly + libFuzzer setup-step).
- Cranelift egraph stack-overflow upstream report + workaround
  (Bug 4 in FUZZ_V0_9_NOTES.md).
- Self-host HIR + typeck examples 04 + 05 (deferred from v0.8;
  the gap is the same in v0.9 since this slice didn't touch HIR).
- Full `TokenStream` marshalling for proc-macros (v0.8 deferral).
- `mty-pkg` cross-file resolution (`use selfhost_hir.HirFn`).
- Parametric newtypes (`type FnId = USize newtype`).
- WASM size optimisation (Target 5 from v0.8 perf swarm).
- HTTP-server throughput optimisation (Target 6 from v0.8 perf swarm).
- Set-of-scopes hygiene cleanup in LSP completion (A111).
- Real `sigstore` integration (replace the stub).

## Known issues

See [`KNOWN_ISSUES.md`](KNOWN_ISSUES.md) for the canonical list with
v0.10-targeted resolutions. Summary:

1. **`cabi_realloc` is a bump allocator** — correct for our current
   canonical-ABI lifts (only call with `old_ptr == 0`); a real
   free-list lands in v0.10.
2. **Parallel monomorphisation** still ships in-tree but
   `Monomorphizer::run()` dispatches to `run_sequential`. Awaiting
   a real-server-class measurement window.
3. **Set-of-scopes hygiene in LSP completion** (A111) — deferred
   post-1.0.
4. **Cranelift egraph stack overflow** on generic-slice input (Bug 4
   in FUZZ_V0_9_NOTES.md) — upstream, not in our code; workaround
   is to disable the egraph pass.
5. **mkdocs `--strict`** not enabled — stale links in the v0.1 → v0.8
   doc accretion; cleanup in v0.10.

## Stats

| | v0.8.0 | v0.9.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Source files (Rust + `.mty`) | 168 + 145 | 168 + 148 (self-host ir lib/nodes/lower) | +3 `.mty` |
| Rust source LoC | ~37 832 | ~37 832 + 2 387 fuzz/test (audit + non-progress tests + fuzz targets) | +2 387 |
| Self-host `.mty` LoC | ~3 600 | ~5 330 | +1 730 (MtyIR) |
| Tests passing | 927 | 955 | +28 |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes | 67+ | 67+ | 0 |
| Examples passing | 20/20 | 20/20 | 0 |
| Demos passing | 2/3 | 3/3 | +1 (02 fixed) |
| Self-host tests | 27 | 34 | +7 |
| Spec amendments | 88 (10 OPEN) | 88 (0 OPEN; 3 FREEZE-MVP + 7 DEFER-V1.1 + 6 RFCs) | resolved |
| RFCs | 0 | 6 first-drafts | +6 |
| Fuzz targets | 0 | 4 | +4 |
| Fuzz bugs found | — | 4 (3 P0 fixed in v0.9, 1 P1 upstream) | — |
| Commits since prior tag | — | **8** (7 swarm + 1 integrator) | — |
| Lines changed since prior tag | — | 161 files, +30 400 / -69 | — |

## Acknowledgments

v0.9 was built in a single overnight autonomous run by a four-agent
swarm:

- **spec-freeze-swarm** — promoted v1.0 spec to v1.0-RC2, resolved
  all 10 OPEN amendments, drafted 6 RFCs (commit `637312d`).
- **fuzz-swarm** — brought up 4 cargo-fuzz targets, ran 5-minute
  smoke per target, triaged 4 bugs with proposed fixes (commits
  `d977ea6`, `9e4980b`, `99c8676`).
- **selfhost-mtyir-swarm** — partial MtyIR lowering in Mighty (7/9
  tests), gap catalog (commits `cb38c8a`, `dc1538a`).
- **rc-prep-swarm** — demo 02 fix, Pages site, package-signing stub,
  CI hardening, release scripts, KNOWN_ISSUES.md (commit `69a3cb7`).

The integrator pass (commits `4f85e8e` plus this v0.9.0 tag commit)
closed the parser non-progress-guard family (Bug 1, 2, 3 + 7
audit-sweep extras + 16 regression tests + parser_fuzz artifact
replay), re-verified the gates (955 tests / clippy / fmt / 20-example
matrix / demos / self-host), and authored this slice document plus
`RELEASE-v0.9.md` and the [`PARSER_AUDIT_V0_9.md`](PARSER_AUDIT_V0_9.md).

See the `*_V0_9_NOTES.md` family for per-agent interpretation calls:

- [`SPEC_FREEZE_V0_9_NOTES.md`](SPEC_FREEZE_V0_9_NOTES.md)
- [`FUZZ_V0_9_NOTES.md`](FUZZ_V0_9_NOTES.md)
- [`SELFHOST_IR_V0_9_NOTES.md`](SELFHOST_IR_V0_9_NOTES.md)
- [`RC_PREP_V0_9_NOTES.md`](RC_PREP_V0_9_NOTES.md)
- [`PARSER_AUDIT_V0_9.md`](PARSER_AUDIT_V0_9.md) (this integrator slice)
