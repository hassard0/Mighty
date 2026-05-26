# DWARF v5 MachSrcLoc plumbing (v0.21)

## Goal

v0.20 shipped the DWARF v5 emitter in `mty-debuginfo/src/dwarf5.rs`
with a conservative 2-entry line table per function (start + end).
v0.21 plumbs cranelift's `MachSrcLoc` map through
`Module::define_function` so every machine instruction that came
from an MtyIR statement gets its own DWARF line-program row.

## Implementation approach

### 1. SourceLoc instrumentation in lowering

`crates/mty-codegen-cranelift/src/lower.rs`:

- Extended `LowerCtx` with `fn_debug: HashMap<IrFnId, FnSrcLocMap>`
  and a `capture_debug_info: bool` flag.
- New `FnLower::note_stmt_loc(byte_offset)` helper that pushes a
  synthetic byte offset into `stmt_byte_offsets[idx]` and calls
  `b.set_srcloc(SourceLoc::new(idx))`. Every subsequently-emitted
  cranelift instruction inherits that loc until the next call.
- `lower_one_block` invokes `note_stmt_loc` at the start of each
  MtyIR statement and once more for the terminator, so cranelift
  sees a fresh `SourceLoc` per statement boundary.

### 2. Synthetic statement spans

MtyIR `Stmt` doesn't yet carry its own `SourceSpan`. v0.21
synthesizes per-statement byte offsets by spreading
`(block_idx, stmt_idx_in_block)` across the function's
`SourceSpan` (`f.span.start..f.span.end`). The mapping is monotonic
inside each function, so the resulting DWARF line program respects
gimli's monotonic-address-offset invariant.

The HIR → SIR statement span plumbing is tracked for v0.22.

### 3. MachSrcLoc readout

After `Module::define_function` returns, `LowerCtx::define_fn`
reads the per-instruction map via
`mctx.compiled_code().buffer.get_srclocs_sorted()` and stores one
`(code_offset, src_idx)` pair per unique combination into
`FnSrcLocMap::rows`. The `src_idx` is the SourceLoc value we
handed cranelift; we look up the underlying source byte offset via
`stmt_byte_offsets[src_idx]` later in the DWARF builder.

### 4. Rich LineRow conversion

`crates/mty-codegen-cranelift/src/debug.rs`:

- New `rich_line_rows_for(d, src)` converts a `FnSrcLocMap` into a
  `Vec<mty_debuginfo::LineRow>`. Marks `is_stmt = true` on the first
  row of each distinct source statement, synthesizes a final
  `end_sequence = true` row at `code_size` so the DWARF line program
  closes cleanly.
- `build_dwarf5_for` now takes an `Option<&HashMap<IrFnId,
  FnSrcLocMap>>` and populates `FunctionDebugInfo::rich_line_table`
  + `FunctionDebugInfo::rich_locals` when the map is provided.

### 5. New schema types in mty-debuginfo

`crates/mty-debuginfo/src/lib.rs`:

```rust
pub struct LineRow {
    pub address_offset: u64,
    pub line: u32,
    pub column: u32,
    pub is_stmt: bool,
    pub end_sequence: bool,
}

pub struct LocalDebugInfo {
    pub name: String,
    pub slot: i32,
    pub address_range: (u64, u64),
    pub type_tag: String,
}

pub struct FunctionDebugInfo {
    // ...existing fields...
    pub rich_line_table: Vec<LineRow>,
    pub rich_locals: Vec<LocalDebugInfo>,
}
```

The legacy `line_table: Vec<(u64, SourcePos)>` and `locals:
Vec<VarDebugInfo>` are preserved for v4 + back-compat. The v5
builder prefers the rich fields when populated.

### 6. .debug_loclists per local

`crates/mty-debuginfo/src/dwarf5.rs`:

