# SELFHOST_CODEGEN_V0_14_NOTES

This file catalogues the v0.14 self-host codegen slice: extending the
Mighty-source Wasm core-module emitter to cover string-pool interning,
ADT linear-memory layout, and pattern lowering. After this slice the
bootstrap covers examples 01-03 + a synthetic pattern-match fixture +
a string-pool fixture, with thirteen live tests against zero ignored.

## Live status

- `selfhost/codegen/lib.mty` — 73 LOC, `mty check` clean.
- `selfhost/codegen/wasm.mty` — ~660 LOC (was ~400 in v0.13), `mty
  check` clean, services the v0.14 bootstrap test.
- `selfhost/codegen/string_pool.mty` — 80 LOC, new in v0.14; documents
  the string-pool helper API + `mty check`s clean.
- `selfhost/codegen/adt_layout.mty` — 145 LOC, new in v0.14; documents
  the ADT layout + bump-allocator helpers + `mty check`s clean.
- `selfhost/codegen/pattern.mty` — 110 LOC, new in v0.14; documents
  the match-arm cascade lowering + `mty check`s clean.
- `crates/mty-driver/tests/selfhost_codegen.rs` — ~2000 LOC, **13/13
  live tests pass; 0 ignored**.

```
test selfhost_codegen_compiles ........................ ok
test selfhost_codegen_lib_compiles .................... ok
test selfhost_codegen_string_pool_compiles ............ ok
test selfhost_codegen_adt_layout_compiles ............. ok
test selfhost_codegen_pattern_compiles ................ ok
test selfhost_codegen_hello_world ..................... ok
test selfhost_codegen_example_01 ...................... ok
test selfhost_codegen_example_02 ...................... ok
test selfhost_codegen_example_03 ...................... ok
test selfhost_codegen_example_03_option ............... ok
test selfhost_codegen_arith_fixture ................... ok
test selfhost_codegen_pattern_match_full .............. ok
test selfhost_codegen_string_const .................... ok
```

## What changed vs v0.13

| Surface | v0.13 | v0.14 |
|---------|-------|-------|
| `Const::Str` | dest local set to 0 (placeholder) | dest local set to interned offset; bytes flushed into a data segment exported as `__strings` |
| `Rvalue::AdtInit` | `unreachable` | real bump-alloc + tag store at offset 0 + per-field `i32.store` at `tag_size + field_idx * 4` |
| `Term::SwitchVariant` | `unreachable` | nested `block`/`br_if` cascade — one block per arm + outer `match_done`; tag-test cascade in the innermost block; arm body lowered after each block-close; `br <depth>` back to `match_done` |
| `Use(Place { proj: VariantField })` | bare `local.get` (incorrect — leaves ADT base on stack instead of the payload value) | `local.get base` + `i32.load offset=<4 + field_idx*4>` |
| memory section | 1 page, no globals | 1 page + 1 mutable `i32` global `$heap_ptr` initialized to 1024 |
| data section | not emitted | optional active segment at memory 0, offset 0; concatenates every interned string's UTF-8 bytes |

## Architectural choice — interpretation calls

### 1. Inline vs cross-file

