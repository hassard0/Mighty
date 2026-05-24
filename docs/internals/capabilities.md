# Capabilities (slice 5)

Capabilities represent authority (spec §8). Slice 5 adds typed
capability values to the resolved-type representation.

## Types

```rust
TyData::Cap { family: CapFamily, constraint: CapConstraint }
```

`CapFamily` is one of `Net`, `Fs`, `Clock`, `Dom`, `Model`, or
`Custom(name)` (for user `cap Foo` declarations — currently parsed but
not yet typed).

`CapConstraint`:

| Variant     | Meaning                                                |
|-------------|--------------------------------------------------------|
| `Any`       | top                                                    |
| `ReadOnly`  | read-only (Fs only)                                    |
| `Path(p)`   | path-prefix glob                                       |
| `Host(xs)`  | network host:port allowlist                            |
| `And(xs)`   | conjunction                                            |

## Narrowing constructors

The built-in capability method table provides:

- `cap.ro(path)` → produces `Cap { family, And([ReadOnly, Path(path)]) }`
  (Fs only in semantics; family preserved).
- `cap.path(path)` → narrow to `Path(path)`.
- `cap.host(host)` → narrow to `Host([host])`.

Composition with the existing constraint is always `And(existing, new)`
(set-union of restrictions).

## Subsumption

`narrower.is_narrower_or_eq(broader)` returns true iff the narrower
constraint can be passed where the broader was expected:

- `Any` accepts anything (so `_.is_narrower_or_eq(Any)` is always true).
- Identical constraints are equal.
- `Path(a) ⊑ Path(b)` iff `a.starts_with(b)`.
- `Host(a) ⊑ Host(b)` iff every host in `a` appears in `b`.
- `ReadOnly ⊑ ReadOnly`.
- `And(xs) ⊑ c` iff some `x` in `xs` is narrower than `c`.

Call-site enforcement:

- `synth_call` and `check_cap_subsumption` run after the normal
  type-unify pass. If `actual` is a Cap and `expected` is a Cap of the
  same family, the constraint check runs. Failure: `SD4010
  capability_too_broad`.

## Effects

A capability call carries the family's corresponding effect
(`fs`/`net`/`time`/`dom`/`model`). The effect inferencer's path-prefix
heuristic detects the call shape.

## Affine

Caps are non-Copy and non-Sendable (slice 5; the spec §8.1 sandbox
"explicitly host-provided" caveat is post-v0.1). They participate
normally in the borrow checker's move/borrow tracking.

## v0.3 (A65) tightening

v0.3 hardens the cross-agent gate: the Sendable check (see
`docs/internals/sendable.md`) explicitly classifies
`Cap{family, ...}` values as **non-Sendable**, so any agent
attempting to pass a raw `Fs` or `Net` handle into a `!Msg(...)` /
`?Msg(...)` call now hard-errors with SD3011 carrying a reason note
that points the author at the typed-message-with-narrowed-authority
pattern. The SD4010 capability_too_broad check itself is unchanged
in v0.3; case-shape coverage lives in
`tests/conformance/capability_checking/04_cap_too_broad/` with the
positive-fire path exercised by the
`cap_subsumption_path_too_broad` unit test in sdust-types.
