# Sendable — Cross-Agent Message Marker Trait

> Status: v0.3 (amendment A65). Implemented in
> `crates/sdust-types/src/sendable.rs`. Enforced at every `!Msg(...)`
> and `?Msg(...)` call site; diagnostic code MT3011
> (`non_sendable_message_arg`).

## Why

Stardust agents share nothing. Every value crossing an agent boundary
must be *self-contained*: it must not point back into the sender's
memory, must not name a resource the receiver cannot independently
acquire, and must satisfy the static guarantees the receiver assumes
about its arguments.

Slice-3 sketched this check as "Copy or `Owned`" but only used the
borrow checker's `is_copy` escape, which let real types like `String`
and `Bytes` look unsupported simply because they're not Copy. v0.3
gives the rule a name (Sendable) and a precise formal definition.

## Definition

A type `T` is **Sendable** iff at least one of:

1. `T` is `Copy` (a primitive, a tuple-of-Copy, a `RawPtr`, or a user
   ADT marked `#[derive(Copy)]`). Copy types contain no internal
   pointers and can be bit-copied across the boundary.
2. `T` is *owned* and *Sized* and contains no internal references.
   Concretely: `String`, `Bytes`, plain owned structs / enums whose
   every field is itself Sendable, and `AgentRef[U]` where `U` is
   Sendable.
3. The user marked `T` with `#[derive(Sendable)]`. This opt-in is a
   short-circuit for owned, complex types whose author has audited
   the no-internal-references contract by hand.

A type is **NOT Sendable** if any of:

- `&T` or `&mut T` reference type — references never cross agent
  boundaries by construction.
- A `Cap{family, ...}` capability handle (`Net`, `Fs`, `Clock`, `Dom`,
  `Model`) — these are runtime authorities the receiver must own its
  own narrowed copy of. Sending a cap would silently widen the
  receiver's authority.
- `dyn Trait` — no static guarantee that the underlying impl is
  Sendable.
- Any compound type (`Tuple`, `Array`, struct/enum field) that
  transitively contains a non-Sendable component.

Generic (`Var`, `Param`) types are treated *permissively* — slice-3
inference frequently leaves fresh vars in send-arg positions that pin
to concrete Sendable types after defaulting, and rejecting them at
the type-checker gate would regress canonical examples.

## Where the check fires

At every `HirExpr::Send { target, msg, args }` and `HirExpr::Ask
{ target, msg, args }` site, after each `args[i].value` is
type-checked, `check_sendable_arg(i, arg_ty, span)` resolves the
type through the substitution and asks
`sendable::sendable_reason(...)`. A `Some(reason)` triggers a MT3011
diagnostic carrying the human-readable reason ("contains a `&T`
reference", "field `payload` of `Body` is not Sendable: ...", etc.).

## `derive(Sendable)`

The resolver (in `resolve.rs`) recognises `Sendable` as a marker
derive on `struct` / `enum` items. It registers a synthetic empty
trait impl (so other passes can query "does T impl Sendable?") and
adds the ADT id to `DefMap::user_sendable`. The marker is opt-in: an
author signaling that the field shapes have been audited. No
field-level validation runs at the derive site itself; MT3011 catches
violations at the send site.

## Examples

```stardust
protocol Hi { Greet(name: Str) -> Str }

agent Greeter: Hi { on Greet(name) -> name }

fn ok(g: Greeter, name: Str) -> Unit {
  g!Greet(name)          // Str is Sendable: owned + no internal refs
}

fn bad_ref(g: Greeter, x: I32) -> Unit {
  // MT3011: argument 1 (`&I32`) is not Sendable: contains a `&T`
  // reference (references never cross agent boundaries)
  g!Greet(&x)
}

fn bad_cap(s: SomeAgent, fs: Fs) -> Unit {
  // MT3011: argument 1 (`Fs`) is not Sendable: capability handle `Fs`
  // does not cross agent boundaries
  s!HandOff(fs)
}
```

## Relationship to the borrow checker

The borrow checker (`sdust-borrow`) handles the *liveness* contract:
the value at the send site must be unborrowed (Owned), so the sender
can give it up. Sendable handles the *type-shape* contract: the type
itself must not embed authority the receiver can't honour.

The two run independently — the type-checker phase emits MT3011, the
borrow checker emits MT3009 / MT3008 — and both must pass for a send
to be sound.

## Future work

- v0.4: a `Send` trait that user-declared agents can implement on
  custom message types, deriving Sendable from the bound automatically
  rather than relying on the per-field walk.
- v0.4: cross-package Sendable propagation: when an external crate
  marks a type Sendable, our MT3011 check should see the bound
  through the metadata API.
- v0.5: optional `#[unsafe_sendable]` escape hatch for raw-FFI
  buffers, gated on a sandbox manifest entry.
