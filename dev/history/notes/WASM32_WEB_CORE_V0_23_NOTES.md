# v0.23 Track B — wasm32-web embedded-core-module investigation

## Scope

The original swarm brief claimed that `mty build --target wasm32-web`
emits a "2.1 KB Component-header-only wasm with no embedded core
module," and that demo `02_counter_web` "ships with this since v0.4"
but the page works only because the JS shim doesn't need the wasm
exports. Track B was supposed to make `wasm32-web` emit a Component
that EMBEDS a real core module.

## Investigation summary (HEAD `fa2522b`)

The bug as described **does not reproduce** on `main` at
`fa2522b post-v0.22: harden work_stealing test for fast runners`.

Concrete measurements (from a probe test, since deleted, that ran
`compile_program_to_file_with_options(..., WasmTarget::Web, ..)`):

| source                                | bytes | core preambles @offsets |
|---------------------------------------|-------|--------------------------|
| empty `mty_ir::Program::default()`    | 1966  | 189, 1034, 1419          |
| `common::empty_main()` (single `main`)| 2020  | 189, 1049, 1434          |
| `examples/01_hello.mty`               | 2055  | 189, 1084, 1469          |
| `demos/02_counter_web/src/main.mty`   | 2082  | 189, 1111, 1496          |

Every artifact has:

- Component preamble at offset 0 (`00 61 73 6d 0d 00 01 00`).
- A real core-Wasm module section starting at offset 189
  (preamble `00 61 73 6d 01 00 00 00`).
- Two additional core preambles further in (the
  `wit-component`-injected adapter modules — string conversion / dummy
  realloc — and the post-link module concatenation).

That matches the demo's `findCoreModule` walker (which just scans for
`\0asm\x01\x00\x00\x00`): it finds the first preamble at 189 and the
counter_web demo instantiates cleanly. `smoke.sh` step 4
(`grep -aFq "mty:web/log" "$WASM"`) also confirms the embedded core
module imports the `mty:web/log` interface.

In short, `compile_program_to_file_with_options` on `WasmTarget::Web`
ALREADY:

1. Runs `compile_program_to_bytes_with_preview(prog, Web, ..)` to
   produce a real core module (>= 800 bytes, function/code/export
   sections present).
2. Generates a WIT document via `emit_wit(prog, name, Web)`.
3. Wraps via `wrap_as_component(core, doc)`, which embeds the core
   module under a Component Model section.

The wasm32-web path is not a stub. The original "no embedded core
module" diagnosis seems to have been a misread of the artifact size:
a Component with a small core module + the wit-component scaffolding
+ two adapter modules lands around 2KB even for an empty program,
because the Component framing dominates over the user core module
body for tiny inputs.

## Why the bug report still mattered

Even though the embedding is correct today, there is no automated
regression test that locks in this invariant. The only existing check
is `demos/02_counter_web/smoke.sh` step 4, which greps for the
literal string `mty:web/log` and would not catch:

- A future refactor that ships only the Component preamble + types
  with no embedded core (the originally feared regression).
- A refactor that embeds an empty core module (no function section).
- A refactor that swaps `Module(&core_bytes)` for `Module(&[])` in
  `wit-component` (silently produces a valid Component without the
  user's code).
- Pure size regressions where the core module loses content but the
  artifact still passes the `mty:web/log` grep.

Track D (the JS-shim / browser smoke unlocker) will lean on these
invariants whenever it walks the wasm to instantiate. So the
deliverable is a five-test regression harness that fails LOUDLY the
moment any of those silent regressions land.

## Plan

1. Land a regression test file
   `crates/mty-codegen-wasm/tests/wasm32_web_core.rs` containing five
   tests against `WasmTarget::Web` + `BuildOptions::new(...)` output:
   - core preamble somewhere inside the Component bytes,
   - core module has a non-empty function section,
   - `main` is exported by the embedded core module,
   - artifact size grows past a non-trivial threshold for a non-trivial
     program (locks in the embedding — a Component-only wasm would
     stay around 800 bytes),
   - both Component and the embedded core module validate under
     `wasmparser::Validator::new_with_features(WasmFeatures::all())`.
2. No code changes to `emit.rs` / `wit.rs` / `lib.rs` were necessary —
   the wasm32-web emission path is already correct. The notes file is
   added so future agents grepping for "wasm32-web core module" find
   this investigation instead of redoing it.

## Files touched

NEW:

- `crates/mty-codegen-wasm/tests/wasm32_web_core.rs` (regression
  harness, 5 tests).
- `dev/history/notes/WASM32_WEB_CORE_V0_23_NOTES.md` (this file).

EXTENDED: none — `emit.rs`, `wit.rs`, and `lib.rs` are unchanged.

## Acceptance evidence

- `cargo build -p mty-codegen-wasm` clean.
- `cargo test -p mty-codegen-wasm --test wasm32_web_core` 5/5 pass.
- `cargo clippy -p mty-codegen-wasm --no-deps -- -D warnings` clean.
- `cargo fmt -p mty-codegen-wasm -- --check` clean.
- `bash demos/02_counter_web/smoke.sh` still PASSes.

Workspace-level `cargo build` is currently blocked by an unrelated
Track C edit in `crates/mty-cli/src/main.rs` (Cmd::New now takes a
second `template` arg). That is outside Track B's ownership.

## Byte-size delta on `examples/01_hello.mty`

Before this slice: 2055 bytes (Component + embedded core module).
After this slice:  2055 bytes (no codegen changes — the slice only
added a test file + this notes doc). The "Component-header-only"
baseline (2.1 KB without an embedded core) was the **assumed**
starting state in the brief; the actual starting state already had
the core module embedded.

## Track D unlocker confirmed

Track D needs three things from Track B:

1. `wasm32-web` artifact contains the Component preamble at offset 0.
2. Somewhere in the Component bytes there is a core module with a
   real function section (so `WebAssembly.instantiate` of the
   extracted core actually does something).
3. The embedded core module exports `main` (the JS shim's entry
   point) — verified by `wasm32_web_main_export_in_core` test.

All three are already true on HEAD `fa2522b` and are now locked in by
the new regression tests. Track D can proceed.
