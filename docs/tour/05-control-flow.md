# 05 — Control flow

Mighty has `if`, `match`, `for`, `while`, and `loop`. All but `loop` are
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
mty check examples/05_match_expr.sd
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

## `break` and `continue` (v0.5)

```sd
fn first_match(xs: &[I32], needle: I32) -> Option[USize] {
  for i in 0..xs.len() {
    if xs[i] == needle { return Some(i) }
  }
  None
}

fn retry_until_ok() -> Result[Str, Err] {
  let mut tries = 0
  loop {
    let r = fetch()
    if r.is_ok() { break r }              // break carries the value out
    tries = tries + 1
    if tries >= 5 { break Err("giveup") }
  }
}

fn sum_evens(n: I32) -> I32 {
  let mut total = 0
  for i in 0..n {
    if i % 2 == 1 { continue }            // skip odd, re-enter the loop
    total = total + i
  }
  total
}
```

- `break` unwinds to the nearest enclosing loop. `break <value>`
  makes the loop expression evaluate to `<value>` (only `loop { … }`
  exposes a value today; `while` and `for` always evaluate to `Unit`).
- `continue` re-enters the loop header without running the rest of
  the body.
- Labels (`'outer: loop { break 'outer }`) are not in v0.5 — `break`
  always exits the innermost loop. Labels land in v0.6.

There is no `defer` in v0.1. Deterministic cleanup is handled by
ownership and destructors. See [spec §11.4](../spec/v0.1.md).

Run it:

```bash
mty check examples/06_for_while_loop.sd
```

## Next

Continue to [06 — Agents and protocols](06-agents.md).
