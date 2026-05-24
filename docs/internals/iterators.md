# Iterator protocol (v0.5)

Status: minimal v0.5 protocol. A full trait-based iterator (`Iter[T]`
plus combinators) lands in v0.6 with the stdlib expansion.

## Wire protocol

A type is iterable if `__sdust_iter_next(receiver, idx)` is dispatchable
on it and returns the tuple `(exhausted: Bool, element: T)`.

* `receiver` — the iterable value.
* `idx` — the current iteration counter (USize), 0-indexed.
* `(true, _)` — iteration is finished; the caller exits the loop.
* `(false, v)` — yield `v` as the next element.

The for-lowering bumps `idx` by 1 between calls; it never resets
mid-iteration. Implementations can be either:

* **Stateless** — read `idx` directly. Both v0.5 built-ins (range,
  array) take this path.
* **Stateful** — ignore `idx` and mutate internal state via deref-write.
  Slice-7 will land this once user-defined iterators ship.

## Built-in iterables

### Range (`lo..hi`, `lo..=hi`)

`crates/sdust-sir/src/lower/exprs.rs::lower_binop` lowers `Range` /
`RangeEq` to `TupleInit(lo, hi, inclusive_marker)`. The marker is a
`Bool` literal embedded so the iterator can pick the right termination
predicate at runtime:

* `1..5` → `Tuple(1, 5, false)`; yields `1, 2, 3, 4`.
* `1..=5` → `Tuple(1, 5, true)`; yields `1, 2, 3, 4, 5`.

### Array (and Vec via `&xs.iter()`)

`__sdust_iter_next(Array(xs), idx)` returns `(idx >= xs.len(), xs[idx])`.
`Vec.iter()` currently returns the receiver as-is (see `eval_method`),
so `for v in vec.iter() { … }` works.

### Anything else

Returns `(true, Unit)` immediately so the loop exits rather than
spinning forever. The type checker will eventually reject non-iterable
receivers; for now the runtime simply bails out.

## Adding a new iterable (post-v0.5)

The expected path for v0.6+ is to define an `Iter[T]` trait in the
prelude:

```sd
trait Iter[T] {
  fn next(self: &mut Self) -> Option[T]
}
```

…and have the SIR for-lowering call `iter.next()` instead of the
permissive `__sdust_iter_next` synthetic. The wire-level tuple shape
above is an intentional intermediate so user iterables can be added
without touching the lowerer, just by registering a method on the
interpreter's permissive method table.
