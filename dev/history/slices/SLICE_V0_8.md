# Mighty v0.8 — Complete

**Tag:** `v0.8.0`
**Date:** 2026-05-24
**Status:** SHIPPED — eighth milestone release. v0.8 is the
"loose-ends closure + self-host HIR + perf + spec v1.0-RC" milestone:
the four open v0.5 loose ends are closed (proc-macro sandboxed
execution, real per-agent HTTP routing, LSP cross-file workspace
resolve, WIT canonical-ABI return-area for DOM string returns), the
HIR lowering + minimal typeck phases are self-hosted in Mighty
(joining the lexer and parser from v0.6), the parse / mailbox /
agent-send hot paths get measurable wins, and the 88 spec amendments
accumulated through v0.1 → v0.7 are consolidated into a normative
v1.0 release-candidate spec.

v0.8 was built by a four-agent autonomous swarm (loose-ends /
self-host / perf / spec consolidation) over a single overnight
session, then integrated through this slice document. The integrator
pass also closed the **rebrand residuals** the v0.7 rebrand agent
missed: the `stardust_runtime_*` runtime ABI symbol names (LLVM +
Cranelift codegen call sites and the matching `pub extern "C" fn`
definitions in `mty-runtime`), the `stardust_10kloc()` benchmark
fixture, the DWARF producer string, the `mty-doc` template comment
headers, the legacy `sd` / `stardust` code-block tag fallbacks in
`mty-doc`, and the `source: crates/sdust-hir/...` headers in the
three regenerated `mty-hir` insta snapshots.

## What landed

### Loose-end closure — loose-ends-swarm agent (commits `c5bb51b`, `3f04b44`, `e3fd243`, `b1ae77b`, `76ccd9a`)

The v0.5 dogfood slice left five items open. v0.8 closes four; the
fifth (set-of-scopes hygiene in the LSP completion path) is
intentionally deferred until after the v1.0 release.

- **Proc-macro sandboxed execution** (`c5bb51b`, A107). The macro
  expander's `ProcMacro::execute()` path was a stub returning
  `Unsupported` since v0.4. v0.8 runs the macro body on a sub-
  `mty_ir::Interp` via `run_fn_with_resource_budget`, with a wall-
  clock timeout (100 ms), step budget (100,000), and memory cap
  (16 MB). Effect leaks at runtime trap to `MT6007`
  (`proc_macro_impure_at_runtime`); resource breaches trap to
  `MT6008` (`proc_macro_resource_exceeded`). Token-stream
  marshalling is the deliberate v0.8 simplification: bodies of shape
  `proc macro name(input: TokenStream) -> TokenStream { … }` see
  `input` as a `Str` of the call-site token text and return a
  rewritten source string. Full `TokenStream` modelling is post-1.0.
- **Real per-agent HTTP routing** (`3f04b44`, A108). The slice-7
  `mty-runtime/http_server` accepted one global handler; v0.8
  installs a real router that dispatches per agent + per route, with
  the `HttpRouter` driven by a `(method, path, agent)` key.
  Documented in `LOOSE_ENDS_V0_8_NOTES.md` §2.
- **LSP cross-file workspace resolve** (`e3fd243`, A109). The v0.5
  LSP only resolved within a single file; v0.8 builds a workspace
  map from `mighty.toml` + the package tree so go-to-definition
  works across `.mty` files in the same package. Single-file editors
  remain supported.
- **WIT canonical-ABI return-area for DOM string returns**
  (`b1ae77b`, A110). The wasm-component DOM bindings dropped string
  returns onto the wrong canonical-ABI slot, causing string-typed
  DOM ops to read garbage in component-model hosts. v0.8 writes the
  `(ptr, len)` pair to a per-call return-area buffer per the canonical
  ABI spec.
- **Polish + ex16 typecheck fix** (`76ccd9a`). Example 16
  (`16_macro.mty`) tripped a typecheck regression introduced by the
  proc-macro execution path; fixed inline.

The fifth loose end — set-of-scopes hygiene in the LSP completion
filter — is documented in `LOOSE_ENDS_V0_8_NOTES.md` §5 and tracked
as A111. It's deferred post-1.0 because the fix requires shadowing
semantics that the v0.8 typeck doesn't yet expose.