The new helper files (`string_pool.mty`, `adt_layout.mty`,
`pattern.mty`) are intended to be cross-file imports under the
post-v1.0 multi-file driver, but the v0.12 driver still compiles one
`.mty` file at a time (see `SELFHOST_HIR_V0_8_NOTES.md` gap #1). The
**runnable** code therefore lives inlined in `wasm.mty`; the
standalone helper files document the intended layout and are
exercised by per-file `mty check` smoke tests. This is the same
pattern v0.13 established with `lib.mty`.

### 2. Bump allocator never frees

The `$heap_ptr` global only grows. v0.15 will integrate this with
the v0.10 arena/drop layer so per-fn ADT allocations are reclaimed
on scope exit. v0.14 leans on the test gate being **wasmparser
validation**, not steady-state memory usage — the validator doesn't
actually run the module.

### 3. Every payload field is 4 bytes

`adt_field_size()` returns 4 unconditionally. This covers i32, Bool,
Char, pointers, and the offset-half of a Mighty string fat-pointer.
The v0.15 follow-up is to plumb per-field `IrTy` widths through the
ADT bridge so i64/f64 fields land at the right offsets.

### 4. Variant-call lowering NOT modelled

The Rust IR pipeline lowers BARE variant references (`None`,
`Maybe.Other`) to `Rvalue::AdtInit`, but lowers variant-CALL syntax
(`Some(x)`, `Maybe.Just(n)`) to `Rvalue::Call { func: Extern(...) }`.
The v0.14 emitter therefore can only exercise the bare-variant path
for ADTs with payloads. The `selfhost_codegen_example_03_option`
fixture uses two payload-less variants on purpose; the
`selfhost_codegen_pattern_match_full` fixture's `Op` enum
construction happens inside MATCH ARMS, not as Call rvalues, so it
exercises the SwitchVariant + VariantField path cleanly.

The v0.15 follow-up is to teach `lower_call` (or `resolve_callee`)
to detect `DefRef::Variant` and emit `AdtInit` instead of a
Builtin-Extern call. That's a Rust-pipeline change (in
`crates/mty-ir/src/lower/exprs.rs`), not a self-host change.

### 5. SwitchInt still emits `unreachable`

The v0.14 pattern lowering focuses on `SwitchVariant` (the ADT-match
shape). Int-discriminant matches (`match n { 0 => ..., 1 => ..., _
=> ... }`) still bottom out in `unreachable`. The shape is
structurally similar to SwitchVariant (br_table or br_if cascade)
but exercises a different terminator + different snapshot accessors;
deferred to v0.15.

### 6. For-loop iter desugar NOT implemented

Stretch goal listed in the brief; deferred to v0.15. The HIR-level
desugar (rewrite `for x in 0..n { ... }` as `let i = 0; while i < n {
let x = i; ...; i = i + 1 }`) lives above the emitter and would
require a separate selfhost slice in `selfhost/hir/`.

## Bridge surface added in v0.14

### Read side (IR queries)

```
ir_block_stmt_rvalue_const_str(fid, bid, j)        -> Str
ir_block_stmt_rvalue_adt_id(fid, bid, j)           -> USize
ir_block_stmt_rvalue_adt_variant(fid, bid, j)      -> USize
ir_block_stmt_rvalue_use_proj_kind(fid, bid, j)    -> Str
ir_block_stmt_rvalue_use_proj_variant(fid, bid, j) -> USize
ir_block_stmt_rvalue_use_proj_field(fid, bid, j)   -> USize
ir_adt_variant_count(adt_id)                       -> USize
ir_adt_variant_field_count(adt_id, v)              -> USize
ir_block_term_switch_discr_local(fid, bid)         -> USize
ir_block_term_switch_adt_id(fid, bid)              -> USize
ir_block_term_switch_arm_count(fid, bid)           -> USize
ir_block_term_switch_arm_variant(fid, bid, k)      -> USize
ir_block_term_switch_arm_target(fid, bid, k)       -> USize
ir_block_term_switch_default(fid, bid)             -> USize
```

### Write side (Wasm sink)

```
wasm_emit_intern_string(s: Str)        -> USize
wasm_emit_string_offset(slot: USize)   -> USize
wasm_emit_string_length(slot: USize)   -> USize
wasm_emit_data_segment_flush()         -> Unit
wasm_emit_heap_global_idx()            -> USize
wasm_emit_global_get(idx: USize)       -> Unit
wasm_emit_global_set(idx: USize)       -> Unit
wasm_emit_i32_load(offset: USize)      -> Unit
wasm_emit_i32_store(offset: USize)     -> Unit
```

## Test fixtures added

### selfhost_codegen_example_03_option

Tests bare-variant lowering for an enum with two payload-less
variants used as if-branch results. Verifies:

