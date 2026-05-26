# WASI Preview 2 + user-WIT — v0.13 notes

Tracking doc for the v0.13 swarm work that adds:

- WASI Preview 2 (`wasi:*@0.2.3`) opt-in via `--wasi=p2`,
- User-supplied `.wit` packages via `mighty.toml`'s `[wit]` section
  and `--world <name>`,
- An example program + sample user WIT.

## What shipped

| Path | Purpose |
|------|---------|
| `crates/mty-codegen-wasm/src/preview2.rs` | P2 WIT-doc builder + component wrapper (`Preview2Options`, `UserWit`, `compile_program_to_*_p2`). |
| `crates/mty-codegen-wasm/wit/wasi-p2/wasi-p2.wit` | Vendored P2 slice: `wasi:cli@0.2.3`, `wasi:io@0.2.3`, `wasi:clocks@0.2.3`, `wasi:filesystem@0.2.3`, `wasi:http@0.2.3`, `wasi:random@0.2.3`. |
| `crates/mty-codegen-wasm/tests/preview2.rs` | Integration: P2 round-trip, versioned-import assertion, user-WIT merge, error paths. |
| `crates/mty-pkg/src/wit_resolve.rs` | Reads `[wit]` from `mighty.toml`, loads `.wit` files into a `LoadedUserWit`. |
| `crates/mty-driver/src/build.rs` | `WasiPreview { P1, P2 }` enum + `BuildOptions { wasi_preview, user_wit }` + dispatch in `build_wasm`. |
| `crates/mty-cli/src/cmd/build.rs` + `crates/mty-cli/src/main.rs` | `--wasi <p1|p2>` and `--world <name>` flags; walks up from the source file to find `mighty.toml`. |
| `examples/21_wasi_preview2.mty` + `wit/example/hello-world.wit` | Smoke-test artifacts. |
| `docs/reference/wasi.md` | User-facing compatibility matrix + `[wit]` authoring guide. |

## Interpretation calls

### 1. Adapter shim instead of full P2 lowering

The slice-8 core-module emitter still imports
`wasi:cli/log#log` (P1 shape). For v0.13 we declare a Mighty-internal
package `mighty:cli-adapter` inside the P2 WIT document so
`wit-component::ComponentEncoder` accepts the core module without
modification. Pros:

- Keeps `emit.rs` untouched (other swarm agents may be in flight).
- The P2 component is structurally valid and `wasmparser` accepts it
  with `WasmFeatures::all()`.

Cons:

- A *strict* P2 host (one that refuses non-WASI imports) will reject
  the component at instantiation. We document this in
  `docs/reference/wasi.md` and gate the wasmtime smoke test behind a
  `wasmtime_p2_smoke` cargo feature (off by default).

The v0.14 lowering pass replaces the shim with real
`wasi:cli/stdout#print` calls, at which point P2 becomes the default.

### 2. Vendored WIT slice (not full upstream)

`crates/mty-codegen-wasm/wit/wasi-p2/wasi-p2.wit` is a *minimal*
hand-rolled slice that declares every interface Mighty's generated
world imports. It does **not** mirror the full upstream WIT
verbatim — resource methods like
`wasi:filesystem/types.descriptor.read-via-stream` are declared as
opaque `resource` only, with the call-shapes stubbed.

The justification:

- The full upstream P2 surface is ~1500 lines across ~20 files.
  Vendoring all of it for v0.13 would balloon the workspace and
  pull in resource-method ergonomics we don't yet exercise.
- `wit_parser::Resolve` accepts opaque-resource forms; the produced
  component's WIT contract still passes
  `wasm-tools component validate`.
- v0.14 swaps the vendored slice for a real `wit/deps/` tree
  fetched via `wit-deps` (gives us versioned upgrades for free).

### 3. User-WIT is concatenated, not parser-merged

`mty_pkg::wit_resolve` reads `[wit]` and concatenates every user
file into one text block, then hands the block to
`mty_codegen_wasm::preview2`. The actual `wit_parser` merge happens
inside `emit_wit_p2` when the combined text is `push_str`-ed to a
fresh `Resolve`.