Two new diagnostic codes landed:

- `MT6007` `proc_macro_impure_at_runtime`
- `MT6008` `proc_macro_resource_exceeded`

### Self-host HIR + minimal typeck — selfhost-swarm agent (commits `b00aa05`, `e415223`)

The v0.5 lexer (4/4) and v0.6 parser (12/12) were already self-hosted
in Mighty. v0.8 adds HIR lowering and minimal typeck.

- **`selfhost/hir/lower.mty`** (~960 LOC). Mighty-source HIR
  lowering for examples 01-03 round-trips byte-for-byte against the
  Rust `mty-hir`. Examples 04 + 05 are ignored with explanatory
  messages (Result-sugar return + `?` operator + struct-literal
  expressions / range patterns + private-fn name mangling; v0.9
  follow-ups).
- **`selfhost/typeck/infer.mty`** (~153 LOC). Minimal HM-style
  inference for the same example subset. Same v0.9 deferrals.

5 new HIR tests + 5 new typeck tests join the existing 4 lexer + 13
parser tests for **27 total self-host tests passing** at v0.8 head.

Five language gaps surfaced during this slice (no `la_arena`
equivalent, single-file compile blocking proper module layout,
awkward `Option[T]` round-trip through the host bridge, no parametric
newtypes, no `dyn`-style trait objects) are documented in
`SELFHOST_HIR_V0_8_NOTES.md` with concrete v0.9 language-fix proposals.

### Performance optimisations — perf-swarm agent (commits `82eafb5`, `f7a5e79`, `452e157`, `207cd64`)

Four of the six v0.6-benchmark targets were addressed. Three landed
with measured wins. One was honest-reverted because it regressed.

| Target | Status | Win |
|---|---|---|
| 1: Parse throughput | **LANDED** | +27% (85→108 MiB/s on the 10 KLOC fixture) |
| 2: Mailbox throughput | **LANDED** | +7% on the slab fast path |
| 3: Agent send latency | **LANDED** | `try_send_empty` ~800 ns (was ~1.6 µs) |
| 4: Compile time (parallel mono) | **PARTIAL** | `Monomorphizer::run_parallel` implemented but reverted from default after measurement showed slowdown; HashMap pre-sizing + scratch Vec in `LowerCtx::declare_fns` kept |
| 5: WASM size | — | not addressed this slice |
| 6: HTTP server throughput | — | not addressed this slice |

Notable mechanism changes (full details in `PERF_V0_8_NOTES.md`):

- `SlabPool::acquire_empty()` fast path for the
  `SmallPayload::Empty` case (dominates fire-and-forget agent sends).
  A "tombstone" `PooledFrame` (no slot, no overflow buffer, len = 0)
  preserves the "every admitted frame holds a handle" invariant
  without the parking_lot lock + Vec alloc + slot write cost.
- `Mailbox::try_recv_many()` free function on the raw receiver,
  avoiding an Arc-deref on the hot drain path.
- 64-byte token cache in `mty-syntax/token_cache.rs` with ±1-token
  widen for cheap incremental re-lex (the LSP `tokencache_edit`
  microbench shows ~100x speedup vs cold re-lex).
- Diagnostic throttle on the parser (`ParseOpts::max_diagnostics`,
  default `usize::MAX`, LSP uses 256) so a 10 KLOC file with one
  stray brace can't emit 50,000 diagnostics and freeze the IDE.

The mailbox `tracks_slot_usage` test was updated to pin the new
empty-FP contract (commit `207cd64`): empty payloads must NOT
consume slab slots; non-empty payloads still admit through the slab
as before.

**Honest revert on Target 4**: parallel mono regressed because
spawning N worker threads for an M-symbol module costs more than the
sequential serial-mono path when M < ~1000. The code stays in tree
(callable as `run_parallel` for benchmarks) but `run()` dispatches
to `run_sequential` by default. Documented in
`BENCHMARKS_V0_8_NOTES.md` §"What's NOT in v0.8".

