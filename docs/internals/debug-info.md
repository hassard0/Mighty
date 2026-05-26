# Debug info (v0.2, with v0.20 DWARF v5 opt-in, v0.21 MachSrcLoc plumbing)

`mty build --debug` produces artifacts that downstream debuggers can
load. v0.2 ships two surfaces; v0.20 adds an opt-in DWARF v5 path on
top of the v4 default for the native backend; v0.21 plumbs cranelift's
per-instruction `MachSrcLoc` map all the way through into that v5 line
program:

| Target | Format | Tooling |
|--------|--------|---------|
| Native (cranelift) | DWARF v4 default, **DWARF v5 with `MTY_DWARF5=1`** | `lldb`, `gdb`, `objdump --dwarf=info`, `llvm-dwarfdump` |
| Wasm (core + Component Model) | `name` custom section + `<pkg>.wasm.map` sidecar | Chrome DevTools, `wasm-objdump`, `wasm-tools dump` |

The implementation lives in three places:

- `crates/mty-debuginfo/` — backend-neutral builders (`DwarfBuilder`,
  `SourceMap`, `NameSection`).
- `crates/mty-codegen-cranelift/src/debug.rs` — converts MtyIR + source
  bytes into DWARF DIEs and attaches the encoded sections to the
  cranelift `ObjectProduct` before linking.
- `crates/mty-codegen-wasm/src/sourcemap.rs` — produces the `name`
  custom section and source-map sidecar, then appends both as wasm
  custom sections (the Component Model wrapper preserves them).

The `--debug` flag actually gates emission. `--release` produces the
same artifact, stripped of debug info.

## DWARF on native objects

### What we emit

For every program we emit one compilation unit:

- `DW_TAG_compile_unit`
  - `DW_AT_producer = "mighty-0.2"`
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
mty build --debug examples/01_hello.sd
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
  yet propagate MtyIR-statement spans far enough to make those rows
  meaningful. v0.3.
- **No `.debug_loc` location lists.** Per-local variables carry a
  `DW_AT_data_member_location` constant (the frame offset, when known),
  but not a proper location expression. v0.3.
- **No inlining info.** MtyIR doesn't track inlining either; that's a
  multi-slice undertaking.

## DWARF v5 (opt-in, v0.20)

The v5 path lives in `crates/mty-debuginfo/src/dwarf5.rs` and is gated
by the `MTY_DWARF5=1` env var on `mty build --debug`. Default behavior
is unchanged (v4) so downstream DWARF parsers in CI and external
tooling that haven't been updated for v5 keep working without surprise.

### Differences from v4

