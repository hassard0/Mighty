# Debug-info v0.2 — interpretation notes

These notes record the design calls made while shipping wave-2
debug-info support. They live at the repo root so they're easy to find
during v0.3 follow-up work.

## Scope

Three deliverables landed:

1. New crate `sdust-debuginfo` with `DwarfBuilder` (wraps `gimli::write`
   for DWARF v4) and `SourceMap` + `NameSection` (wasm source-map v3 +
   wasm `name` custom section).
2. Cranelift backend: `compile_object_with_debug` attaches a DWARF
   section bundle to the emitted object via `object::write::Object`.
   Per-platform section naming (ELF/COFF `.debug_*`, Mach-O
   `__DWARF.__debug_*`) is handled in `attach_dwarf_sections`.
3. Wasm backend: a new `sourcemap.rs` module produces the `name`
   custom section + writes `<binary>.wasm.map`. Driver-level
   `attach_wasm_debug_info` re-reads the emitted wasm and appends both
   the `name` section and a `sourceMappingURL` section.

## Interpretation calls

### DWARF version → 4 (not 5)

DWARF 5 emits `.debug_info` differently (unit-headers carry a unit-type
byte + new addr/line/loclists sections). DWARF 4 is widely supported by
older `gdb`/`lldb` on developer machines. We picked v4 for v0.2 since
gimli `write` supports both equally and 4 is the path of least
resistance for `objdump --dwarf=info` everywhere we tested.

Trade-off: we don't get `.debug_loclists` (5-only) — but we don't emit
location lists yet anyway. Switching to v5 is a one-line change in
`DwarfBuilder::new` once we want it.

### Address::Constant, not Address::Symbol

The plan called for full `Address::Symbol { symbol, addend }`
references so the linker patches `low_pc`/`high_pc`. We deferred that
to v0.3 because:

1. Cranelift's `ObjectModule::define_function` doesn't currently expose
   the per-fn symbol id back to the lowerer in a way that's easy to
   thread into a separate DWARF pass.
2. Without relocations, lldb still walks the DIE tree and answers
   `image lookup -n main` correctly; only live source-line stepping
   against runtime virtual addresses doesn't work.

The fix is a bigger plumbing change (pass `ObjectProduct.functions[]`
into `DwarfBuilder`, generate one relocation per `low_pc`/`high_pc`
attribute). Tracked as a v0.3 follow-up.

### One DWARF base-type per type-name, not per fn

We intern `DW_TAG_base_type` DIEs on the compile-unit by display name.
Two fns returning `i32` share the same type DIE. Simpler than
maintaining a per-fn type cache and matches what rustc + clang produce.

### Line program: one row per fn-entry (v0.2)

Per-instruction source mapping requires plumbing cranelift's
`MachSrcLoc` events back to the SIR span source. SIR statements don't
yet carry `SourceSpan` (only `Function::span` does). v0.3 will add
per-stmt spans and regenerate per-instr line rows.

For v0.2 we emit one row per fn entry. `objdump --dwarf=line` shows the
fn entry; lldb's `image lookup` correctly answers fn-level queries; the
line program is structurally valid (`gimli::read` parses it
round-trip).

### `DW_LANG_Rust` for `DW_AT_language`

DWARF doesn't have a Stardust constant. `Rust` is the closest match
semantically (ownership-aware, monomorphized, mid-IR-similar to MIR);
rust-lang/rust assigns the value `0x001c` for it.

### Wasm: append custom sections post-emit, not in `Emitter`

The Wasm CM agent owns `Emitter` in wave-2. To avoid contention we
chose to append custom sections in a separate pass (`append_debug_sections`)
that re-reads the wasm bytes. This is robust to the Component Model
wrapper because:

- Custom sections are valid anywhere in a core wasm module.
- `wit-component::ComponentEncoder` preserves unknown custom sections
  from the inner core module.
- The wasm validator accepts any number of custom sections at the end.

### Source-map: one mapping per fn, fn-index as generated offset

v0.2 doesn't have real wasm byte offsets for each fn entry (the
encoder doesn't expose code-section offsets). We use `f.id.0` as a
stand-in for `generated_offset`. Once `wasm-encoder` exposes
per-fn offsets (or we track them by parsing the emitted bytes), we'll
switch to real offsets and per-instr mappings.

DevTools handles the coarse mapping gracefully — it just snaps to the
nearest mapping when hovering, which for v0.2 always means "the fn
this address is in."

## Acceptance

- `cargo test -p sdust-debuginfo` — 13 unit tests + 2 round-trip tests
- `cargo test -p sdust-codegen-cranelift` — 24 lib tests + 2 debug
  integration tests (one verifies `DW_TAG_subprogram` for `main`
  appears in the linked-object DWARF, one verifies plain
  `compile_object` emits no `.debug_*` sections)
- `cargo test -p sdust-codegen-wasm --test sourcemap` — 3 tests
  (`name` custom section present, sidecar is valid source-map v3 JSON
  with `sourcesContent`, `sourceMappingURL` references sidecar
  filename)
- `cargo build -p sdust-cli` — clean
- `--debug` (default) now actually emits debug info; `--release`
  strips it

## Deferred to v0.3

| Item | Effort | Notes |
|------|--------|-------|
| `Address::Symbol` for low_pc/high_pc | medium | requires plumbing ObjectProduct.functions[] into DwarfBuilder + adding relocations |
| Per-instr line program | medium | needs SIR-statement SourceSpan + cranelift MachSrcLoc plumbing |
| `.debug_loc` per-local location lists | medium | needs cranelift slot-offset extraction |
| `name` subsection id 2 (locals) | small | trivial once we want it |
| Per-stmt wasm source-map mappings | small | gated on SIR-statement spans + wasm-encoder byte offsets |
| DWARF for the LLVM backend | large | build host lacks LLVM; not a v0.2 target |
| Inlining info (`DW_TAG_inlined_subroutine`) | large | needs inliner first |
| Generics info | medium | needs typed monomorphization metadata |
