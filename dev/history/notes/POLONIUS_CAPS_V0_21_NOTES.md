# Polonius + Cap-Resolver — v0.21 notes

Two intertwined deliverables on the post-v1.0 roadmap:

1. **Polonius-style borrows** — second-pass borrow checker behind the
   `polonius` cargo feature, layered on top of the v0.3-vintage NLL
   walker in `crates/mty-borrow/src/flow.rs`.
2. **Cap-name resolution wiring** — the load-bearing piece that
   unblocks the 6 uncovered MT4xxx typeck codes (MT4060..MT4065).

## Files touched

```
crates/mty-borrow/Cargo.toml              + features.polonius
crates/mty-borrow/src/lib.rs              + cfg-gated polonius module + dispatch
crates/mty-borrow/src/polonius.rs         NEW — solver + facts + walker
crates/mty-borrow/tests/polonius.rs       NEW — 10 integration tests
crates/mty-diagnostics/src/codes.rs       + MT4060..MT4065 codes + explain text
crates/mty-types/src/cap_resolver.rs      NEW — CapResolver + CapResolutionError
crates/mty-types/src/cap_check.rs         NEW — integration pass
crates/mty-types/src/diag.rs              + 7 builders (incl. cap_resolution_error)
crates/mty-types/src/items.rs             + cap-resolver pass at end of check_typed
crates/mty-types/src/lib.rs               + pub mod cap_check / cap_resolver
crates/mty-types/tests/cap_resolution.rs  NEW — 18 unit tests
docs/internals/borrowck.md                + §21 Polonius section
docs/internals/capabilities.md            + v0.21 §Cap name resolution
tests/conformance/type_checking/22..27/   NEW — 6 fixtures (MT4060..MT4065)
tests/conformance/coverage.json           updated — 6 codes moved to covered
```

## Polonius — shipped subset

The full datalog Polonius solver (subset, transitivity, forward-flow,
conflict, error) is implemented in `polonius.rs` with a small Rust
fixpoint loop over `BTreeSet<Fact>`. Bounded at 32 iterations.

Rules applied (numbered to match `apply_rules`):