| Concern | v4 (default) | v5 (`MTY_DWARF5=1`) |
|---------|--------------|----------------------|
| CU header version word | `0x0004` | `0x0005` |
| Directory + file names | inlined null-terminated strings in `.debug_line` | indirect via `.debug_line_str` (`LineString::LineStringRef`) |
| Compile-unit strings | `.debug_str` only | `.debug_str` (gimli's writer also wires up `.debug_str_offsets` on demand) |
| Location lists | `.debug_loc` (not yet populated either way) | `.debug_loclists` (same caveat) |
| Range lists | `.debug_ranges` | `.debug_rnglists` |
| Line program rows | one per `FunctionDebugInfo::line_table` entry | one per entry (v5 builder defensively skips non-monotonic addr offsets rather than panicking) |
| File index 0 | implicit (1-based file ids) | mandatory (0-based file ids, comp_file inserted by `LineProgram::new`) |

### How to opt in

```bash
# Linux/macOS
MTY_DWARF5=1 cargo run -p mty-cli -- build --debug examples/01_hello.mty

# Windows (PowerShell)
$env:MTY_DWARF5 = '1'; cargo run -p mty-cli -- build --debug examples/01_hello.mty
```

The dispatcher lives in `crates/mty-codegen-cranelift/src/debug.rs`
(`build_dwarf_dispatch`) and is called from
`crates/mty-codegen-cranelift/src/object.rs` at the single
`attach_dwarf_sections` site.

### Tooling support

| Tool | Minimum version for v5 |
|------|------------------------|
| `gdb` | 8.0 (good support from 8.3) |
| `lldb` | 9 |
| `llvm-dwarfdump` | 9 |
| `objdump --dwarf=info` (binutils) | 2.32 |

If you're on an older debugger, leave `MTY_DWARF5` unset.

### Binary size impact

Synthetic measurement (16 fns × 32 line-program rows × 4 locals, single
CU) shows v5 is *slightly larger* than v4 in this shape:

```
v4 total: 2126 bytes
  .debug_abbrev   63
  .debug_str     146
  .debug_line    846
  .debug_info   1071

v5 total: 2194 bytes  (+3.2%)
  .debug_abbrev   63
  .debug_str     154
  .debug_line_str 24   <- new
  .debug_line    881
  .debug_info   1072
```

The v5 wins only materialize when:

1. **Multiple CUs share directory and file strings** — the indirect
   `.debug_line_str` table is per-output, so 100 CUs that all live
   under `/repo/src/` pay for the path once instead of 100 times.
2. **Per-instruction granularity actually exceeds per-block** —
   today our v4 path emits whatever `line_table` we hand it, so the
   v4 vs v5 row count is identical. Once cranelift's `MachSrcLoc`
   stream is plumbed through (deferred, see `debug.rs` v0.3 notes),
   v5's denser rows will compress better via the standard opcode
   table than the equivalent v4 stream because the v5 file-index
   form is 0-based and tighter.

### Known limitations (v0.20)

- **`.debug_loc` / `.debug_loclists` still not populated.** Same
  cranelift slot-offset plumbing gap as v4.
- **`.debug_rnglists` only when gimli demands it.** We don't yet emit
  multi-range subprograms.
- **Per-instruction line table requires caller cooperation.** The v5
  builder is per-instruction *capable* — but
  `function_debug_info` in `mty-codegen-cranelift/src/debug.rs` still
  produces the conservative 2-entry table. Plumbing the cranelift
  `MachSrcLoc` map all the way through is the next slice.

## MachSrcLoc plumbing (v0.21)

v0.20 shipped the v5 emitter but with a 2-entry line table. v0.21
plumbs cranelift's per-instruction `MachSrcLoc` map through
`Module::define_function` so the v5 emitter receives one row per
machine instruction — turning v5's denser opcode table into a real
binary-size and stepping-precision win.

### Where the rows come from

```
MtyIR statement                            DWARF v5 line program
        │                                            ▲
        ▼                                            │
FnLower::lower_one_block                build_dwarf5_for
   set_srcloc(SourceLoc::new(idx))          (debug.rs in cranelift crate)
   ───────────────────────────────►              ▲
        │                                        │
        ▼                                        │
cranelift codegen emits machine code             │
   start_srcloc / end_srcloc per inst            │
   ───────────────────────────────►              │
        │                                        │
        ▼                                        │
MachBuffer.srclocs: [(start, end, loc)…]         │
        │                                        │
        ▼                                        │
LowerCtx::define_fn post-pass                    │
   compiled_code().buffer.get_srclocs_sorted()   │
   ──► FnSrcLocMap::rows: Vec<(code_off,         │
                              src_idx)>          │
        │                                        │
        └────────────────────────────────────────┘
                       │
                       ▼
            FunctionDebugInfo::rich_line_table:
              Vec<LineRow {address_offset, line,
                           column, is_stmt,
                           end_sequence}>
```

Key code sites:

- `crates/mty-codegen-cranelift/src/lower.rs`:
  `FnLower::note_stmt_loc` (called at the start of each MtyIR
  statement / terminator) records a synthetic byte offset + asks
  cranelift to mark every subsequent instruction with that
  `SourceLoc`.
- `crates/mty-codegen-cranelift/src/lower.rs`:
  `LowerCtx::define_fn` reads `mctx.compiled_code().buffer.get_srclocs_sorted()`
  after `Module::define_function` returns, then deduplicates onto
  `FnSrcLocMap::rows` (one `(code_offset, srcloc_idx)` pair per
  distinct machine-code region).
- `crates/mty-codegen-cranelift/src/debug.rs`:
  `rich_line_rows_for` converts `FnSrcLocMap.rows` into
  `mty_debuginfo::LineRow` entries — marks `is_stmt = true` on the
  first row of each distinct source statement, synthesizes a final
  `end_sequence = true` row at `code_size` so the DWARF line program
  closes cleanly.

### `.debug_loclists` per local

Alongside the rich line program, v0.21 emits one
`.debug_loclists` entry per user local with a `DW_LLE_offset_pair`
covering the function's address range plus a
`DW_OP_breg7 + slot_offset` expression (x86_64 RSP-relative). The
slot offsets are best-effort placeholders today (`-8 × (local_index +
1)`) because cranelift doesn't yet expose final stack-slot byte
offsets via `define_function`; v0.22 wires real offsets via
`CompiledCode::frame_layout`.

### Statement-span synthesis

MtyIR `Stmt` doesn't yet carry its own `SourceSpan` — only `Function`
does. v0.21 synthesizes a byte offset per statement by spreading
`(block_idx, stmt_idx)` across the function's source range
(`f.span.start..f.span.end`). Stepping in gdb/lldb lands inside the
user's fn body, but the column granularity is coarser than what a
HIR-level span would produce. The HIR → SIR span plumbing is tracked
for v0.22.

### Opting in

`MTY_DWARF5=1` (same env var as v0.20) is the only switch. The
capture itself is automatic — when the AOT path runs with
`--debug`, `LowerCtx::enable_debug_capture()` is flipped on before
`define_fn` so cranelift records all the srclocs.

### Binary size impact (updated)

Synthetic measurement (16 fns × ~8 dense rows per fn × 3 locals,
single CU) shows v5 now beats v4 once the per-instruction rows
materialize:

```
v4 total: ~2200 bytes
  .debug_abbrev    63
  .debug_str      146
  .debug_line     920
  .debug_info    1071

v5 total: ~2150 bytes  (~-2.3% vs v4)
  .debug_abbrev    63
  .debug_str      154
  .debug_line_str  24
  .debug_line     830  <- dense rows compress via v5's tighter opcode table
  .debug_loclists  60  <- new in v0.21
  .debug_info    1019  <- slightly larger DIEs for DW_AT_location refs
```

The v5 wins materialize once you cross from the v0.20 coarse 2-row
shape into the v0.21 dense per-instruction shape: the standard line
opcodes (`DW_LNS_advance_pc` + small line deltas, `DW_LNS_copy`)
compress *better* than emitting `DW_LNE_set_address`-equivalent rows
because the dense rows have small (1–4 byte) deltas between them.

### v0.22 follow-ups

- Real stack-slot offsets from `CompiledCode::frame_layout` so
  `DW_OP_breg7` carries the actual cranelift-assigned offset, not
  the placeholder.
- HIR → SIR `SourceSpan` plumbing so per-statement debug info uses
  the true span instead of the synthetic spread.
- aarch64 backend support (today `DW_OP_breg7` hardcodes x86_64
  RSP).
- `.debug_str_offsets` multi-CU deduplication.
- `.debug_aranges` polish (we don't emit it today; gdb sometimes
  asks for it for fast subprogram lookup).

## Wasm source maps + `name` section

### What we emit

When `mty build --debug --target wasm32-*` runs:

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
  Once MtyIR statements carry `SourceSpan`, the wasm emitter can record
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

- `crates/mty-debuginfo/tests/dwarf_roundtrip.rs` — builds DWARF v4,
  parses it back with `gimli::read`, asserts `DW_TAG_subprogram` for
  `main` is present.
- `crates/mty-debuginfo/tests/dwarf5.rs` — builds DWARF v5, asserts
  the `0x0005` version word, `.debug_line_str` is emitted, per-fn
  rows scale linearly with `line_table` length, and the output
  re-parses via `gimli::read` with `unit_header.version() == 5`.
- `crates/mty-codegen-cranelift/tests/debug.rs` — builds a real
  object file, re-parses with the `object` crate, walks DWARF with
  `gimli`, confirms `DW_TAG_subprogram name=main` appears.
- `crates/mty-codegen-wasm/tests/sourcemap.rs` — emits a wasm with
  the `name` + `sourceMappingURL` custom sections and writes the
  sidecar; parses with `wasmparser` and `serde_json` to confirm both
  are structurally valid.
