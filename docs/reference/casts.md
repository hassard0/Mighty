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

**Non-literal sources** (variables, fn returns, arithmetic expressions)
**pass typeck** and produce the raw bit pattern at runtime. This is a
deliberate v0.39 T2 trade-off:

- Authors who hand-code constants in their source see the diagnostic
  immediately — the most common authoring footgun.
- Computed codepoints that come from a real Unicode-aware source
  (parser output, MIDI byte stream, etc.) flow through without a
  spurious runtime overhead.

v0.40 will tighten the non-literal path. Two designs are on the table
and a decision is deferred until the call-site frequency data lands:

1. **Runtime trap.** Emit a check in IR that traps with a new
   `MT5081 INVALID_CODEPOINT_RUNTIME` if `v >= 0x110000 ||
   (0xD800..=0xDFFF).contains(&v)` at the cast site. Mirrors the
   v0.36 T3 `MT5080 STRING_NOT_CHAR_BOUNDARY` pattern.
2. **`Option[Char]` surface.** Change the result type of non-literal
   `Int as Char` to `Option[Char]`, so the call site is forced to
   match on `Some` / `None`. Equivalent to Rust's
   `char::from_u32(x)`. Better DX but breaks the
   `let c: Char = x as Char` ergonomics that v0.37 T2 established.

Until v0.40 picks one, the safe pattern for runtime-computed
codepoints is a manual guard:

```mty
fn safe_char(v: U32) -> Char {
  if v < 0xD800_u32 || (v >= 0xE000_u32 && v < 0x110000_u32) {
    v as Char
  } else {
    '?'    // or whatever your replacement strategy is
  }
}
```

## Rejection table (post-v0.39 T2)

| Cast | Result | Reason |
|------|--------|--------|
| `Float → Bool` | MT2027 | NaN has no defined truth value; use an explicit predicate. |
| `Bool → Float` | MT2027 | Spelling out `if b { 1.0 } else { 0.0 }` is cheaper and clearer. |
| `Bool → Char` | MT2027 | No defined codepoint mapping (`true → U+0001`?). |
| `Char → Bool` | MT2027 | Same — no defined nonzero-codepoint policy. |
| `Str / Bytes / Tuple / Array / Adt → anything` | MT2027 | No scalar-conversion path; use a parser / constructor. |
| `Int → *T`, `*T → Int` | MT2027 | Surface `as` doesn't admit pointer arithmetic; use `unsafe { raw_ptr(addr) }`. |
| `&U8 as *I32` (and friends) | MT2027 | Inner type mismatch — pointer-pointee bit-casts must be explicit unsafe. |

## Diagnostic codes touched by this surface

| Code | Fires at | Introduced |
|------|----------|------------|
| `MT2027 INVALID_CAST` | typeck cast emit-site | v0.37 T2 |
| `MT2028 INVALID_CODEPOINT` | literal `Int as Char` with out-of-range value | **v0.39 T2** |

`MT5081 INVALID_CODEPOINT_RUNTIME` is **reserved** for the v0.40
runtime check. The diagnostic family currently registers MT5080
(`STRING_NOT_CHAR_BOUNDARY`); the next entry will sit alongside it.
