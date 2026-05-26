# wasmtime bump — v0.17

## What changed

`crates/mty-codegen-wasm/Cargo.toml`:

```toml
# before
[dev-dependencies]
wasmtime = { version = "25", default-features = false, features = ["cranelift", "runtime"] }

# after
[dev-dependencies]
wasmtime = { version = "36", default-features = false, features = ["cranelift", "runtime"] }
```

`cargo update -p wasmtime` resolved the dev-dep graph to `wasmtime
v36.0.10` (latest stable at the time of bump was v45.0.0; we
deliberately stayed at the conservative `36` to minimise transitive
churn — the v0.17 mandate only requires `>= 36` to clear the
RUSTSEC advisory bundle).

Transitive deps that landed at the same time:

- `wasm-encoder` 0.217 → 0.236
- `wasmparser` 0.217 → 0.236
- `wasmprinter` 0.217 → 0.236
- `gimli` 0.29 → 0.32 (dev-dep tree only — workspace `gimli` is
  still pinned to `0.31` for debuginfo).
- `pulley-interpreter` 36.0.10 (new — pulley-machine fallback that
  replaces the v25 native-only cranelift backend).
- `wasmtime-internal-*` (renamed-from-`wasmtime-*-internal` family;
  wasmtime 36 reshuffled its internal crate names but the public
  `wasmtime::*` re-exports are unchanged).

## API changes encountered

**None.** The two test files that drive the wasmtime runtime
(`tests/cabi_realloc_real.rs` and `tests/canonical_abi_return.rs`)
use only the most stable wasmtime surface:

- `Engine::default()`
- `Module::new(&engine, bytes)`
- `Store::new(&engine, ())`
- `Linker::new(&engine)` + `linker.func_wrap(...)`
- `linker.instantiate(&mut store, &module)`
- `instance.get_typed_func::<...>(...)` / `instance.get_memory(...)`
- `TypedFunc<(...), ...>::call(...)`
- `Caller<'_, T>::get_export(...)`

All of these have stable signatures between wasmtime 25 and 36. No
test-side edits were required. `cargo build -p mty-codegen-wasm
--tests` is clean on the first attempt after the version bump.

The `preview2.rs` test file (`tests/preview2.rs`,
`tests/preview2_fs_http.rs`) doesn't link against the wasmtime
runtime at all — it only uses `wasmparser` + `wit-component` — so
the dev-dep tree change is invisible to it.

## Advisories cleared

The audit-bundle workaround landed in v0.16 as commit `7148b56`
ignored the following 15 RUSTSEC IDs across `audit.toml` and
`.cargo/audit.toml`. v0.17 removes them all — wasmtime 36 ships
the upstream fix for every one:

- RUSTSEC-2025-0046 — wasmtime: WASIp1 `fd_renumber` host panic
- RUSTSEC-2025-0118 — wasmtime: vmctx aliasing in JIT
- RUSTSEC-2026-0020 — wasmtime: WebAssembly `ref.*` type confusion
- RUSTSEC-2026-0021 — wasmtime: GC heap underflow
- RUSTSEC-2026-0085 — wasmtime: 0085-series component-model bundle
- RUSTSEC-2026-0086 — wasmtime: bundle
- RUSTSEC-2026-0087 — wasmtime: bundle
- RUSTSEC-2026-0088 — wasmtime: bundle
- RUSTSEC-2026-0089 — wasmtime: bundle
- RUSTSEC-2026-0090 — wasmtime: bundle
- RUSTSEC-2026-0091 — wasmtime: bundle
- RUSTSEC-2026-0092 — wasmtime: bundle
- RUSTSEC-2026-0093 — wasmtime: bundle
- RUSTSEC-2026-0094 — wasmtime: bundle
- RUSTSEC-2026-0095 — wasmtime: bundle
- RUSTSEC-2026-0096 — wasmtime: aarch64 Cranelift miscompile

(15 distinct wasmtime IDs cleared; the v0.16 list had a single
0085-bundle umbrella comment but enumerated entries 0085 through
0096 individually.)

## Advisories still ignored

Three remain:

1. **RUSTSEC-2024-0436** — `paste` is unmaintained. Transitive
   dependency via several macro crates (notably `inkwell` →
   `llvm-sys`). Not a vulnerability; tracked for replacement when
   we audit the LLVM backend's macro-stack.
2. **RUSTSEC-2025-0134** — `rustls-pemfile` is unmaintained.
   Transitive via the `reqwest` + `rustls` stack. Not a
   vulnerability; tracked for replacement when we bump `rustls`
   past the API that still imports the legacy pemfile parser.
3. **RUSTSEC-2026-0008** — `git2` 0.19.0 has an unsoundness in
   `Buf` deref. `mty-pkg` uses `git2` for registry fetches but
   doesn't touch the unsound API. Tracked separately for a `git2`
   bump (the bump is non-trivial because `git2` 0.20 changed the
   `Repository::clone` callback signature).

## Remaining work to clear all ignores

- `git2` bump (~half-day): bump workspace dep to 0.20, update
  `mty-pkg/src/registry/git.rs` clone-callback to the new closure
  signature. Removes RUSTSEC-2026-0008.
- `paste` replacement (~half-day): identify which transitive
  importer is still on `paste` (likely an old `inkwell` release)
  and either bump or vendor a non-`paste` macro shim. Removes
  RUSTSEC-2024-0436.
- `rustls-pemfile` removal (~half-day): bump `rustls` to a
  release whose default features don't pull in the legacy
  pemfile parser, or migrate to `rustls-pki-types::PrivateKeyDer`
  directly. Removes RUSTSEC-2025-0134.

After all three the audit ignore list can be emptied entirely.

## Verification

```
cargo build -p mty-codegen-wasm --tests   # clean
cargo test  -p mty-codegen-wasm           # all previously-passing
                                          # tests still pass; no API
                                          # regressions from the bump
```

Workspace-wide `cargo build --workspace` was deferred to the
orchestrator merge step because sibling agents on the
`feat/agent-replay-v0.17` branch were actively editing
`mty-hir/src/effects.rs`, `mty-codegen-wasm/src/preview2.rs`, and
several other source files during this slice — running a full
workspace build would have churned against their in-flight WIP.
The wasmtime bump itself touches only the dev-dep graph plus
audit.toml; no workspace source files were modified.
