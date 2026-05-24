# 05 — Control flow

Stardust has `if`, `match`, `for`, `while`, and `loop`. All but `loop` are
also expressions; they may produce values.

## Match — from chapter 02

```sd
fn classify(n: I32) -> Str {
  match n {
    0 => "zero"
    1..10 => "small"
    _ => "big"
  }
}
```

- `0 =>` matches the integer literal.
- `1..10` is a half-open range pattern (1 through 9). The inclusive form
  is `1..=10`.
- `_` is the wildcard. Match is exhaustive; the wildcard makes that
  trivially true.

Run it:

```bash
sdust check examples/05_match_expr.sd
```

## Loops

```sd
fn process(items: &[I32]) {
  for item in items {
    work(item)?
  }
  while ready() {
    step()
  }
  loop {
    tick()
  }
}
```

- `for item in items` iterates a borrowed slice. The loop variable is a
  borrow because `items` is `&[I32]`.
- `?` inside a loop body propagates the error out of the enclosing
  function, just as in straight-line code.
- `while` runs as long as the condition is true.
- `loop` runs forever, exited via `break`, `return`, or a panic.

There is no `defer` in v0.1. Deterministic cleanup is handled by
ownership and destructors. See [spec §11.4](../spec/v0.1.md).

Run it:

```bash
sdust check examples/06_for_while_loop.sd
```

## Next

Continue to [06 — Agents and protocols](06-agents.md).
