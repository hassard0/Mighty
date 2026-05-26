# Changelog

All notable changes to Mighty are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

For the full per-release notes, see
[`dev/history/releases/`](dev/history/releases/).

## [Unreleased]

- v1.0-RC4 work: extend Python 2nd-impl through HIR + sketch typeck
  (~5.5 KLOC, ~8 days); wire 6 remaining Gap-B typeck call-sites
  (MT2003/MT2009/MT2022/MT2023/MT2024/MT2025); fix the v0.12
  red-shirt `borrow_checking/14_borrow_outlives_owner` by extending
  the `BinOp::Assign` branch in `record_borrow_for_rhs` to stamp
  `pending_borrower`; run `go test ./...` on the Go 3rd-impl
  (`impl-go/`) on a Go-1.22+ host and cross-validate against Rust +
  Python over the `examples/` sweep; MT0001 funnel split
  (MT0002/MT0003/MT0010/MT0011/MT0012/MT0020/MT0021/MT0030);
  `mty-pkg` cross-file resolution; parametric newtypes for self-host
  arena ids; v0.13 RFC-009 set-of-scopes wiring into mty-hir + LSP
  completion (A111); WASI P2 stdlib lowerings + preview1-adapter
  embed + default flip to P2 (closes v0.13 issue #15); effect-row
  surface-syntax parser + typeck call-site validator + rest of
  stdlib HOFs + MT4020-25 diagnostics (v0.13 RFC-008 follow-up);
  self-host codegen broadening (string pool, pattern lowering, ADT
  layout, for-loop iter); normative conformance suite kit
  publication; full `TokenStream` marshalling.

## [0.13.0] - 2026-05-25

**Capability tier — end-to-end self-host complete + WASI Preview 2 +
2 new RFCs (effect rows + set-of-scopes hygiene).** The Mighty
compiler front-end + Wasm core-module back-end is now implemented in
Mighty source for the slice-1 subset:
[`selfhost/codegen/wasm.mty`](selfhost/codegen/wasm.mty) (~400 LOC)
closes the bootstrap chain lexer → parser → HIR → typeck → MtyIR →
wasm codegen, with 6/6 live driver tests passing (1 ignored — example
03's generic `Option[T]`). **The self-host milestone called for since
the v0.5 lexer port is reached.** A WASI Preview 2 backend lands
behind `--wasi=p2` (default stays `p1`): new `--world <name>` flag, a
new `[wit]` section in `mighty.toml` for user-supplied WIT, a vendored
`wasi:*@0.2.3` slice covering `cli`/`io`/`clocks`/`filesystem`/`http`/
`random`, example at [`examples/21_wasi_preview2.mty`](examples/21_wasi_preview2.mty),
user-facing matrix at [`docs/reference/wasi.md`](docs/reference/wasi.md).
Two new RFCs land with usable infrastructure: **RFC-008 effect-row
polymorphism** (`!E`, `!{a | E}`, four-case unification, subsumption)
with a 450-LOC row module in `crates/mty-types/src/effects.rs::row`
and a relaxed `stdlib_list_map_sig()`; and **RFC-009 set-of-scopes
macro hygiene** (Flatt-style scope sets) with `scopes.rs` + `hygiene.rs`
+ a new `expand_scoped()` entry point alongside the legacy mangler.
Both ship as **SHIPPED-SUBSET**: infrastructure + tests + first wired
consumer, with v0.14 follow-ups for surface-syntax parsing
(RFC-008) and mty-hir rewire (RFC-009). The spec stays at v1.0-RC3;
the conformance corpus stays at 89 cases / 16 categories / 3 ignored.
**1051 Rust + 137 Python + 89 conformance + 46 self-host = 1323 tests
passing** (+82 vs v0.12), 0 failing, 5 ignored.
[Release notes](dev/history/releases/RELEASE-v0.13.md).

## [0.12.0] - 2026-05-25

**Spec-and-evidence tier — v1.0-RC3 spec released + 4th showcase
demo + conformance Gap B/C/E partial closure + Go 3rd-impl source
landed.** The normative spec advances **v1.0-RC2 → v1.0-RC3**:
operator precedence is promoted to normative §11.1.1 (was deferred
to non-normative `docs/internals/parser.md`); the full reserved
keyword set is enumerated (63 reserved + 4 contextual + 7
reserved-for-future); the 16 Python-impl spec findings from v0.11
are codified in prose (+396 spec lines, no behaviour change). A
fourth runnable showcase lands at [`demos/04_kvstore/`](demos/04_kvstore/)
— a sharded supervised in-memory key-value store (~400 LOC)
exercising agents + protocols + supervisors + restart-on-crash +
`std.http` end-to-end (the first demo whose pitch is the
supervisor restart story). The conformance corpus gains six new
fixtures (typeck 17..20, borrow 13..14) and a real MT3007
`BORROW_OUTLIVES_OWNER` emit-site in `mty-borrow/src/flow.rs`;
the harness now reports **89 cases / 16 categories / 3 ignored**
(one new red-shirt: `borrow_checking/14_borrow_outlives_owner`
needs `pending_borrower` wired through plain assignments —
deferred to v0.13). A Go 3rd-impl lands at
[`impl-go/`](impl-go/): 4848 LOC of lexer + parser + CLI + tests,
built from `docs/spec/v1.0-rc.md` (v1.0-RC3) prose alone, with
zero peeking at `crates/mty-*`, `selfhost/`, or `impl-py/`. The
Go toolchain is not installed on the v0.12 build host so
`go test ./...` has not been run; cross-validation pending v0.13.
**Closes KNOWN_ISSUES #10 (operator precedence not normative) and
#12 (`package`/`export`/`requires` keywords not in §3.3).** **977
Rust + 135 Python + 89 conformance + 40 self-host = 1241 tests
passing**, 0 failing, 3 ignored. [Release notes](dev/history/releases/RELEASE-v0.12.md).

## [0.11.0] - 2026-05-25

**Quality tier — strict-clippy gate green + Python 2nd-impl partial
+ conformance gap closure + UX polish.** The `clippy (strict)` CI
job is now **required** (no more `continue-on-error: true`) and
clean across the whole 20-crate workspace: 2341 pedantic warnings
on baseline → 0 via a workspace-level `[lints.clippy]` allowlist
plus ~30 real fixes. **All six CI jobs now run as required gates.**
An independent Python implementation of the Mighty front-end lands
at [`impl-py/`](impl-py/): pure-Python lexer + parser (~2.5 KLOC)
built from the v1.0-RC2 spec prose alone (no peeking at
`crates/mty-syntax`, `crates/mty-ast`, or `selfhost/`); **135 tests
passing, 20/20 examples lex+parse**. **Real partial credit on v1.0
freeze blocker #1** (two independent implementations). The slice
also surfaced 16 spec findings — biggest: operator precedence is
not in the normative §11 (deferred to `docs/internals/parser.md`)
and needs to be promoted before v1.0 freeze. Normative conformance
corpus grows **88% → 91% FROZEN coverage** (62% → 70% direct), 4 of
8 documented gaps closed with two harness extensions
(warning-severity assertions; per-case `mighty.toml` via `CwdGuard`)
plus 3 new positive-fire cases (MT2012, MT6003, MT6008); the 4
deferred gaps each have a precise crate-source-edit reason recorded.
UX polish: 15 high-traffic MTxxxx codes rewritten to a consistent
Cause/Example/Fix/Spec format, all 16 tour chapters refreshed
(`.sd` → `.mty`, spec links bumped to `v1.0-rc.md`), FAQ extended
12 → 26 entries, getting-started rewritten 187 → 290 lines.
Inherited from post-v0.10.0 `main`: three macOS codegen fixes
(`LC_BUILD_VERSION` on Mach-O objects + cosmetic + CI tolerance for
missing `cc`). **977 Rust tests + 135 Python tests = 1112 total.**
[Release notes](dev/history/releases/RELEASE-v0.11.md).

## [0.10.0] - 2026-05-25

**Production cleanup + conformance audit.** Lifts the v0.9 RC-prep
stubs to real implementations: `cabi_realloc` becomes a segregated
free-list allocator (8 size classes + bump tail), sigstore signing
gets a real keyless path behind the `sigstore-real` feature (default
keeps the v0.9 SHA-256 envelope shape), the Cranelift egraph fuzz
bug is filed upstream as
[wasmtime #13476](https://github.com/bytecodealliance/wasmtime/issues/13476)
with an in-tree `MTY_CRANELIFT_NO_OPT` workaround and a new
`MTY_DUMP_CLIF` debug knob. Conformance corpus grows 16 → 81 cases
(88% FROZEN coverage). Self-host examples 04 + 05 deferrals closed —
**40/40 selfhost tests now pass**. CI hardened: MSRV gate now runs
`cargo test --no-run` + bedrock subset; `mkdocs --strict` enabled
with all 55 stale links fixed; cargo-audit job added; parallel
monomorphisation honestly reverted to sequential default after
re-benching. Major repo cleanup: 62 dev artefacts archived under
`dev/history/`, README rewritten 421 → 210 lines, root
`CHANGELOG.md` introduced, license switched from Apache-2.0/MIT dual
to **MIT-only**, repo URL bumped `hassard0/stardust` →
`hassard0/Mighty`. **977 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.10.md).

## [0.9.0] - 2026-05-24

**RC-prep + freeze-readiness.** Spec promoted to **v1.0-RC2** with all
10 OPEN amendments resolved (3 FREEZE-MVP, 7 DEFER-V1.1) and six
follow-up RFCs drafted (RFC-001..RFC-006). Brought up a four-target
cargo-fuzz harness (parser / typeck / fmt / codegen) with 27-file seed
corpus, fixed three P0 OOM parser bugs the fuzzer surfaced, and did an
audit sweep over every sibling `loop` for the same anti-pattern.
Self-hosted the MtyIR lowering on examples 01-03 (joining the v0.5
lexer, v0.6 parser, v0.8 HIR + minimal typeck — **34 self-host tests
passing**). Fixed `demos/02_counter_web`'s long-standing
`cabi_realloc` regression (3/3 demos passing again). Published the
[GitHub Pages docs site](https://hassard0.github.io/Mighty/), hardened
CI (stable/beta/nightly matrix, minimal-versions, strict, MSRV), shipped
reproducible release scripts, and landed a sigstore-style package
signing stub. **955 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.9.md).

## [0.8.0] - 2026-05-24

**Loose-end closure + self-host HIR + perf + spec v1.0-RC.** Closed 4 of
5 remaining v0.5 loose ends (proc-macro sandboxed execution with
MT6007/MT6008, real per-agent HTTP routing, LSP cross-file workspace
resolve, WIT canonical-ABI return-area for DOM strings). Self-hosted
the HIR + minimal typeck phases (~1.1 KLOC of Mighty source; 5+5 new
self-host tests). Three of four perf optimisations landed (parse +27%,
mailbox +7%, ~800 ns agent-send). Consolidated 88 spec amendments into
**v1.0-RC** at `docs/spec/v1.0-rc.md`. Closed all rebrand residuals
(runtime ABI symbols, DWARF producer, bench fixture). **927 tests
passing.** [Release notes](dev/history/releases/RELEASE-v0.8.md).

## [0.7.0-rebrand] - 2026-05-24

**Stardust → Mighty rename.** Naming-only release: 20 `sdust-*` crates
renamed to `mty-*`, `.sd` → `.mty` source extension, `star.toml`/`star.lock`
→ `mighty.toml`/`mighty.lock`, `SD####` → `MT####` diagnostic codes
(with `SD` aliases preserved for `mty explain`), WIT `stardust:*` →
`mty:*`, VS Code extension repackaged. **0 behavioural deltas — 885
tests pass byte-for-byte against v0.6.0.**
[Release notes](dev/history/releases/RELEASE-v0.7.md).

## [0.6.0] - 2026-05-24

**Multi-core + benchmarks + self-host parser.** Runtime now distributes
work across N OS threads via per-worker tokio runtimes + crossbeam-deque
work-stealing + affinity hints + lightweight migration + per-worker
stats. First honest benchmarks shipped — new `mty-bench` crate covers
six categories with Rust/Go/C++ comparators. Self-host parser subset
(~1930 LOC, 13/13 bootstrap tests, examples 01-05 covered). DOM MtyIR
lowering reaches `emit_dom_call` end-to-end. MT6001-MT6006 macro codes
merged into the central `mty-diagnostics` catalog. Per-call `FsCap`
isolation contract test. **885 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.6.md).

## [0.5.0] - 2026-05-24

**Self-hosting + dogfood completion.** Loops actually terminate via
`break`/`continue`/iterator exhaustion (bounded-fixed-point loop
borrows). Self-host lexer now round-trips byte-for-byte against the
Rust lexer. Five v0.4 dogfood stopgaps replaced with real
implementations (real `std.http.serve` over TCP, Wasm DOM imports as
a 4-method WIT interface, full `Str` method table, MtyIR
mem-budget auto-charge, `FsCap` allowlist process-wide). Macros
completion: `name!(args)` invocation, extended hygiene, cross-file
`pub macro`, proc-macro skeleton, stdlib macros. LSP advanced —
semantic tokens, rename, inlay hints, code actions, signature help,
workspace folders, semantic completion. **839 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.5.md).

