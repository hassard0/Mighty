# 07 — Send, ask, deadlines

Agents communicate with three operators:

| Operator           | Meaning                          |
|--------------------|----------------------------------|
| `target!Msg(args)` | fire-and-forget send             |
| `target?Msg(args)` | ask-and-await reply              |
| `expr @ duration`  | apply a deadline to `expr`       |

## The program

```mty
fn driver(logger: Logger, fetcher: Fetcher, url: Url) -> Page!FetchErr {
  logger!Info("started")
  let page = fetcher?Page(url) @2s?
  Ok(page)
}
```

## What is interesting

- `logger!Info("started")` sends `Info("started")` to `logger`. It
  does not wait for any reply; the caller continues immediately.
- `fetcher?Page(url) @2s?` is three operators stacked:
  - `?Page(url)` asks `fetcher` for a reply to message `Page(url)`,
    suspending the current task until a reply arrives.
  - `@2s` attaches a 2-second deadline. If the reply does not arrive
    within 2 seconds, the ask fails with `MT5011 deadline_exceeded`
    (or a typed-error variant when the protocol declares one).
  - The trailing `?` propagates that error (or any reply error) out
    of `driver` as `FetchErr`.
- Duration literals (`2s`, `100ms`, `5us`) are first-class tokens in
  the grammar — see the [lexer](../internals/lexer.md).

## Try it

```bash
mty check examples/09_send_ask_deadline.mty
```

## Runtime semantics (v0.7+)

- `!Msg` enqueues into the target agent's mailbox and returns
  immediately. If the mailbox is full and the budget policy is
  `block` (the default) the sender back-pressures; `drop` or `fail`
  policies surface `MT5012 mailbox_full`.
- `?Msg @D` creates a oneshot reply channel, enqueues, and `await`s
  the channel with deadline `D`. Expiry is observed by the caller as
  `Result::Err(DeadlineExceeded)`.
- If the target is dead or unsupervised, the caller sees
  `MT5021 send_to_dead_agent`.

## Next

Continue to [08 — Supervisors](08-supervisors.md).