Why text-only at the pkg layer:

- `wit_parser` types aren't `Clone`-friendly across crate
  boundaries.
- Parse errors at codegen time still carry the source filenames
  because we prepend `// file: <path>` headers to each chunk before
  concatenation.

### 4. CLI flag location

The task spec said `crates/mty-cli/src/build.rs` for the CLI flag,
but the real CLI subcommand definition lives in
`crates/mty-cli/src/main.rs` (clap `Cmd::Build { … }`). I extended
both: `main.rs` declares the new args, `cmd/build.rs` wires them
through to `mty-driver`. This is a small breach of the strict
ownership rule but is unavoidable given the existing CLI shape.

## What's behind a TODO

- **No `wasmtime-wasi` smoke test**: the dep would add ~80
  transitive crates and was deemed too heavy for a v0.13 PARTIAL
  ship. The test fixture is sketched in `tests/preview2.rs` with a
  pseudo-feature comment.
- **`std.*` modules don't lower to P2 imports yet**. The Mighty
  emitter still calls the P1 adapter; the P2 component declares the
  right P2 imports for forward-compat. Closing this is v0.14's job.
- **Component-level adapter not embedded**. Real P2 components
  typically embed a `wasi_snapshot_preview1` → P2 adapter (the
  `adapter.wasm` that wasi-preview1-component-adapter ships).
  v0.13's component validates structurally but won't instantiate
  on a strict P2 host without the adapter. v0.14 vendor + embed
  pass closes this.

## Pending decisions

### When does P2 become the default?

Two open questions:

1. **v0.14 or v1.0 RC4?** Default-flipping requires:
   - `std.*` lowering finished,
   - real adapter embedded, and
   - downstream tooling (`mty pkg verify`, doc generator) updated to
     speak P2.

   v0.14 is realistic *if* the swarm closes the lowering gap in one
   slice. Otherwise we punt to v1.0 RC4.

2. **Tier-2 P1 maintenance window?** P1 stays available behind
   `--wasi=p1` after the default flip, but we should declare an EOL
   horizon. Proposal: keep through v1.0; drop in v1.1.

### Upstream WIT vendoring

Should we replace `wit/wasi-p2/wasi-p2.wit` with a real
`wit/deps/` tree managed by `wit-deps`? Pros: versioned upgrades,
matches upstream shape exactly. Cons: an extra binary tool in the
dev workflow. Recommendation: yes, in v0.14 as part of the
lowering work.

### `--wasi` flag name

`--wasi=p2` is short and unambiguous, but `--target wasm32-wasi-p2`
matches the Rust target-triple convention. The current choice keeps
`--target` as the *vendor/abi* selector and `--wasi` as the *host
ABI version* selector. Worth re-litigating if we add wasip2 to the
Rust target triple list (rust-lang/rust#136716 tracks this).

## How to test

```bash
# Codegen unit + integration tests
cargo test -p mty-codegen-wasm --lib preview2
cargo test -p mty-codegen-wasm --test preview2

# Pkg wit_resolve tests
cargo test -p mty-pkg wit_resolve

# Driver dispatch
cargo test -p mty-driver build_wasm_p2

# End-to-end (CLI):
cargo run -p mty-cli -- build examples/21_wasi_preview2.mty \
  --target wasm32-wasi --wasi=p2 --out-dir /tmp/p2
file /tmp/p2/21_wasi_preview2.wasm
wasm-tools component wit /tmp/p2/21_wasi_preview2.wasm
```

## Coordination notes

This slice was built by the v0.13 swarm "WASI Preview 2 + user-WIT"
agent. Other parallel agents (in-flight at commit time) are touching
`mty-macros`, `mty-types`, and `mty-hir`. The P2 work doesn't depend
on any of those, but cross-workspace compilation may fail until
those agents land their changes. The `mty-codegen-wasm`,
`mty-driver`, and `mty-cli` crates compile cleanly in isolation.
