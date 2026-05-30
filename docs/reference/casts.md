# `as` casts — spec reference

`expr as Ty` is Mighty's explicit-conversion surface. It is intentionally
narrow: only a small set of well-defined scalar / reference conversions
are recognised; anything else falls through to `MT2027 INVALID_CAST`.
This page enumerates the accepted shapes per release and the rationale
for the deliberate rejections.

## v0.37 T2 baseline

Pre-v0.37 the parser silently degraded `x as I64` into `BinOp::Add` on
a type-path RHS, so the cast never reached the type checker. v0.37 T2
introduced:

- A real `CAST_EXPR` CST node.
- `MT2027 INVALID_CAST` at the typeck emit-site.
- The starter scalar matrix:
  - `Int ↔ Int` (widen / narrow / sign change)
  - `Int ↔ Float` (truncate / round)
  - `Float ↔ Float` (widen / narrow)
  - `Bool → Int` (`false → 0`, `true → 1`)
  - `Char ↔ Int` (codepoint round-trip)

## v0.39 T2 polish

The v0.39 T2 track extends the matrix in three places.

### 1. Bool ↔ Int

Both directions are now accepted:

| Source | Target | Semantics |
|--------|--------|-----------|
| `Bool` | `IntN` | `false → 0`, `true → 1`; widening is unsigned (matches `Bool` being stored as `I8` at runtime). |
| `IntN` | `Bool` | `0 → false`, any nonzero → `true`. The cranelift back-end emits `icmp ne, 0` rather than truncating low bits; `256_i32 as Bool` correctly yields `true`, not `false`. |

`Float ↔ Bool` stays **rejected** (MT2027). NaN has no obvious truth
value (`NaN != NaN` would round-trip to `true` then `false`, which is
surprising), and `Bool → Float` adds nothing over the explicit
`if b { 1.0 } else { 0.0 }`. Authors who want either conversion must
spell the predicate explicitly:

```mty
// Float → Bool
let truthy: Bool = x != 0.0 && !x.is_nan()

// Bool → Float
let f: F64 = if b { 1.0 } else { 0.0 }
```

### 2. Reference casts (`&T as *T`)

`&T as *T` and `&mut T as *mut T` are now accepted as explicit casts.
Previously this conversion was only available implicitly at extern-c
call sites via the `coerce_addr_of` path (v0.37 T3 / v0.38 T3).

Rules:

- The source must be `&T` or `&mut T`. `Int as *T` and `*T as Int` are
  **rejected** — surface `as` does not admit pointer arithmetic. Use
  `unsafe { raw_ptr(addr) }` for the raw-address-to-pointer builtin.
- The inner type must unify. `&I32 as *I32` is fine; `&U8 as *I32` is
  rejected (MT2027 with an inner-type mismatch). Authors who really
  want to bit-cast a pointer's pointee type must use a sequence of
  builtins inside `unsafe`.
- At the syntax level `*T` and `&T` share the `TYPE_BORROW` CST node
  (a slice-1 simplification — see
  `crates/mty-syntax/src/parser/types.rs`). This means the typeck
  representation collapses `*const T` and `*mut T` onto a single
  `TyData::Ref { mutable: false, inner: T }` shape; v0.40 will revisit
  whether the const/mut distinction should be re-introduced at the
  source level.

Example:

```mty
let x: I32 = 42
let p: *I32 = &x as *I32   // explicit cast — same SIR shape as the
                           // FFI coercion path emits implicitly.
```

### 3. `Int as Char` codepoint validity

