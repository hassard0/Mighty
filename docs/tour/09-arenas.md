# 09 — Arenas

Arenas are scoped allocators. Allocations made inside an `arena` block
live as long as the arena and are reclaimed together when the block
exits.

## The program

```sd
fn turn(input: Str) -> Lowered!ParseErr {
  arena turn {
    let toks = tokenize(input)
    let ast = parse(toks)?
    lower(ast)
  }
}

fn turn_short(input: Str) -> Lowered!ParseErr {
  arena turn: lower(parse(tokenize(input))?)
}
```

## What is interesting

- `arena turn { ... }` opens a named arena. The name (`turn`) is
  meaningful — agent message handlers get a default `turn` arena, and
  observability surfaces report arena lifetimes by name.
- Allocations inside the arena cannot escape unless explicitly promoted
  to the caller's region. Slice 4's borrow checker enforces this with
  `MT3010 arena_escape`: an arena body whose tail directly names an
  arena-local non-Copy binding errors. See
  [14 — Ownership](14-ownership.md) for the full rule.
- The compact form `arena turn: expr` is the spec's short-body syntax:
  the arena scope contains a single expression. The expression's value
  is the arena block's value.
- `tokenize`, `parse`, and `lower` here are illustrative free functions;
  in a real program they would live in modules under `use`.

## Run it

```bash
sdust check examples/12_arena.sd
```

## Next

Continue to [10 — Capabilities](10-capabilities.md).
