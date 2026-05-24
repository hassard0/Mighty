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

## Next

Continue to [05 — Control flow](05-control-flow.md).
