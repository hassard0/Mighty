# Changelog

All notable changes to Mighty are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

For the full per-release notes, see
[`dev/history/releases/`](dev/history/releases/).

## [Unreleased]

- v1.0-RC2 work: second-implementation effort (RFC-007), MT0001 funnel
  split (MT0002/MT0003/MT0010/MT0011/MT0012/MT0020/MT0021/MT0030),
  `mty-pkg` cross-file resolution, parametric newtypes for self-host
  arena ids, set-of-scopes hygiene in LSP completion (A111), normative
  conformance suite kit publication, full `TokenStream` marshalling.

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

[Unreleased]: https://github.com/hassard0/Mighty/compare/v0.10.0...HEAD
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