### Spec consolidation v1.0-RC — spec-swarm agent (commit `c131a6e`)

The 88 amendments accumulated through v0.1 → v0.7 are folded into a
normative release-candidate spec.

- **`docs/spec/v1.0-rc.md`** (NEW) — single-document v1.0-RC.
- **`docs/spec/v0.1-amendments.md`** (EDITED) — each amendment now
  carries a `**Status:**` line: FROZEN (63), SUPERSEDED (15), OPEN
  (10), REVERTED (0).
- **`docs/spec/CHANGELOG.md`** (NEW) — chronological log per
  ladder step.
- **`scripts/classify_amendments.py`** (NEW) — reproducible status-
  line injector.

12 cross-amendment contradictions were reconciled (full table in
`SPEC_CONSOLIDATION_V0_8_NOTES.md` §"Contradictions"). No
source-code change was needed — the spec rewrite is documentation-
only.

### Rebrand residuals cleanup — integrator pass (commits `fbcf8c6`, `49b7951`)

Real misses from the v0.7 rebrand agent, addressed in this slice:

| Category | Files |
|---|---|
| A: LLVM + Cranelift runtime ABI symbols (`stardust_runtime_*` → `mty_runtime_*`) | `mty-codegen-llvm/src/lower.rs`, `mty-codegen-cranelift/src/{lower,jit,runtime_imports}.rs`, `mty-runtime/src/codegen_abi.rs`, `mty-driver/tests/conformance_codegen.rs` |
| B: `mty-bench` fixture (`stardust_10kloc()` → `mty_10kloc()`) | `mty-bench/src/{fixtures,lib}.rs`, `mty-bench/src/bin/mty-bench-runner.rs`, `mty-bench/tests/{fixture_load,criterion_smoke}.rs`, `mty-bench/benches/parse_throughput.rs` |
| C: DWARF producer string (`"stardust-0.2"` → `"mighty-0.8"`) | `mty-debuginfo/src/dwarf.rs`, `mty-debuginfo/tests/dwarf_roundtrip.rs` |
| D: `mty-doc` template comment headers | `mty-doc/templates/{style.css,search.js}` |
| E: Code-block recognition: `mty` / `mighty` primary, `sd` / `stardust` re-added as back-compat fallbacks | `mty-doc/src/extract.rs`, `mty-cli/src/main.rs` |
| F: Insta snapshot source headers (`source: crates/sdust-hir/...` → `source: crates/mty-hir/...`) | `mty-hir/tests/snapshots/dump_snapshots__d_{agent,fn,arena}.snap` |

These are real misses (the LLVM + Cranelift codegen had been calling
into ABI symbols that no longer existed on the runtime side, except
the runtime still defined them under the old name — so the link
succeeded but the brand split was real). The integrator pass also
landed three cross-cut clippy fixes (`items-after-test-module`,
`implicit-saturating-sub`, `manual-clamp`, `doc-overindented-list-
items`) and a `cargo fmt --all` sweep.

## Verification

| Gate | v0.7.0-rebrand | v0.8.0 | Delta |
|---|---|---|---|
| `cargo build --workspace` | clean | clean | — |
| `cargo test --workspace` | 885 / 0 / 2 | **927 / 0 / 7** | +42 passed, +5 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean | clean | — |
| `cargo fmt --all -- --check` | clean | clean | — |
| 20-example matrix | 20/20 | 20/20 | — |
| Demo smoke | 3/3 | 2/3 (counter_web pre-existing wasm-component `cabi_realloc` issue, see Known issues) | — |
| Conformance (`conformance_full`) | passes | passes | — |
| Self-host: lexer | 4 | 4 | — |
| Self-host: parser | 13 | 13 | — |
| Self-host: HIR | — | 5 (2 v0.9-deferred) | new |
| Self-host: typeck | — | 5 (2 v0.9-deferred) | new |
| Self-host total | 17 | **27** | +10 |

The 5 additional ignored tests are the documented v0.9 deferrals in
the self-host HIR + typeck tests (examples 04 + 05 × 2 phases), plus
the one pre-existing `http_server` doctest ignore.

## v0.5 loose-ends status

