# Mighty v0.8 — Release Notes

**Tag:** `v0.8.0`
**Date:** 2026-05-24
**Status:** SHIPPED — feature release. v0.8 closes 4 of 5 remaining
v0.5 loose ends, self-hosts the HIR + minimal typeck phases, lands 3
of 4 perf-optimisation targets, consolidates 88 spec amendments into
the v1.0 release-candidate spec, and closes the rebrand residuals
the v0.7 rebrand agent missed.

If you were on v0.7.0-rebrand, the upgrade is `git pull && cargo
install --path crates/mty-cli --force`. There are no source-level
breaking changes for end-user Mighty programs. The runtime ABI symbol
names changed (see "Runtime ABI" below) — this matters only if you
were linking the Mighty runtime from a foreign-language host without
going through the `mty` driver, which no one should be doing.

## Highlights

- **v0.5 loose ends closed (4 of 5)**: proc-macro sandboxed
  execution, real per-agent HTTP routing, LSP cross-file workspace
  resolve, WIT canonical-ABI return-area for DOM string returns.
- **Self-host HIR + minimal typeck** — `selfhost/hir/lower.mty`
  (~960 LOC) and `selfhost/typeck/infer.mty` (~153 LOC) join the
  v0.5 lexer and v0.6 parser. 27 self-host tests pass at HEAD
  (lexer 4 / parser 13 / HIR 5 / typeck 5).
- **Parse throughput +27%** (85 → 108 MiB/s on the 10 KLOC
  fixture), **mailbox throughput +7%** on the slab fast path,
  **agent send latency** ~800 ns for the empty-payload fast path
  (was ~1.6 µs).
- **v1.0-RC spec published** at `docs/spec/v1.0-rc.md`. 88
  amendments classified: 63 FROZEN, 15 SUPERSEDED, 10 OPEN, 0
  REVERTED. 12 cross-amendment contradictions reconciled.
- **Rebrand residuals closed**: runtime ABI symbol names, DWARF
  producer string, `mty-bench` fixture name, `mty-doc` template
  comments, `mty-hir` insta snapshot headers, plus back-compat
  fallbacks for legacy `sd` / `stardust` code-block tags in
  `mty-doc`.
- **927 tests passing** (was 885 at v0.7.0-rebrand; +42 net).

## What's new

### Proc-macro sandboxed execution (A107)

`proc macro name(input: TokenStream) -> TokenStream { … }` bodies
now actually run, on a sub-interpreter with:

- **Wall-clock timeout**: 100 ms (dedicated thread + step budget
  for sync cancellation).
- **Step cap**: 100,000 interpreter steps.
- **Memory cap**: 16 MB.
- **Effect isolation**: any `effect_call` from the macro body traps
  through `ProcMacroHost` and is mapped to `MT6007`
  (`proc_macro_impure_at_runtime`).
- **Resource breaches** are mapped to `MT6008`
  (`proc_macro_resource_exceeded`).

Token-stream marshalling is the deliberate v0.8 simplification —
`input` is a `Str` of the call-site token text, and the return
value is treated as rewritten source text. Full `TokenStream`
modelling is post-1.0.

### Real per-agent HTTP routing (A108)

The slice-7 `mty-runtime` HTTP server accepted one global handler
per port. v0.8 installs a real `HttpRouter` keyed by
`(method, path, agent)`, dispatching incoming requests to the right
agent's `on Request` handler. Single-handler programs are
unaffected.

### LSP cross-file workspace resolve (A109)

The v0.5 LSP only resolved within a single file. v0.8 builds a
workspace map from `mighty.toml` + the package tree, enabling
go-to-definition across `.mty` files in the same package.

### WIT canonical-ABI return-area for DOM string returns (A110)

The wasm-component DOM bindings dropped string returns onto the
wrong canonical-ABI slot, causing string-typed DOM ops to read
garbage in component-model hosts. v0.8 writes the `(ptr, len)` pair
to a per-call return-area buffer per the canonical ABI spec.

### Self-host HIR + minimal typeck

Two new self-host files port the HIR lowering and the minimal HM
typeck to Mighty itself:

- `selfhost/hir/lower.mty` (~960 LOC). Round-trips byte-for-byte
  against the Rust `mty-hir` for examples 01-03.