## [0.4.0] - 2026-05-24

**Dogfood + ecosystem.** Three end-to-end dogfood demos
(`01_search_api`, `02_counter_web`, `03_extract_tool`) with passing
smoke scripts. Real package registry transport over GitHub Releases
REST with on-disk index cache + sha256 sidecar + deterministic
`.tar.gz` bundles + three new CLI subcommands. Hygienic declarative
macros (MT6001..MT6004 catch unknown/arity/depth/bad-arg). Self-host
lexer subset bootstrap. MtyIR loop terminator fix — loops genuinely
iterate. **692 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.4.md).

## [0.3.0] - 2026-05-25

**Soundness hardening.** Borrow checker grew NLL last-use deactivation
and field-level Places. Type checker grew scope-aware tolerance and
the formal `Sendable` trait (MT3011 at every send/ask site). Runtime
grew cooperative mid-turn cancellation, OTLP wire-format telemetry,
and slab-pool mailbox frames. Closed v0.2 cleanup backlog: stdlib
install, 6/20 wasm-CM gaps, 3 of 5 INTENTIONALLY_IGNORED conformance
cases. **623 tests passing, 20/20 wasm Components.**
[Release notes](dev/history/releases/RELEASE-v0.3.md).

## [0.2.0] - 2026-05-24

