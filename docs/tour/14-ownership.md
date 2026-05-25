# 14 — Ownership, Borrows, and Drop

Mighty enforces single-owner semantics for non-Copy values. The
compiler tracks every binding's ownership state through the body of each
fn / handler / lambda, and reports moves, borrows, and drops with
matching diagnostics in the **MT3001..MT3099** range.

This chapter walks the rules with worked examples. See
[spec §7](../spec/v1.0-rc.md) and
[`docs/internals/borrowck.md`](../internals/borrowck.md) for the full
reference. Run `mty explain MT3001` (etc.) for the
Cause/Example/Fix/Spec block on any specific code.

## Ownership and `move`

A non-Copy value has exactly one owner. Reassigning **moves** the value:

```mty
let a = String("hello")
let b = move a
// a is now invalid; reading it errors MT3001
```

Without the explicit `move` keyword Mighty does NOT silently move the
value — assignment requires being clear about intent. (Calling a fn that
takes a non-Copy value also moves; see "Calls and parameters" below.)

### What can be reused freely (Copy types)

Primitives, shared references, raw pointers, function pointers, and
tuples/arrays of Copy values are implicitly copyable. No `move` required;
no MT3001 risk.

```mty
let n: I32 = 7
let m = n
let p = n            // fine — I32 is Copy
```

## Immutable borrows: `&T`

A shared borrow lets you read the value without consuming it. Many
shared borrows can coexist:

```mty
let buf = String("data")
let r1 = &buf
let r2 = &buf
log_len(r1)
log_len(r2)
// r1 and r2 decay at end of scope; buf is owned again
```

Borrows deactivate at the **last use of the borrower binding** (NLL,
v0.3 / A55). Programs that were once rejected for lexical reasons now
work — for example:

```mty
let mut buf = String("data")
let r = &buf
log_len(r)                   // r's last use; the shared borrow ends here
let m = &mut buf             // OK — no live shared borrow
push(m, "!")
```

Pre-v0.3 this would have errored with MT3004 because `r`'s borrow was
"live" until the end of the enclosing block. v0.3 sees that `r` is
last used at `log_len(r)`, so the borrow ends there.

## Mutable borrows: `&mut T`

At most one mutable borrow may exist at a time. While it lives, no
shared borrow may coexist:

```mty
let mut buf = String("data")
let m = &mut buf
push(m, "!")
// m decays here; you can now read or mutably borrow buf again
```

Errors you might trip:

- `MT3004 mut_borrow_while_shared` — created `&mut` while `&` was live
- `MT3005 shared_borrow_while_mut` — created `&` while `&mut` was live
- `MT3006 two_mut_borrows` — created a second `&mut`
- `MT3013 mut_borrow_of_immut_local` — used `&mut` on a `let` without `mut`

## Field-level borrows (v0.3 / A54)

Borrows of disjoint fields of the same struct don't conflict:

```mty
struct Pair { a: String, b: String, }

let mut s = Pair { a: String("x"), b: String("y") }
let ra = &mut s.a              // borrow of place s.a
let rb = &s.b                  // borrow of place s.b — disjoint
push(ra, "!")
log_len(rb)                    // both succeed
```

The checker tracks borrows at **Place** granularity — a rooted
projection path like `s.a` or `arr[_]`. Two borrows conflict iff
their Places overlap (one is a prefix of the other). v0.3 truncates
projection chains at depth 1, so `&s.a.x` and `&s.a.y` still conflict
(folded to `&s.a`); v0.4 will deepen.

## Moves through references (v0.3 / A56)

Dereferencing a reference does NOT transfer ownership. For a non-Copy
type, this errors with `MT3009 move_out_of_ref`:

```mty
let s = String("x")
let r = &s
let x = *r                     // MT3009 — can't move out of &String
```

For Copy types (primitives, references, function pointers), `*r` is
just a load:

```mty
let n: I32 = 42
let r = &n
let m = *r                     // OK — I32 is Copy
```

Fix: clone, take ownership, or work with the borrow directly.

## Calls and parameters

Non-Copy arguments are moved into the fn unless the parameter type is
`&T` / `&mut T`:

```mty
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

```mty
fn turn(input: Str) -> Lowered!ParseErr {
  arena turn {
    let toks = tokenize(input)
    let ast = parse(toks)?
    lower(ast)              // OK — lower returns a fresh non-arena value
  }
}
```

Trying to return an arena-local binding directly is `MT3010 arena_escape`:

```mty
fn bad() -> String {
  arena turn {
    let x = String("hi")
    x                       // MT3010 — x is arena-local
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

```mty
fn caller(r: AgentRef[Worker], buf: &String) {
  r!Send(buf)               // MT3011 — &String is not Sendable
}
```

Pass owned data, copies, or convert to a Sendable form first.

## Quick reference

| Symptom                          | Code   | Fix                                              |
|----------------------------------|--------|--------------------------------------------------|
| Used a moved local               | MT3001 | Don't reuse, or `clone` before the move          |
| Borrowed after move              | MT3003 | Same                                             |
| `&mut` while `&` is live         | MT3004 | Reorder, narrow scope, or use a fresh borrow     |
| `&` while `&mut` is live         | MT3005 | Same                                             |
| Two `&mut` to same value         | MT3006 | Sequence them; only one mut borrow at a time     |
| Moved a borrowed value           | MT3008 | Move only after the borrow ends                  |
| Moved out of a reference         | MT3009 | Clone or borrow; don't `*ref` a non-Copy value   |
| Arena-local escapes              | MT3010 | Copy out, or restructure to return a derived val |
| Cross-agent arg is not Sendable  | MT3011 | Pass owned data; don't ship references           |
| `&mut x` but `x` not `mut`       | MT3013 | `let mut x = ...`                                |
| Assigned to non-`mut` local      | MT3014 | `let mut x = ...`                                |
| Used un-initialized binding      | MT3015 | Initialise the binding before its first read     |

## Try it

There is no single `14_ownership.mty` example file — the ownership
machinery is exercised by every program. To experiment, paste any
snippet above into `scratch.mty` and run:

```bash
mty check scratch.mty
```

Borrow conformance is also covered by the
[`tests/conformance/`](https://github.com/hassard0/Mighty/tree/main/tests/conformance)
suite, which runs as part of `cargo test`.

## Next

Continue to [15 — Traits](15-traits.md). For the full borrow-checker
implementation notes, see
[`docs/internals/borrowck.md`](../internals/borrowck.md).