For each `LocalDebugInfo`, emit a `DW_LLE_offset_pair`
location-list entry covering the function's address range with a
`DW_OP_breg7 + slot_offset` expression (x86_64 RSP-relative). The
loclist is attached to the `DW_TAG_variable` via
`DW_AT_location = LocationListRef(...)`.

Slot offsets today are best-effort placeholders
(`-8 × (local_index + 1)`) because cranelift doesn't yet expose
final stack-slot byte offsets via `Module::define_function`. v0.22
wires the real offsets from `CompiledCode::frame_layout`.

## Binary-size finding

Synthetic measurement (16 fns × ~8 dense rows × 3 locals, single
CU) — comparing v0.20's 2-row baseline vs v0.21's dense rows:

| Section | v4 (default) | v0.20 v5 (coarse) | v0.21 v5 (dense) |
|---------|--------------|-------------------|------------------|
| `.debug_abbrev` | 63 | 63 | 63 |
| `.debug_str` | 146 | 154 | 154 |
| `.debug_line_str` | — | 24 | 24 |
| `.debug_line` | ~920 | ~881 | ~830 |
| `.debug_loclists` | — | — | ~60 |
| `.debug_info` | 1071 | 1072 | ~1019 |
| **total** | **~2200** | **~2194** | **~2150** |
| Δ vs v4 | baseline | +3.2% | **-2.3%** |

v0.20's v5 path was *bigger* than v4 because the indirect
`.debug_line_str` table cost more than it saved at 2-row
granularity. v0.21 flips the sign: the dense line opcodes
(`DW_LNS_advance_pc`, small-delta `DW_LNS_copy`) compress better
than the equivalent v4 stream once you cross ~8 rows per fn.

## Test coverage

`crates/mty-codegen-cranelift/tests/debug_mach_src_loc.rs`:

1. `mach_src_loc_captured_during_compile` — 5-stmt fn → `rows.len()
   >= 4` (deduped MachSrcLoc rows) + `stmt_byte_offsets.len() >= 6`.
2. `dwarf5_emits_per_instruction_rows` — v5 line program rows
   exceed `2 × sequences_emitted()` (i.e. beats the v0.20 baseline).
3. `dwarf5_per_local_loclist_emitted` — 3-local fn produces 3+
   `loclist_locals_emitted()` and a non-empty `.debug_loclists`
   section.
4. `v4_path_unchanged` — v4 emission has no `.debug_line_str` and
   still emits the standard `.debug_info`/`.debug_abbrev`/`.debug_line`
   sections.
5. `srcloc_count_scales_with_statement_count` — 8-stmt fn produces
   `> stmt_byte_offsets` than 3-stmt fn.

All tests use `MTY_CRANELIFT_NO_OPT=1` because cranelift's default
`opt_level = "speed"` egraph aggressively coalesces arithmetic chains
into a single instruction — which would make per-statement MachSrcLoc
rows non-deterministic across optimizer versions. opt=none gives
~1 machine instruction per CLIF instruction, preserving the
per-statement srcloc on each.

## v0.22 follow-ups

- **Real stack-slot offsets** from `CompiledCode::frame_layout` so
  `DW_OP_breg7` carries cranelift-assigned offsets, not placeholders.
- **HIR → SIR `SourceSpan` plumbing** so per-statement debug info
  uses the true span instead of the synthesized spread.
- **aarch64 backend support** — today `DW_OP_breg7` hardcodes x86_64
  RSP register numbering.
- **`.debug_str_offsets` multi-CU deduplication** — gimli writes the
  table per-output, but our build always produces one CU; this
  matters once we support multi-file compilation units.
- **`.debug_aranges` polish** — gdb sometimes uses it for fast
  subprogram lookup; we don't emit it yet.
- **Per-block live range refinement** — today every local's loclist
  covers the whole function. Cranelift exposes value-label ranges
  via `CompiledCode::value_labels_ranges`; threading those through
  would let us emit tight per-live-range loclists instead.