`Char` in Mighty is a Unicode scalar value (matching Rust's `char`): a
32-bit integer in `0..0x110000` excluding the UTF-16 surrogate gap
`0xD800..=0xDFFF`. Allowing an out-of-range value to flow through
would corrupt UTF-8 invariants once the char hit a `String`.

v0.39 T2 enforces this with a **compile-time check for integer
literals**:

```mty
let c1 = 0x41_u32 as Char       // ok — 'A'
let c2 = 0x110000_u32 as Char   // MT2028 INVALID_CODEPOINT
let c3 = 0xD800_u32 as Char     // MT2028 (UTF-16 surrogate)
let c4 = 0xD7FF_u32 as Char     // ok — last value before the gap
```

### v0.40 T3 — non-literal `Int as Char` rejected; use `Char.from_u32`

v0.39 T2 left non-literal sources as a documented pass-through that
produced the raw bit pattern at runtime. v0.40 T3 closes that surface:

**Non-literal `Int as Char` is now rejected at the cast emit-site with
`MT2027 INVALID_CAST`** plus a fix-suggestion that auto-rewrites the
expression to `Char.from_u32(value)?`. The shape was picked from the
v0.39 T2 design table (options (a) runtime trap vs (b) `Option[Char]`
surface):

- **(b) Option[Char] won.** Mighty's general direction is
  `Result` / `Option` for fallible operations, not panics. Non-literal
  codepoint validity isn't a memory-safety boundary (so a runtime trap
  in the v0.39 T3 `MT5081` sense isn't warranted) — it's an input-
  validation problem, which the call site should pattern-match on.
- The literal-time path is unchanged: `0x41 as Char` is still a valid
  compile-time cast (with MT2028 on out-of-range literals).
- The `as` surface for `Char` is now **one-directional**: `Char as IntN`
  remains accepted (codepoint round-trip is total), but `IntN as Char`
  requires the explicit constructor for non-literal sources.

Authors get a typed `Option[Char]` so the surrounding code is forced to
say what it wants for the invalid case:

```mty
// Surfaces the failure to the caller via `?`
fn parse_codepoint(s: Str) -> Option[Char] {
  let n: U32 = s.parse()?
  Char.from_u32(n)?
}

// Replacement-character fallback
fn safe_char(v: U32) -> Char {
  Char.from_u32(v).unwrap_or('?')
}

// Pattern match for a custom strategy
match Char.from_u32(v) {
  Some(c) => emit(c),
  None    => emit_diagnostic("bad codepoint"),
}
```

The fix engine (`mty fix --apply`) auto-converts an existing
`<expr> as Char` line to `Char.from_u32(<expr>)?`, so the v0.39 T2
shape migrates without manual edits. See
`crates/mty-diagnostics/src/codes_fix.rs::fix_invalid_cast_int_to_char`.

See the `Char.from_u32` hover entry (`mty doc --hover std.Char.from_u32`)
for the full prelude reference.

## v0.42 T2 — numeric casts actually convert

v0.37 T2 introduced the `Int ↔ Int`, `Int ↔ Float`, `Float ↔ Float`
typeck matrix but every back-end left the conversion as a no-op: a
`U8` operand of `expr as I32` flowed downstream as a U8, a `3.7 as
I32` stayed a `Float(3.7)` at the IR level, and `(u as F32) * 2.0_f32`
only typechecked because the multiplier's left side still had the
original `USize` SIR type. The IDE worked around it for v0.41 by
keeping every value in one type end-to-end and pushing all
int↔pixel layout to the FFI shim (lesson L19 in
`docs/mighty-language-lessons.md`).

v0.42 T2 wires the conversion into the lowering / codegen layers so
`as` does what it says on every back-end:

| src       | dst        | cranelift                         | LLVM                              | wasm                                | interp           |
|-----------|------------|-----------------------------------|-----------------------------------|-------------------------------------|------------------|
| int (s)   | wider int  | `sextend`                         | `sext`                            | `i64.extend_i32_s`                  | `narrow_int` (sign-ext) |
| int (u)   | wider int  | `uextend`                         | `zext`                            | `i64.extend_i32_u`                  | `narrow_int` (zero-ext) |
| int       | narrower int | `ireduce`                       | `trunc`                           | `i32.wrap_i64`                      | `narrow_int` (mask) |
| int (s)   | float      | `fcvt_from_sint`                  | `sitofp`                          | `f32/f64.convert_iN_s`              | `as f64`         |
| int (u)   | float      | `fcvt_from_uint`                  | `uitofp`                          | `f32/f64.convert_iN_u`              | `as f64` via unsigned mask |
| float     | int (s)    | `fcvt_to_sint_sat` (saturating)   | `llvm.fptosi.sat.iN.fM`           | `iN.trunc_sat_fM_s`                 | `float_to_int_saturating` |
| float     | int (u)    | `fcvt_to_uint_sat` (saturating)   | `llvm.fptoui.sat.iN.fM`           | `iN.trunc_sat_fM_u`                 | `float_to_int_saturating` |
| float     | wider fp   | `fpromote`                        | `fpext`                           | `f64.promote_f32`                   | `as f64`         |
| float     | narrower fp | `fdemote`                        | `fptrunc`                         | `f32.demote_f64`                    | `(_ as f32) as f64` |

### Float → Int overflow policy: saturating, not trapping

Every back-end uses the saturating conversion (cranelift's `*_sat`,
wasm's `trunc_sat_*`, LLVM's `fpto*.sat` intrinsic), and the interp
mirrors Rust's `as` semantics:

- `NaN`        → `0`
- `+inf` / `> dst::MAX` → `dst::MAX`
- `-inf` / `< dst::MIN` → `dst::MIN`  (or `0` for unsigned dst)
- in-range → truncate toward zero

Rationale for **saturating** over **trapping**:

1. **Mighty already has `Result`/`Option` for fallible conversions.**
   If the author wants the bad case escalated to a typed error they
   can write `Char.from_u32` (v0.40 T3), or a future
   `F32.try_to_i32() -> Option[I32]`, instead of weaving a panic into
   a one-line numeric expression.
2. **The Rust-`as` rule is well-known.** Mighty's numeric `as` has
   the same shape as Rust's since v0.37 T2; copying its overflow
   policy keeps the surface frictionless for Rust-fluent authors.
3. **Hardware support is universal.** WebAssembly 2.0 mandates the
   `trunc_sat_*` ops, x86-64 has `vcvttss2si`-with-saturation since
   AVX-512 (and cranelift / LLVM emit a software fallback below
   that), and the cranelift `*_sat` instructions are zero-cost on
   Aarch64 (`fcvtzs`/`fcvtzu`).
4. **No new diagnostic code needed.** A trapping policy would
   reserve `MT5082 FLOAT_TO_INT_OUT_OF_RANGE`; saturating keeps the
   slot free for a real safety boundary later.

Authors who want a trap can spell it explicitly:

```mty
fn f64_to_i32_or_panic(x: F64) -> I32 {
  if x.is_nan() || x < (I32.MIN as F64) || x > (I32.MAX as F64) {
    panic!("float-to-int overflow: {}", x)
  }
  x as I32
}
```

### What this unblocks

- The IDE no longer has to push `usize → f32` layout math to the
  shim (lesson L19): pixel coords, scrollbar fractions, animation
  timing, and arena `usize` totals can all be expressed in pure
  Mighty.
- The byte-vector / unicode helpers can drop the
  `usize_to_i32`-accumulator pattern (which built up an I32 via a
  `while` loop instead of casting) — `(v.len() as I32)` now works.
- `std.math` callers can mix `F32` and `F64` in one expression by
  spelling the precision they want once at the boundary.

### Stdlib conversion methods

v0.42 T2 keeps the surface intentionally minimal: only the
`as Ty` operator. Method-style wrappers (`i.to_f32()`,
`u.to_i64()`, …) remain optional sugar — they would be one-line
`#[inline]` wrappers around `as` and don't carry their weight until
authors actually reach for them. The casts.md examples will use
`as Ty` as the canonical spelling; method wrappers may land in a
later track if the dogfooding pressure justifies them.

## Rejection table (post-v0.40 T3)

| Cast | Result | Reason |
|------|--------|--------|
| `Float → Bool` | MT2027 | NaN has no defined truth value; use an explicit predicate. |
| `Bool → Float` | MT2027 | Spelling out `if b { 1.0 } else { 0.0 }` is cheaper and clearer. |
| `Bool → Char` | MT2027 | No defined codepoint mapping (`true → U+0001`?). |
| `Char → Bool` | MT2027 | Same — no defined nonzero-codepoint policy. |
| `<non-literal Int> → Char` | MT2027 (v0.40 T3) | Codepoint validity not verifiable at compile time — use `Char.from_u32(value) -> Option[Char]`. |
| `Str / Bytes / Tuple / Array / Adt → anything` | MT2027 | No scalar-conversion path; use a parser / constructor. |
| `Int → *T`, `*T → Int` | MT2027 | Surface `as` doesn't admit pointer arithmetic; use `unsafe { raw_ptr(addr) }`. |
| `&U8 as *I32` (and friends) | MT2027 | Inner type mismatch — pointer-pointee bit-casts must be explicit unsafe. |

## Diagnostic codes touched by this surface

| Code | Fires at | Introduced |
|------|----------|------------|
| `MT2027 INVALID_CAST` | typeck cast emit-site | v0.37 T2 (extended v0.40 T3 to cover non-literal `Int as Char`) |
| `MT2028 INVALID_CODEPOINT` | literal `Int as Char` with out-of-range value | v0.39 T2 |

`MT5081 INVALID_CODEPOINT_RUNTIME` was **reserved** for a v0.40 runtime
trap design that v0.40 T3 ultimately rejected in favour of the
`Char.from_u32` constructor surface. The reservation is released
(the next freed slot is available for a future runtime diagnostic).
