# 04 — Errors

Stardust has typed recoverable errors and an explicit propagation
operator. Recoverable failures are values of type `Result[T, E]`. The
sugar `T!E` desugars to `Result[T, E]`.

## The program

```sd
fn parse(s: Str) -> I32!ParseErr {
  Ok(0)
}

fn load(url: Url) -> Page!{NetErr, ParseErr} {
  let body = fetch(url)?
  parse(body)?
  Ok(Page {})
}
```

## What is interesting

- `I32!ParseErr` is shorthand for `Result[I32, ParseErr]`. It is the
  canonical short form per spec §6.3.
- `Page!{NetErr, ParseErr}` is `Result[Page, NetErr | ParseErr]` — a
  function that may fail with either error type.
- `?` propagates errors: `fetch(url)?` returns early with the wrapped
  error if `fetch` returned `Err`, otherwise unwraps to `Bytes`.
- Compose error sets explicitly; there is no implicit "anything goes"
  error type, and there is no exception machinery.
- The final `Ok(Page {})` is the success case. `Page {}` is a struct
  literal with all defaulted fields.

## Run it

```bash
sdust check examples/04_result_propagation.sd
```

## Type errors you might see

`?` is strict about the enclosing function's return type (slice-3
amendment A7):

```sd
// SD2010 ? outside Result-returning function
fn returns_unit() -> Unit {
  fetch(url)?    // can't propagate; fn returns Unit
}

// SD2011 ? error-type mismatch
fn outer() -> Result[I32, NetErr] {
  first()?       // first() returns Result[I32, IoErr] — err types don't match
  Ok(2)
}
```

To compose a fn that doesn't have any natural Result return, use
`Unit!ErrType`:

```sd
fn process(items: &[I32]) -> Unit!WorkErr {
  for item in items {
    work(item)?       // legal — enclosing fn returns Result[Unit, WorkErr]
  }
}
```

## Next

Continue to [05 — Control flow](05-control-flow.md).