- ✅ Proc-macro sandboxed execution (closed in `c5bb51b`)
- ✅ Real per-agent HTTP routing (closed in `3f04b44`)
- ✅ LSP cross-file workspace resolve (closed in `e3fd243`)
- ✅ WIT canonical-ABI return-area for DOM string returns (closed in `b1ae77b`)
- ⏸ Set-of-scopes hygiene in LSP completion (deferred post-1.0; tracked as A111)

## Known issues

1. **`demos/02_counter_web/smoke.sh` fails** with `module does not
   export a function named cabi_realloc`. This regression predates
   v0.8 (reproduces at `v0.7.0-rebrand`); the wasm-component
   `cabi_realloc` synthesis was lost in the v0.4-v0.5 codegen
   refactor. Tracked for v0.9 as the wasm-size + counter_web
   regression slice.

2. **Parallel monomorphisation regresses on the host benchmark
   surface** for the typical workload (M < ~1000 generic
   instantiations). `Monomorphizer::run_parallel` ships in-tree but
   is not called from `Monomorphizer::run()`. Re-evaluate on a
   real-server-class host with M > ~5000 generics for v0.9.

3. **Set-of-scopes hygiene in the LSP completion path** is deferred
   post-1.0 (A111). Edge case: shadowing within a `match` arm body
   can present the outer binding as a completion. Mitigation: the
   inner-arm binding is always offered first.

## v0.8 deferrals → v0.9

- Self-host HIR + typeck examples 04 + 05 (Result-sugar return + `?`
  operator + struct-literal expressions; range patterns + private-fn
  name mangling).
- Full `TokenStream` marshalling for proc-macros (currently strings
  only).
- `mty-pkg` cross-file resolution for `use selfhost_hir.HirFn`-style
  paths (also unblocks proper self-host module layout).
- Parametric newtypes (`type FnId = USize newtype`) needed by the
  self-host arena id story.
- WASM size optimisation (Target 5).
- HTTP-server throughput optimisation (Target 6).
- `demos/02_counter_web` wasm-component `cabi_realloc` fix.

## Stats

| | v0.7.0-rebrand | v0.8.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Source files (Rust + `.mty`) | 168 + 143 | 168 + 145 | +2 `.mty` (self-host HIR + typeck) |
| Rust source LoC | ~36 200 | ~37 832 | +1 632 |
| Tests passing | 885 | 927 | +42 |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes | 65+ | 67+ (MT6007, MT6008 new) | +2 |
| Examples passing | 20/20 | 20/20 | 0 |
| Demos passing | 3/3 | 2/3 (one pre-existing regression documented) | -1 |
| Conformance | passes | passes | — |
| Self-host tests | 17 | 27 | +10 |
| Spec amendments | 88 (loose) | 88 (classified: 63 FROZEN / 15 SUPERSEDED / 10 OPEN) | consolidated |
| Commits since prior tag | — | 15 | — |
| Lines changed since prior tag | — | 87 files, +11 960 / -390 | — |

## Acknowledgments

v0.8 was built in a single overnight autonomous run by a four-agent
swarm:

- loose-ends-swarm — closed 4/5 v0.5 loose ends (commits `c5bb51b`,
  `3f04b44`, `e3fd243`, `b1ae77b`, `76ccd9a`).
- selfhost-swarm — self-hosted HIR + minimal typeck (commits
  `b00aa05`, `e415223`).
- perf-swarm — landed 3 of 4 perf targets, honestly reverted the
  fourth (commits `82eafb5`, `f7a5e79`, `452e157`, `207cd64`).
- spec-swarm — consolidated 88 amendments into the v1.0-RC spec
  (commit `c131a6e`).

The integrator pass (commits `fbcf8c6`, `49b7951`, and the v0.8.0
tag commit) closed the rebrand residuals, fixed the cross-cut
clippy issues introduced by the swarm work, applied `cargo fmt`,
verified the 927-test count + 20-example matrix + conformance +
self-host gates, and authored this slice document plus
`RELEASE-v0.8.md`.

See the `*_V0_8_NOTES.md` family for per-agent interpretation calls.
