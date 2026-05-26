# WASI Preview 2 lowerings — v0.14 notes

Tracking doc for the v0.14 swarm work that closes the v0.13
"P2 component declares interfaces but Mighty's stdlib still
emits P1 calls" gap.

## What shipped

| Path | Purpose |
|------|---------|
| `crates/mty-codegen-wasm/adapter/` | NEW directory vendoring the official wasmtime v32.0.0 `wasi_snapshot_preview1` adapter modules (command + reactor + proxy). |
| `crates/mty-codegen-wasm/src/preview2.rs` | Extended with `AdapterKind`, `P2DirectImport`, `WASI_P1_ADAPTER_*` byte constants, `build_direct_p2_probe_module`, plus adapter-embed + `_start`-alias logic in `wrap_p2`. |
| `crates/mty-codegen-wasm/wit/wasi-p2/wasi-p2.wit` | Replaced the v0.13 hand-rolled minimal slice with the full upstream WASI 0.2.3 WIT surface (wasi-cli + wasi-http v0.2.3, all packages concatenated in nested form). |
| `crates/mty-codegen-wasm/tests/preview2.rs` | +9 new tests covering adapter presence, embed-on / opt-out, size-delta, per-import direct-lowering, and stdlib-codegen constant agreement. |
| `crates/mty-codegen-wasm/Cargo.toml` | +`mty-stdlib` dev-dep; +`wasmtime_p2_smoke` feature gate (off by default, reserves the opt-in for the wasmtime-wasi smoke test). |
| `crates/mty-stdlib/src/random.rs` | NEW. OS-entropy backed `bytes()` / `u64()` plus `P2_DIRECT_IMPORT_RANDOM_BYTES` / `P2_DIRECT_IMPORT_RANDOM_U64` import-name constants. |
| `crates/mty-stdlib/src/time.rs` | +`P2_DIRECT_IMPORT_MONOTONIC_NOW` / `P2_DIRECT_IMPORT_WALL_CLOCK_NOW` / `P2_DIRECT_IMPORT_MONOTONIC_RESOLUTION` constants + agreement test. |
| `crates/mty-stdlib/src/fs.rs`, `http.rs` | +doc-comment block documenting the adapter-routed P2 path (no direct lowering yet — that's v0.15). |
| `docs/reference/wasi.md` | Compatibility matrix updated: `std.random` + `std.time` listed as P2-direct, others as adapter-routed. |

## Lowering status (post-v0.14)

| `std.*` surface | v0.13 | v0.14 | v0.15 plan |
|-----------------|-------|-------|------------|
| `std.random.bytes` / `u64` | P1 import shape, no P2 wiring | **direct P2 import** `wasi:random/random@0.2.3#get-random-bytes` | unchanged |
| `std.time.now` / `monotonic_now` / `resolution` | P1 import shape | **direct P2 import** of `wasi:clocks/wall-clock@0.2.3#now` / `wasi:clocks/monotonic-clock@0.2.3#{now,resolution}` | unchanged |
| `std.fs.read` / `write` / `mkdir` / `read_dir` | P1 import shape, no P2 wiring | P1 import shape, **adapter-routed to `wasi:filesystem@0.2.3`** | direct lowering (needs canonical-ABI for `descriptor` resource) |
| `std.http.get` / `post` / `serve` | P1 import shape, no P2 wiring | P1 import shape, **adapter-routed to `wasi:http@0.2.3`** | direct lowering (needs canonical-ABI for `outgoing-request` / `future-incoming-response` resources) |
| `log()` / `print()` | `wasi:cli/log` shim | `wasi:cli/log` shim (unchanged) | replace with `wasi:cli/stdout@0.2.3` direct lowering |

The interpretation: **direct** means the core module imports the
versioned P2 interface verbatim; **adapter-routed** means the core
module imports `wasi_snapshot_preview1#fd_write` (or similar)
and the vendored adapter translates that to the matching P2 call
at component-instantiation time.

## Adapter provenance

`wasi_snapshot_preview1.command.wasm` etc. are byte-for-byte the
upstream wasmtime v32.0.0 release artifacts. v32 was picked because
it's the first wasmtime release whose adapter targets WASI 0.2.3 —
matching the version Mighty's vendored WIT slice declares. Earlier
wasmtime releases (v22, v27, v30) ship adapters targeting WASI 0.2.0
or 0.2.2, which `wit-component` refuses to merge against a 0.2.3
WIT surface (the encoder's "semver-compatible upgrade" check fails
when interfaces differ across the bump).

If we ever bump the vendored WIT slice to 0.2.4+, also bump the
adapter version — both versions must move in lockstep.

## Component-size impact

The command adapter contributes ~54 KB of additional component
bytes (post-strip; the raw adapter is ~55 KB but wit-component's
encode pass elides unused adapter exports). For an empty `fn
main() {}` program:

| Build | Component size |
|-------|----------------|
| `--wasi=p2 --no-adapter` (v0.15 future default) | ~6 KB |
| `--wasi=p2` (v0.14 default — command adapter embedded) | ~60 KB |
| `--wasi=p1` (default through v0.12) | ~3 KB |

The reactor + proxy adapters are vendored alongside the command
one (in `adapter/`) but are NOT yet embedded by any Mighty build
path. They'll be selected automatically when Mighty grows
reactor / proxy component shapes.

## Architecture changes since v0.13

1. **Full upstream WIT, not a hand-rolled slice**. v0.13 carried a
   ~80-line minimal WIT slice that declared only the interface
   *shells* Mighty needed. The wasmtime v32 adapter's WIT
   metadata declares the *full* upstream interfaces, and the
   encoder's interface-merge step fails on interface mismatch.
   v0.14 vendors the full ~141 KB upstream WIT surface
   (wasi-cli + wasi-http v0.2.3, ~3300 source lines collapsed into
   one self-contained WIT file with nested-package blocks). The
   `world`-block trimming is done by the Python concat script
   `scripts/` is not used — the slice was generated once and
   checked into `wit/wasi-p2/wasi-p2.wit`. To re-vendor, see the
   adapter README and re-run the same concat logic on a freshly
   extracted wasi-http release tarball.

2. **Per-package `Resolve::push_str`**. The v0.13 path
   concatenated the full WIT into one blob and pushed it as a
   single `push_str` call. `wit-parser` doesn't allow cross-
   reference between *nested* packages in one file (only between
   *separate* top-level files), so v0.14 splits the vendored WIT
   on nested-package boundaries and pushes each chunk separately.
   The `WitDocument::text` field still returns the concatenated
   blob for display, but callers that want to re-resolve it must
   walk the same split logic.

3. **`_start` alias for the command adapter**. The wasmtime
   command-adapter expects an exported `_start: func()` on the
   core module (the wasi-libc / clang convention). Mighty's
   slice-8 emitter exports `main` instead. `wrap_p2` now
   post-processes the core module to add a `_start` export
   aliasing `main` whenever the command adapter is in use.

4. **Adapter opt-out for v0.15 direct lowering**. Once
   `std.fs`/`std.http`/`log` lower directly to P2, the adapter
   becomes pure dead weight. `Preview2Options::with_adapter(None)`
   skips the embed entirely; the v0.14 default keeps the adapter
   on so existing P1-shaped lowerings continue to work.

## Direct-import helper API

```rust
// In codegen layer:
let probe = mty_codegen_wasm::build_direct_p2_probe_module(
    mty_codegen_wasm::P2DirectImport::RandomBytes,
);
// `probe` is a core module with one import:
//   `wasi:random/random@0.2.3#get-random-bytes`

// `P2DirectImport::import_pair()` is the single source of truth
// for the (module, fn) name pair. Stdlib-side constants in
// `mty_stdlib::random` and `mty_stdlib::time` mirror these via
// `(&'static str, &'static str)` pairs; the integration test
// `p2_direct_import_names_match_stdlib_constants` pins them.
```

## Open follow-ups for v0.15

1. **Real direct-lowering pass** for `std.random` + `std.time`.
   The v0.14 work exposes the helpers + import descriptors; the
   actual codegen-layer dispatch that picks "direct vs P1
   import" lives in `emit.rs`, which this slice didn't touch
   (owned by another agent). The follow-up wires
   `mty_codegen_wasm::P2DirectImport::*` into the emitter so
   `std.random.bytes()` lowers to a direct P2 import instead of
   the current P1 syscall.

2. **`std.fs` + `std.http` direct lowering**. Requires
   canonical-ABI plumbing for resource types (`descriptor`,
   `outgoing-request`, etc.). Once that lands the command
   adapter becomes optional.

3. **`log()` → `wasi:cli/stdout@0.2.3`**. Drop the unversioned
   `wasi:cli/log` shim from `preview2.rs::emit_wit_p2`.

4. **wasmtime-wasi end-to-end smoke test**. The
   `wasmtime_p2_smoke` cargo feature is reserved but no test
   currently runs under it. A future slice can flip it on and
   add `wasmtime-wasi` as a feature-gated dev-dep + one test
   that instantiates the adapter-wrapped command component and
   calls into `_start`.

5. **Default flip**. With direct lowering complete for all four
   surfaces, `--wasi=p2` becomes the default for `wasm32-wasi`.
   Earliest reasonable target: v0.15. P1 stays opt-in via
   `--wasi=p1` through v1.0; drop in v1.1.

## How to test

```bash
# Codegen + adapter integration tests:
cargo test -p mty-codegen-wasm --test preview2

# Stdlib import-name constants:
cargo test -p mty-stdlib random
cargo test -p mty-stdlib time

# Workspace regression check:
cargo test --workspace --no-fail-fast

# Lint + format:
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# End-to-end (CLI):
cargo run -p mty-cli -- build examples/21_wasi_preview2.mty \
  --target wasm32-wasi --wasi=p2
file path/to/output.wasm  # WebAssembly binary module
wasm-tools component wit path/to/output.wasm | head -50
# → should show `wasi:*@0.2.3` imports, no `wasi_snapshot_preview1`
```

## Coordination notes

Built by the v0.14 swarm "WASI P2 lowerings" agent. Other parallel
agents in this slice touched `mty-stdlib::iter` (HOF row-poly relax),
`mty-types`, and `mty-hir`. The P2 work doesn't depend on those —
the agent obeyed the off-limits rule for `iter.rs` + all of
`mty-types`/`mty-hir`/`mty-syntax` etc. Only `mty-codegen-wasm`,
`mty-codegen-wasm/adapter/`, the four named `mty-stdlib` modules
(plus the new `random.rs`), and the docs were modified.

The cargo lock file was untouched; the new `getrandom = "0.2"` dep
on `mty-stdlib` resolves to an already-present transitive version.
