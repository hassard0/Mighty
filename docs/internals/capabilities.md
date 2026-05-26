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
  same family, the constraint check runs. Failure: `MT4010
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
`?Msg(...)` call now hard-errors with MT3011 carrying a reason note
that points the author at the typed-message-with-narrowed-authority
pattern. The MT4010 capability_too_broad check itself is unchanged
in v0.3; case-shape coverage lives in
`tests/conformance/capability_checking/04_cap_too_broad/` with the
positive-fire path exercised by the
`cap_subsumption_path_too_broad` unit test in mty-types.

## v0.21 — Capability name resolution

`crates/mty-types/src/cap_resolver.rs` ships the `CapResolver` —
the load-bearing API for capability name lookup. The
`crates/mty-types/src/cap_check.rs` pass invokes the resolver over
every typed package after typeck and emits diagnostics in the
**MT4060..MT4065** range.

### Resolver model

The resolver maintains two tracking surfaces:

1. **Module-level registry** (`declared: HashMap<String, CapSpec>`) —
   capabilities visible everywhere in the package.
2. **Scope-frame stack** (`in_scope: Vec<Vec<(String, CapSpec)>>`) —
   each `push_scope` opens a frame; `bind_in_scope` adds a name to
   the topmost frame; `pop_scope` undeclares every name introduced
   in the popped frame.

```rust
let mut r = CapResolver::new();
r.declare("net", CapSpec::top(CapFamily::Net))?;
r.push_scope();
r.bind_in_scope("fs", CapSpec::new(CapFamily::Fs, CapConstraint::ReadOnly))?;
// inside the frame: both names resolve.
r.resolve("fs")?;   // ReadOnly Fs
r.resolve("net")?;  // Top Net
r.pop_scope();
// after pop: `fs` is now a scope violation (MT4062).
```

### The six diagnostics

| Code   | Trigger                                                          |
|--------|------------------------------------------------------------------|
| MT4060 | name referenced but not declared in any scope or registry        |
| MT4061 | declared family differs from use-site's expected family          |
| MT4062 | reference to a popped binding (scope violation)                  |
| MT4063 | same name declared twice in the same scope frame                 |
| MT4064 | method not in the family's built-in surface                      |
| MT4065 | narrowing constructor's constraint not accepted by the family    |

### Family surface

`family_methods(family)` enumerates the **narrowing constructors**
the resolver validates. Operational methods (`read`, `write`,
`get`, `now`, ...) are accepted via `is_operational_method` and
delegated to the typeck permissive fallback. The split prevents
MT4064 from regressing existing programs that call operational
methods on caps:

| Family | Narrowing surface (validated) | Operational (permissive)             |
|--------|-------------------------------|--------------------------------------|
| Fs     | `ro`, `path`                  | `read`, `write`, `list`, `open`, ... |
| Net    | `host`                        | `get`, `post`, `connect`, ...        |
| Clock  | _none_                        | `now`, `sleep`, `elapsed`, ...       |
| Dom    | _none_                        | `query`, `render`, `mount`, ...      |
| Model  | _none_                        | `call`, `stream`, `embed`, ...       |

### Integration

`cap_check::run(typed, pkg, &mut out)` runs three sweeps in order:

1. `sweep_method_calls` — for every `Call { callee: Path([name,
   method]) }` or `MethodCall { receiver, method }` whose receiver
   resolves to a `TyData::Cap`, validate the method against the
   family surface (MT4064) and the narrowing args (MT4065). Across
   fns, name → family collisions surface MT4061.
2. `sweep_scope_violations` — for each top-level fn, restrict the
   walker to the fn's body and emit MT4062 when a cap-name
   declared in a different fn is referenced.
3. `sweep_redeclarations` — for each fn's param list, emit MT4063
   when the same cap-typed name appears twice.

### Conformance

Fixtures `tests/conformance/type_checking/22_*..27_*` exercise each
of MT4060..MT4065 once. Unit tests in
`crates/mty-types/tests/cap_resolution.rs` drive the resolver API
directly for all six paths.