**LSP + pkg + doc + DWARF + Wasm CM + stdlib.** Closed every bullet on
the v0.1 deferral list: LSP 3.17 server with VS Code scaffold, package
manager (resolver + lockfile + path/git fetchers + publisher), doc
generator (markdown + HTML + search index), real stdlib (`std.json`,
`std.tls`, `std.http`, `std.fs`, `std.time`, `std.test`) backed by
rustls/hyper/serde_json/tokio, DWARF v4 debug info + wasm source maps,
Wasm Component Model output by default (`wit-component`). 20/20 native
+ 20/20 wasm core-module compilation. **550 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.2.md).

## [0.1.0] - 2026-05-24

**First feature-complete release.** Walked the full spec §31 roadmap
across eight slices: parser → formatter → HIR → type checker → borrow
checker → effects/capabilities/traits → MtyIR + interpreter → runtime
MVP → native (Cranelift JIT + AOT) + Wasm core module codegen. `mty
new` / `check` / `fmt` / `dump` / `run` / `build` / `explain`. 65+
diagnostic codes across MT0xxx..MT8xxx. MSRV Rust 1.85. **376 tests
passing.** [Release notes](dev/history/releases/RELEASE-v0.1.md).

[Unreleased]: https://github.com/hassard0/Mighty/compare/v0.11.0...HEAD
[0.11.0]: https://github.com/hassard0/Mighty/releases/tag/v0.11.0
[0.10.0]: https://github.com/hassard0/Mighty/releases/tag/v0.10.0
[0.9.0]: https://github.com/hassard0/Mighty/releases/tag/v0.9.0
[0.8.0]: https://github.com/hassard0/Mighty/releases/tag/v0.8.0
[0.7.0-rebrand]: https://github.com/hassard0/Mighty/releases/tag/v0.7.0-rebrand
[0.6.0]: https://github.com/hassard0/Mighty/releases/tag/v0.6.0
[0.5.0]: https://github.com/hassard0/Mighty/releases/tag/v0.5.0
[0.4.0]: https://github.com/hassard0/Mighty/releases/tag/v0.4.0
[0.3.0]: https://github.com/hassard0/Mighty/releases/tag/v0.3.0
[0.2.0]: https://github.com/hassard0/Mighty/releases/tag/v0.2.0
[0.1.0]: https://github.com/hassard0/Mighty/releases/tag/v0.1.0
