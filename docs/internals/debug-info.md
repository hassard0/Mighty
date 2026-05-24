# Debug info (v0.2)

`sdust build --debug` produces artifacts that downstream debuggers can
load. v0.2 ships two surfaces:

| Target | Format | Tooling |
|--------|--------|---------|
| Native (cranelift) | DWARF v4 in the object file | `lldb`, `gdb`, `objdump --dwarf=info` |
| Wasm (core + Component Model) | `name` custom section + `<pkg>.wasm.map` sidecar | Chrome DevTools, `wasm-objdump`, `wasm-tools dump` |

The implementation lives in three places:

- `crates/sdust-debuginfo/` — backend-neutral builders (`DwarfBuilder`,
  `SourceMap`, `NameSection`).
- `crates/sdust-codegen-cranelift/src/debug.rs` — converts SIR + source
  bytes into DWARF DIEs and attaches the encoded sections to the
  cranelift `ObjectProduct` before linking.
- `crates/sdust-codegen-wasm/src/sourcemap.rs` — produces the `name`
  custom section and source-map sidecar, then appends both as wasm
  custom sections (the Component Model wrapper preserves them).

The `--debug` flag actually gates emission. `--release` produces the
same artifact, stripped of debug info.

## DWARF on native objects

### What we emit

For every program we emit one compilation unit:

- `DW_TAG_compile_unit`
  - `DW_AT_producer = "stardust-0.2"`
  - `DW_AT_language = DW_LANG_Rust` (closest semantic match in DWARF v4)
  - `DW_AT_name`, `DW_AT_comp_dir` from the build inputs
  - `DW_AT_low_pc = 0`, `DW_AT_high_pc = total_code_size`
- For each fn:
  - `DW_TAG_subprogram`
    - `DW_AT_name = <fn name>`
    - `DW_AT_external = true`
    - `DW_AT_low_pc`, `DW_AT_high_pc` (as offset)
    - `DW_AT_decl_file`, `DW_AT_decl_line`, `DW_AT_decl_column`
    - `DW_AT_type = <ref to base_type>`
  - For each user local + temp + param:
    - `DW_TAG_variable`
      - `DW_AT_name = <local name>`
      - `DW_AT_type = <ref to base_type>`
      - `DW_AT_data_member_location` if a frame offset is known
- For every type seen, one synthetic `DW_TAG_base_type` with
  `DW_AT_name`, `DW_AT_encoding`, `DW_AT_byte_size`.
- A line program with one row per source-spanned function entry.

### Section layout in the object

Per platform:

- **ELF / COFF**: sections are named `.debug_info`, `.debug_abbrev`,
  `.debug_line`, `.debug_str`, `.debug_ranges` (when used). Kind
  `SectionKind::Debug`.
- **Mach-O**: sections live in the `__DWARF` segment with names
  `__debug_info`, `__debug_abbrev`, etc. The translation is handled by
  `attach_dwarf_sections` in `codegen-cranelift/src/object.rs`.

### Inspecting

```bash
sdust build --debug examples/01_hello.sd
objdump --dwarf=info target/01_hello.o
# or
llvm-dwarfdump target/01_hello.o
# or fire up lldb on the linked binary
lldb target/01_hello
(lldb) target list
(lldb) image lookup -n main
```

### Known limitations (v0.2)

- **No relocations on `DW_AT_low_pc` / `DW_AT_high_pc`.** We use
  `Address::Constant` rather than `Address::Symbol`, so DWARF
  inspection works but live source-line stepping against the linked
  binary's runtime addresses does not. Fix-up plan: thread cranelift's
  per-fn symbol IDs through to `DwarfBuilder` and emit
  `Address::Symbol { symbol, addend }`, then teach
  `attach_dwarf_sections` to populate the corresponding object-write
  relocations.
- **Coarse line table.** We emit one line-program row at the fn entry,
  not one per machine instruction. Cranelift exposes per-instr
  `MachSrcLoc` events via `compiled_code()`, but the lowerer doesn't
  yet propagate SIR-statement spans far enough to make those rows
  meaningful. v0.3.
- **No `.debug_loc` location lists.** Per-local variables carry a
  `DW_AT_data_member_location` constant (the frame offset, when known),
  but not a proper location expression. v0.3.
- **No inlining info.** SIR doesn't track inlining either; that's a
  multi-slice undertaking.

## Wasm source maps + `name` section

### What we emit

When `sdust build --debug --target wasm32-*` runs:

1. The core module is emitted by `compile_program_to_bytes` (slice 8
   pipeline, untouched by debug info).
2. `build_name_section(prog, import_count)` constructs a
   subsection-id-1 `name` payload listing every user fn at its wasm
   function-index slot (import-relative).
3. `build_source_map(prog, source_path, source_text, output_wasm)`
   constructs a v3 source-map with one mapping per fn entry. The full
   source text is embedded in `sourcesContent[0]` so debuggers can
   render lines even without fetching the `.sd`.
4. `write_sourcemap_sidecar(out, sm)` writes the JSON to
   `<out>.wasm.map`.
5. `append_debug_sections(wasm, name_section, sidecar_url)` appends two
   custom sections to the wasm: the `name` section and a
   `sourceMappingURL` section pointing at the sidecar filename.

The Component-Model wrapper preserves both custom sections — they ride
through `wit-component::ComponentEncoder` untouched.

### Serving the sidecar

The `.wasm.map` is a regular UTF-8 JSON file. Browser DevTools fetch it
via the URL in the `sourceMappingURL` section, resolved relative to the
wasm's own fetch URL. Some hosts need an explicit MIME type:

```nginx
location ~* \.wasm\.map$ {
    add_header Content-Type "application/json; charset=utf-8";
}
```

For local dev, simple-server tooling usually serves `.map` files as
`application/octet-stream` which Chrome will still parse. If DevTools
complains "could not load source map," check the network tab and add
the header.

### Known limitations (v0.2)

- **Coarse mapping.** One source position per fn — not per instruction.
  Once SIR statements carry `SourceSpan`, the wasm emitter can record
  per-instruction byte offsets and we'll regenerate the mappings
  string.
- **No local-name subsection.** Subsection id 2 (locals) is empty.
  Local names show up as `var<N>` in DevTools.
- **No DWARF in wasm.** The Wasm community has a parallel
  [DWARF-in-wasm](https://yurydelendik.github.io/webassembly-dwarf/)
  initiative; we may add it later, but the source-map sidecar covers
  the common DevTools use-case today.

## Round-tripping in tests

The debuginfo builders are exercised by:

- `crates/sdust-debuginfo/tests/dwarf_roundtrip.rs` — builds DWARF,
  parses it back with `gimli::read`, asserts `DW_TAG_subprogram` for
  `main` is present.
- `crates/sdust-codegen-cranelift/tests/debug.rs` — builds a real
  object file, re-parses with the `object` crate, walks DWARF with
  `gimli`, confirms `DW_TAG_subprogram name=main` appears.
- `crates/sdust-codegen-wasm/tests/sourcemap.rs` — emits a wasm with
  the `name` + `sourceMappingURL` custom sections and writes the
  sidecar; parses with `wasmparser` and `serde_json` to confirm both
  are structurally valid.