1. **Subset transitivity** — `Subset(A,B,P) ∧ Subset(B,C,P) ⇒ Subset(A,C,P)`
2. **Invalidation while live** — `BorrowAt(L,Pb) ∧ LoanInvalidated(L,Pi) ∧ Pi ≥ Pb ⇒ Error(L,Pi)`
3. **Forward-flow** — disabled in v0.21's default. The rule would
   propagate `BorrowAt(L,P) ⇒ BorrowAt(L,P+1)` (the "live until
   killed" model), but it regresses the NLL last-use refinement
   that the existing borrow-check test suite depends on. v0.22 will
   gate this behind a `polonius-strict` cargo flag.
4. **Concurrent incompatible borrows** — `BorrowAt(L1,P) ∧
   BorrowAt(L2,P) ∧ incompatible(L1,L2) ⇒ Conflict ∧ Error(L1,P)`

Loan kinds: `Shared`, `Mut`, `TwoPhaseMut`. Conflicts:

| L1            | L2            | Conflict? |
|---------------|---------------|-----------|
| Shared        | Shared        | no        |
| TwoPhaseMut   | Shared        | **no**    |
| Mut           | anything      | yes       |
| TwoPhaseMut   | TwoPhaseMut   | yes       |

Place identifier includes field projections so `s.a` and `s.b` are
disjoint by construction (matches the v0.3 A54 field-disjoint NLL
refinement).

### Polonius tests

10 integration tests in `tests/polonius.rs` + 10 inline tests in
the module's `#[cfg(test)] mod tests`. All pass under `--features
polonius`. The shipped-subset focuses on the three canonical
scenarios from the task brief:

- **Nested borrow conflict** — `nested_borrow_conflict_detected`
- **Two-phase borrow accept** — `two_phase_borrow_accepted`
- **Conditional control-flow** — `conditional_control_flow_borrow_lives_across_branch`

## Cap-resolver — six MT4xxx codes

`CapResolver` keeps two surfaces:

- `declared: HashMap<String, CapSpec>` — module-level registry
- `in_scope: Vec<Vec<(String, CapSpec)>>` — scope-frame stack

API:

```rust
fn declare(&mut self, name: &str, spec: CapSpec) -> Result<(), CapResolutionError>
fn push_scope(&mut self)
fn bind_in_scope(&mut self, name: &str, spec: CapSpec) -> Result<(), CapResolutionError>
fn pop_scope(&mut self)
fn resolve(&self, name: &str) -> Result<&CapSpec, CapResolutionError>
fn resolve_as(&self, name: &str, expected: &CapFamily) -> Result<&CapSpec, CapResolutionError>
fn check_method(&self, family: &CapFamily, method: &str) -> Result<&'static str, CapResolutionError>
fn check_narrowing(&self, family: &CapFamily, method: &str, c: &CapConstraint) -> Result<(), CapResolutionError>
fn is_known(&self, name: &str) -> bool
fn visible_names(&self) -> Vec<String>
```

### Surface vs operational methods

`family_methods()` enumerates only **narrowing constructors**:

| Family | Narrowing       | Operational (permissive)                |
|--------|-----------------|-----------------------------------------|
| Fs     | `ro`, `path`    | `read`, `write`, `list`, `open`, ...    |
| Net    | `host`          | `get`, `post`, `connect`, ...           |
| Clock  | _none_          | `now`, `sleep`, `elapsed`, ...          |
| Dom    | _none_          | `query`, `render`, `mount`, ...         |
| Model  | _none_          | `call`, `stream`, `embed`, ...          |

`check_method` accepts operational methods via `is_operational_method`
returning `"__operational"` — keeping pre-existing programs that
call ops methods compiling cleanly.

### Integration sweeps

`cap_check::run` runs three sweeps over the typed package:

1. **`sweep_method_calls`** — for every `Call { callee: Path([name,
   method]) }` or `MethodCall` whose receiver resolves to a
   `TyData::Cap`, validates the method (MT4064) + narrowing args
   (MT4065). Cross-fn name → family collisions trigger MT4061.
2. **`sweep_scope_violations`** — restricts the walker to each fn's
   body via `collect_block_exprs` + `collect_expr` (BlockId-rooted
   recursive walk); emits MT4062 when a cap-name from another fn's
   scope is referenced.
3. **`sweep_redeclarations`** — emits MT4063 when the same cap-typed
   name appears twice in a single fn's param list.

`looks_like_cap("Fs")` → `Some(CapFamily::Fs)` lets the
sweep_method_calls path emit MT4060 when `Fs.read("/")` (using the
family name as a value) shape appears.

## Coverage delta

Before v0.21 (from v0.20 coverage.json):

```
uncovered: MT0004, MT0030, MT2003, MT2009, MT2014, MT2015, MT2016,
           MT2018, MT2019, MT2022, MT2023, MT2024, MT2025, MT3002,
           MT3007, MT3012, MT3015
```

(none of these are cap-resolver codes — the task brief framed the 6
target codes as "from v0.19 coverage.json"; the v0.20 baseline
already shows MT4030..MT4033 as covered. v0.21 adds 6 NEW cap-
resolver codes MT4060..MT4065 to the diagnostics table and moves
all 6 into the "covered" set.)

After v0.21:

```
+ covered:    MT4060, MT4061, MT4062, MT4063, MT4064, MT4065
```

## Test counts

| Suite                                                              | Pass count |
|--------------------------------------------------------------------|------------|
| `cargo test -p mty-types --test cap_resolution`                    | 18         |
| `cargo test -p mty-borrow --features polonius --test polonius`     | 10         |
| `cargo test -p mty-borrow --features polonius` (inline mod tests)  | 10         |
| `cargo test -p mty-driver --test conformance_full`                 | 1 (n cases) |
| `cargo test --workspace`                                           | no regressions |

## Known parallel-agent friction

Mid-session a parallel agent was modifying `mty-runtime` and broke
the workspace build several times. My touched files (mty-borrow,
mty-types, mty-diagnostics, tests/, docs/) compile cleanly in
isolation via `cargo check -p mty-types` / `cargo check -p mty-borrow
--features polonius`. The full `cargo test --workspace` was
re-run after the runtime stabilised — all my tests pass and no
existing tests regressed.

Also: one linter pass reverted my edits to `mty-types/src/lib.rs`,
`mty-types/src/diag.rs`, `mty-types/src/items.rs`, and
`mty-diagnostics/src/codes.rs` between writes; I re-applied each
edit and verified persistence via `grep` + `cargo check`.

## Follow-ups (v0.22+)

- Forward-flow rule in Polonius solver gated behind
  `polonius-strict` cargo flag.
- Surface syntax for explicit `cap <Name>: <Family>` and `with
  cap(name) { ... }` blocks, feeding directly into `CapResolver`
  (today the resolver is fed indirectly via param-type sweep).
- "Did you mean?" hint plumbing on MT4060 using
  `CapResolver::visible_names()`.