- `selfhost/typeck/infer.mty` (~153 LOC). Minimal HM-style inference
  for the same example subset.

Examples 04 + 05 are ignored with explanatory messages — they
exercise Result-sugar return + `?` operator + struct-literal
expressions (04) and range patterns + private-fn name mangling
(05). Both are v0.9 follow-ups.

The selfhost work surfaced five language gaps documented with
concrete v0.9 fix proposals in `SELFHOST_HIR_V0_8_NOTES.md`.

### Performance optimisations

| Target | Status | Mechanism |
|---|---|---|
| Parse throughput | **+27%** | 64-byte token cache + ±1-token widen for incremental re-lex; `ParseOpts::max_diagnostics` throttle (default `usize::MAX`, LSP uses 256) |
| Mailbox throughput | **+7%** | `SlabPool::acquire_empty()` fast path: tombstone `PooledFrame` for `SmallPayload::Empty`, skipping parking_lot lock + Vec alloc + slot write |
| Agent send latency | **~800 ns** (was ~1.6 µs) on `try_send_empty` | `Mailbox::try_recv_many()` free function on the raw receiver, avoiding Arc-deref on the drain path |
| Compile time (parallel mono) | **honest revert** | `Monomorphizer::run_parallel` ships in-tree but `run()` dispatches to `run_sequential` after measurement showed parallel was slower for typical M < ~1000 generic-instantiation workloads |

The mailbox `tracks_slot_usage` test was updated to pin the v0.8
empty-FP contract (commit `207cd64`): empty payloads do NOT
consume slab slots; non-empty payloads still admit through the slab.

### Spec consolidation v1.0-RC

The 88 amendments accumulated through v0.1 → v0.7 are folded into
a single normative release-candidate spec:

- `docs/spec/v1.0-rc.md` (NEW) — normative v1.0-RC.
- `docs/spec/v0.1-amendments.md` (EDITED) — each amendment now
  carries a `**Status:**` line (FROZEN 63 / SUPERSEDED 15 / OPEN 10
  / REVERTED 0).
- `docs/spec/CHANGELOG.md` (NEW).
- `scripts/classify_amendments.py` (NEW) — reproducible status-
  line injector.

No source code was touched by this slice. 12 cross-amendment
contradictions were reconciled — see `SPEC_CONSOLIDATION_V0_8_NOTES.md`
§"Contradictions" for the full table.

## Rebrand residuals — what changed

These were the real misses the v0.7 rebrand agent's pattern matched
on the abbreviated `sdust_` form but not the full `stardust_` form:

### Runtime ABI symbol names (A)

The LLVM + Cranelift codegen back-ends emit `extern "C"` calls into
the Mighty runtime. The full list:

| Old symbol | New symbol |
|---|---|
| `stardust_runtime_log` | `mty_runtime_log` |
| `stardust_runtime_print` | `mty_runtime_print` |
| `stardust_runtime_panic` | `mty_runtime_panic` |
| `stardust_runtime_arena_push` | `mty_runtime_arena_push` |
| `stardust_runtime_arena_pop` | `mty_runtime_arena_pop` |
| `stardust_runtime_alloc` | `mty_runtime_alloc` |
| `stardust_runtime_budget_charge` | `mty_runtime_budget_charge` |
| `stardust_runtime_send` | `mty_runtime_send` |
| `stardust_runtime_ask` | `mty_runtime_ask` |
| `stardust_runtime_spawn` | `mty_runtime_spawn` |
| `stardust_runtime_extern_call` | `mty_runtime_extern_call` |
| `stardust_runtime_log_i64` | `mty_runtime_log_i64` |

This change is invisible to Mighty source programs — the rename is
internal to the compiler/runtime boundary. **It matters only** if
you were dynamically loading `libmty_runtime` from a foreign-
language host and resolving these symbols by name. If so, update
the strings in your loader.

### Other residuals (B-F)

- DWARF producer string: `"stardust-0.2"` → `"mighty-0.8"`.
- `mty-bench` fixture: `stardust_10kloc()` → `mty_10kloc()`.
- `mty-doc` template comments: `sdust-doc` → `mty-doc`.
- `mty-doc` code-block recognition: still recognises `mty` /
  `mighty` as primary, plus `sd` / `stardust` as legacy back-compat
  fallbacks so existing docs render correctly.
