# 03 — Generics

Generic parameters use square brackets, not angle brackets — this keeps
the syntax unambiguous with comparison operators and avoids the
turbofish.

## The program

```sd
fn first[T](xs: &[T]) -> Option[&T] {
  if xs.len == 0 { None } else { Some(&xs[0]) }
}
```

## What is interesting

- `fn first[T](...)` declares a single generic type parameter `T`. No
  constraints — `T` may be any type that fits the body.
- `&[T]` is an immutable borrow of a slice of `T`. Stardust borrows look
  like Rust borrows but are written with square brackets for the slice
  shape.
- `Option[&T]` is the built-in optional type from `std.option`. The
  spec defines `Option[T] = Some(T) | None`.
- `if` is an expression. Both arms produce a value of type `Option[&T]`,
  so the whole `if` does.
- `&xs[0]` borrows the first element without copying it.

## Run it

```bash
sdust check examples/03_generic_fn.sd
```

## Note on syntax limits in slice 1

Generic arguments at expression position (the turbofish form,
`Vec[Str]::new()`) are not yet parsed; only generics in type position
work. See `examples/19_backend_service.sd` for a worked-around case.

## Next

Continue to [04 — Errors](04-errors.md).
