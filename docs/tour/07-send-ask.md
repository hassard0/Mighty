# 07 — Send, ask, deadlines

Agents communicate with three operators:

| Operator | Meaning |
|---|---|
| `target!Msg(args)` | fire-and-forget send |
| `target?Msg(args)` | ask-and-await reply |
| `expr @ duration` | apply a deadline to `expr` |

## The program

```sd
fn driver(logger: Logger, fetcher: Fetcher, url: Url) -> Page!FetchErr {
  logger!Info("started")
  let page = fetcher?Page(url) @2s?
  Ok(page)
}
```

## What is interesting

- `logger!Info("started")` sends `Info("started")` to `logger`. It does
  not wait for any reply; the caller continues immediately.
- `fetcher?Page(url) @2s?` is three operators stacked:
  - `?Page(url)` asks `fetcher` for a reply to message `Page(url)`,
    suspending the current task until a reply arrives.
  - `@2s` attaches a 2-second deadline. If the reply does not arrive
    within 2 seconds, the ask fails with a timeout error.
  - The trailing `?` propagates that error (or any reply error) out of
    `driver` as `FetchErr`.
- Duration literals (`2s`, `100ms`, `5us`) are first-class tokens in the
  grammar — see the [lexer](../internals/lexer.md).

## Run it

```bash
sdust check examples/09_send_ask_deadline.sd
```

## Next

Continue to [08 — Supervisors](08-supervisors.md).