- `mty-hir` insta snapshot source headers: regenerated +
  back-patched to `source: crates/mty-hir/...`.

## New diagnostic codes

- `MT6007` `proc_macro_impure_at_runtime` — runtime effect leak from
  a proc-macro body (separate from `MT6005`'s static detection).
- `MT6008` `proc_macro_resource_exceeded` — generic resource-bound
  breach inside a proc-macro execution (wall-clock / memory / steps).

`mty explain MT6007` and `mty explain MT6008` ship in v0.8.

## Backwards-compat aliases (status)

The v0.7 aliases all stay live:

- `mty dump --sir` aliases `--ir` ✅
- `mty explain SD####` accepts legacy `SD` prefix ✅
- `--legacy-interp` flag unchanged ✅
- v0.8 adds: `mty-doc` recognises legacy `sd` / `stardust`
  code-block tags ✅

All slated for removal in a future major release per A45.

## Stats

| | v0.7.0-rebrand | v0.8.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Source files (Rust + `.mty`) | 168 + 143 | 168 + 145 | +2 |
| Rust source LoC | ~36 200 | ~37 832 | +1 632 |
| Tests passing | 885 | 927 | +42 |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes | 65+ | 67+ | +2 (MT6007, MT6008) |
| Examples passing | 20/20 | 20/20 | 0 |
| Demos passing | 3/3 | 2/3 | -1 (pre-existing, documented) |
| Self-host tests | 17 | 27 | +10 |
| Spec amendments | 88 (loose) | 88 (classified) | consolidated |
| Lines changed | — | 87 files, +11 960 / -390 | — |
| Commits since prior tag | — | 15 | — |

## Migration steps

For end-user Mighty packages: **none required**. Source-level Mighty
programs from v0.7.0-rebrand compile and run unchanged on v0.8.0.

For foreign-language hosts dynamically loading `libmty_runtime`:
update the symbol-name strings per the "Runtime ABI symbol names"
table above.

For consumers of the `mty-bench` library: rename
`mty_bench::fixtures::stardust_10kloc` to
`mty_bench::fixtures::mty_10kloc` in your imports.

## Known issues

1. **`demos/02_counter_web/smoke.sh` fails** with `module does not
   export a function named cabi_realloc`. Reproduces at
   `v0.7.0-rebrand` — pre-existing wasm-component synthesis
   regression from the v0.4-v0.5 codegen refactor. Tracked for v0.9.

2. **Parallel monomorphisation** regresses on typical M < ~1000
   workloads. Ships in-tree as `Monomorphizer::run_parallel` but
   `run()` dispatches to `run_sequential`. Re-evaluate on real-
   server-class hosts for v0.9.

3. **Set-of-scopes hygiene in LSP completion** (A111) deferred
   post-1.0.

## v0.8 → v0.9 roadmap

The v1.0 release candidate is one slice away. Planned for v0.9:

- Self-host HIR + typeck examples 04 + 05 (Result-sugar return + `?`,
  struct-literal expressions, range patterns, private-fn name
  mangling).
- Full `TokenStream` marshalling for proc-macros.
- `mty-pkg` cross-file resolution (`use selfhost_hir.HirFn`).
- Parametric newtypes (`type FnId = USize newtype`) for self-host
  arena ids.
- WASM size optimisation (Target 5).
- HTTP-server throughput optimisation (Target 6).
- `demos/02_counter_web` wasm-component `cabi_realloc` fix.
- Set-of-scopes hygiene cleanup in LSP completion (A111).

After v0.9 the spec is frozen at v1.0-final and the release sequence
becomes a v1.0.0 RC → GA promotion.

## Acknowledgments

v0.8 was built in a single overnight autonomous swarm: loose-ends
(4 commits), selfhost (2 commits), perf (4 commits), spec
consolidation (1 commit). The integrator pass closed the rebrand
residuals, ran the cross-cut clippy + fmt + test gates, authored
`SLICE_V0_8.md` and these release notes, and cut the `v0.8.0` tag.

See `LOOSE_ENDS_V0_8_NOTES.md`, `SELFHOST_HIR_V0_8_NOTES.md`,
`PERF_V0_8_NOTES.md`, `BENCHMARKS_V0_8_NOTES.md`, and
`SPEC_CONSOLIDATION_V0_8_NOTES.md` for per-agent interpretation
calls.
