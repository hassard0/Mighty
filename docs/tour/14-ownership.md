# 14 — Ownership, Borrows, and Drop

Stardust enforces single-owner semantics for non-Copy values. The
compiler tracks every binding's ownership state through the body of each
fn / handler / lambda, and reports moves, borrows, and drops with
matching diagnostics in the **SD3001..SD3099** range.

This chapter walks the rules with worked examples. See
[spec §7](../spec/v0.1.md) and
[`docs/internals/borrowck.md`](../internals/borrowck.md) for the full
reference.

## Ownership and `move`

A non-Copy value has exactly one owner. Reassigning **moves** the value:

```sd
let a = String("hello")
let b = move a
// a is now invalid; reading it errors SD3001
```

Without the explicit `move` keyword Stardust does NOT silently move the
value — assignment requires being clear about intent. (Calling a fn that
takes a non-Copy value also moves; see "Calls and parameters" below.)

### What can be reused freely (Copy types)

Primitives, shared references, raw pointers, function pointers, and
tuples/arrays of Copy values are implicitly copyable. No `move` required;
no SD3001 risk.

```sd
let n: I32 = 7
let m = n
let p = n            // fine — I32 is Copy
```

## Immutable borrows: `&T`

A shared borrow lets you read the value without consuming it. Many
shared borrows can coexist:

```sd
let buf = String("data")
let r1 = &buf
let r2 = &buf
log_len(r1)
log_len(r2)
// r1 and r2 decay at end of scope; buf is owned again
```

Borrows are lexical (slice 4 has no NLL). They decay at the end of the
innermost enclosing block.

## Mutable borrows: `&mut T`

At most one mutable borrow may exist at a time. While it lives, no
shared borrow may coexist:

```sd
let mut buf = String("data")
let m = &mut buf
push(m, "!")
// m decays here; you can now read or mutably borrow buf again
```

Errors you might trip:

- `SD3004 mut_borrow_while_shared` — created `&mut` while `&` was live
- `SD3005 shared_borrow_while_mut` — created `&` while `&mut` was live
- `SD3006 two_mut_borrows` — created a second `&mut`
- `SD3013 mut_borrow_of_immut_local` — used `&mut` on a `let` without `mut`

## Calls and parameters

Non-Copy arguments are moved into the fn unless the parameter type is
`&T` / `&mut T`:

```sd
fn take(s: String) {}        // takes ownership
fn read(s: &String) {}       // reads via shared borrow
fn fill(s: &mut String) {}   // writes via mutable borrow

let owned = String("x")
read(&owned)                 // shared borrow; owned still usable
take(move owned)             // ownership transferred; owned is now Moved
```

## `Drop` and scope exit

Owned non-Copy values are dropped (deterministically) at end of their
scope. Slice 4 records this as drop **intent** in an internal `DropPlan`;
real codegen of `.drop()` calls arrives in a later slice. From the
language perspective the contract is: when you no longer own a value at
scope end, no leak; when you do own one, its `Drop` runs.

## Arenas: scope-bound allocation

Values created inside an `arena` block may not escape the arena's scope
unless they are Copy or you explicitly `move` them out:

```sd
fn turn(input: Str) -> Lowered!ParseErr {
  arena turn {
    let toks = tokenize(input)
    let ast = parse(toks)?
    lower(ast)              // OK — lower returns a fresh non-arena value
  }
}
```

Trying to return an arena-local binding directly is `SD3010 arena_escape`:

```sd
fn bad() -> String {
  arena turn {
    let x = String("hi")
    x                       // SD3010 — x is arena-local
  }
}
```

To return an arena-local value, copy it (if Copy) or restructure the
computation so the arena's tail is a derived value.

## Cross-agent messages: Sendable

Arguments to `!Msg(args)` (send) or `?Msg(args)` (ask) must be
**Sendable**: Copy ∨ owned-`String`/`Bytes` ∨ Sendable
tuples/arrays/structs. References and raw pointers can't cross agent
boundaries.

```sd
fn caller(r: AgentRef[Worker], buf: &String) {
  r!Send(buf)               // SD3011 — &String is not Sendable
}
```

Pass owned data, copies, or convert to a Sendable form first.

## Quick reference

| Symptom                          | Code   | Fix                                              |
|----------------------------------|--------|--------------------------------------------------|
| Used a moved local               | SD3001 | Don't reuse, or `clone` before the move          |
| Borrowed after move              | SD3003 | Same                                             |
| `&mut` while `&` is live         | SD3004 | Reorder, narrow scope, or use a fresh borrow     |
| `&` while `&mut` is live         | SD3005 | Same                                             |
| Two `&mut` to same value         | SD3006 | Sequence them; only one mut borrow at a time     |
| Moved a borrowed value           | SD3008 | Move only after the borrow ends                  |
| Arena-local escapes              | SD3010 | Copy out, or restructure to return a derived val |
| Cross-agent arg is not Sendable  | SD3011 | Pass owned data; don't ship references           |
| `&mut x` but `x` not `mut`       | SD3013 | `let mut x = ...`                                |
| Assigned to non-`mut` local      | SD3014 | `let mut x = ...`                                |
| Used un-initialized binding      | SD3015 | Initialise the binding before its first read     |

## Next

Slice 5 adds effect closure + capability narrowing enforcement. See the
[README roadmap](../../README.md#roadmap).
