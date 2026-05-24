# 02 — Types

Stardust has structs, enums, and type aliases. Enums are sum types with
optional payloads, and they are exhaustively matched with `match`.

## The program

```sd
struct User {
  id: UserId
  name: String
}

enum Shape {
  Circle(F64)
  Rect(F64, F64)
}

type UserId = U64

fn area(s: Shape) -> F64 {
  match s {
    Shape.Circle(r) => 3.14159 * r * r
    Shape.Rect(w, h) => w * h
  }
}
```

## What is interesting

- Struct fields are listed one per line, without commas. The formatter
  enforces this style (see [tour chapter 5](05-control-flow.md) for the
  general rule on trailing punctuation).
- Enums carry payloads positionally: `Circle(F64)` takes one `F64`,
  `Rect(F64, F64)` takes two. Patterns destructure positionally too.
- `type UserId = U64` introduces a name alias. It is not a newtype; in
  v0.1 this is a direct synonym.
- `match` is an expression. The arms produce the function's return value
  directly.
- Variants are referred to with `Type.Variant` syntax (see
  [spec §6](../spec/v0.1.md)).

## Run it

```bash
sdust check examples/02_struct_enum.sd
```

## Type errors you might see

```sd
struct User { id: U64, name: String }

// SD2006 unknown field
let u = User { id: 1, name: "x", missing: 2 }

// SD2013 missing field
let u = User { id: 1 }

// SD2014 duplicate field
let u = User { id: 1, id: 2, name: "x" }

// SD2001 type mismatch
let u = User { id: "one", name: "Ada" }   // id expects U64
```

For enums and `match`:

```sd
enum Shape { Circle(F64), Rect(F64, F64) }

// SD2012 wrong variant arity
let s = Shape.Circle(1.0, 2.0)
```

## Copy types

Slice 4 hardcodes which types implicitly copy on use rather than move:

- Primitives — Bool, all Int* and Float*, Char, Unit, Duration, Size
- Shared references `&T`
- Raw pointers `*T`
- `Str` (the string slice; `String` and `Bytes` are NOT Copy)
- Tuples and arrays of Copy elements
- Function pointers

User structs and enums are **not** Copy in v0.1. A non-Copy value moves
on assignment / call / return; use a second copy by introducing the
value at the source again, or by switching to `&T` borrows.

See [ownership](14-ownership.md) for full move and borrow rules.

## Next

Continue to [03 — Generics](03-generics.md).