- Two `i32.store` opcodes (one per AdtInit tag store)
- At least one `global.get` (the bump allocator's pointer read)

### selfhost_codegen_pattern_match_full

Tests `SwitchVariant` lowering for a 3-arm enum with payload binding.
Verifies:

- At least one `i32.load` opcode (tag read inside the cascade)
- At least one `br_if` opcode (arm test branch)

### selfhost_codegen_string_const

Tests string-pool interning. The fixture defines two literal strings
(one bound to a let, one passed to `log`). Verifies:

- At least one `i32.const` opcode (the interned offset)

### selfhost_codegen_example_03

Un-ignored from v0.13. Verifies:

- The `first` fn appears in the rebuilt module's fn-name list.
- At least one `i32.store` opcode appears in some fn body (the `None`
  arm's AdtInit lowering).

## Coverage matrix

| MtyIR shape | v0.13 | v0.14 |
|-------------|-------|-------|
| i32/i64 arithmetic | yes | yes |
| if/else | yes | yes |
| while + loop | yes | yes |
| log/print/panic | yes | yes |
| Const::Bool/Int/Float/Char | yes | yes |
| Const::Str | placeholder | real intern |
| Rvalue::Use (plain) | yes | yes |
| Rvalue::Use (VariantField proj) | broken | yes |
| Rvalue::AdtInit | unreachable | yes |
| Term::Return | yes | yes |
| Term::If | yes | yes |
| Term::SwitchVariant | unreachable | yes |
| Term::SwitchInt | unreachable | unreachable |
| Tuple/Array init | unreachable | unreachable |
| MethodCall | unreachable | unreachable |
| FieldRead (struct) | unreachable | unreachable |
| For-loop iter | unreachable | unreachable |
| Agent/Send/Ask/Spawn | unreachable | unreachable |
| Drop / arena | nop | nop |

## v0.15 follow-ups

1. **Variant-call lowering**: teach `resolve_callee` (or
   `lower_call`) in `crates/mty-ir/src/lower/exprs.rs` to detect
   `DefRef::Variant` and emit `AdtInit` instead of a Builtin-Extern
   call. Required for `Some(x)`, `Result.Ok(v)`, `Maybe.Just(n)` to
   flow through the v0.14 AdtInit path.
2. **SwitchInt multi-arm lowering**: mirror the SwitchVariant cascade
   but with the i32 discriminant value compared against each arm's
   `i128` payload (truncated to i32).
3. **For-loop iter desugar**: lift the `for x in iter { ... }` form
   into a HIR-level rewrite the way the Rust pipeline does.
4. **Real LEB128 + section-byte emission in Mighty source**: requires
   Vec[U8] mutators in the stdlib (currently the byte-twiddling is on
   the host side). Per the brief this is post-v0.14.
5. **Allocator arena drop**: integrate `$heap_ptr` with the v0.10
   arena/drop layer so per-fn ADT allocations are reclaimed at
   StorageDead. Today the allocator only grows.
6. **Per-field width in ADT layout**: every field is 4 bytes today.
   Plumb per-field `IrTy` widths through the ADT bridge so i64/f64
   fields land at the right offsets.
7. **Multi-data-segment + dedup**: today we emit ONE active data
   segment at offset 0 containing every interned string in their
   assigned offset order. v0.15 might want a passive segment with
   bulk-memory ops, or a per-string segment for incremental linking.
8. **Canonical-ABI string layout**: for the Component Model target,
   the string fat-pointer (`(ptr, len)`) interacts with WIT's
   `string` lowering. v0.14 keeps strings as `i32`-offsets only;
   `(ptr, len)` pairs are synthesised on demand at the log-call site.

## Brief budget vs actual

- **Budget**: 6 hours
- **Actual**: ~3 hours
- **Scope shipped**: phases 1 (string pool) + 2 (pattern lowering) +
  3 (ADT layout) + 4 (drive example 03) + 6 (docs). Phase 5 (for-loop
  iter, optional stretch) deferred to v0.15.

## Status

**SHIPPED-FULL** for the v0.14 brief's required scope (phases 1-4 +
6). Stretch phase 5 is documented as v0.15 follow-up.
