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

## Next

Continue to [03 — Generics](03-generics.md).
