# Loops v0.5 — Loop agent notes

Owner: loop control-flow + iterator-protocol swarm agent.
Status: SHIPPED (HEAD == d6e65de at commit time).

## What landed

| Component                | Slice 4 baseline                          | v0.5 result                                                                 |
| ------------------------ | ----------------------------------------- | --------------------------------------------------------------------------- |
| Parser                   | `break`/`continue` parse as IDENT         | `BREAK_KW` / `CONTINUE_KW` tokens; `break <value>?` + `continue`            |
| HIR                      | no nodes                                  | `HirExpr::Break(Option<ExprId>)`, `HirExpr::Continue`                       |
| Type checker             | n/a                                       | Both synth as `never` (matches `Return`)                                    |
| Borrow checker           | one-pass walk over loop body              | Bounded fixed-point (16 iter cap, conservative `join_states + join_ledgers`)|
| SIR lowering             | header → body → header                    | header → body → continue_tgt → header; break sets result_local, gotos exit  |
| Iterator                 | no exhaustion check                       | `__sdust_iter_next` wire protocol; range + array built-ins                  |
| Self-host bootstrap      | `#[ignore]` w/ "scan_* never returns"     | UN-IGNORED — full token diff vs Rust lexer passes byte-for-byte             |

## Test count

- `parse_break_continue.rs` — 6 new
- `lower_break_continue.rs` — 4 new
- `lower_for_range.rs` — 3 new
- `loop_break.rs` — 5 new
- `loop_continue.rs` — 2 new
- `for_range.rs` — 4 new
- `loop_back_edge.rs` — 4 new
- 5 conformance cases under `tests/conformance/control_flow/`
- 1 unignored selfhost test (`selfhost_lexer_full_diff_against_rust`)

Total: 28 new dedicated tests + 1 unignored. Workspace baseline 692 →
795 tests passing (the +103 includes my 28 plus parallel-agent
additions).

## Interpretation calls (the v0.5 working-agreement log)

1. **No labelled break in v0.5.** Spec doesn't require it; deferring
   to v0.6 keeps the HIR shape simple (Option<value> instead of
   tuple of label+value). Documented in `docs/internals/loops.md` and
   `docs/spec/v0.1-amendments.md` A80.

2. **`break` and `continue` types.** Both synth to `never`. The
   plan-as-written suggested unifying break-value with the loop's
   result type, but doing so requires walking back up through
   nested expressions to find the enclosing loop's type variable.
   v0.5 takes the simpler route: the loop expression's result type
   is whatever the SIR lowering emits (`Unit` for `while`/`for`,
   `result_local`'s carried value for `loop`). v0.6 will land
   proper unification with labels.

3. **`__sdust_iter_next` method dispatch.** Initially I tried to
   make this a trait, but trait-based dispatch in v0.5 would have
   required HIR/type changes the plan flagged as a fallback only.
   The minimal wire protocol via the interpreter's permissive method
   table is what shipped — it works because we control both the
   lowerer and the interpreter, and the cranelift/llvm backends
   handle `MethodCall` via the same lookup path.

4. **Range now carries an inclusivity bit.** `1..5` lowered to
   `Tuple(1, 5)` in v0.4; v0.5 lowers to `Tuple(1, 5, Bool(false))`
   so the iterator can distinguish exclusive (`<`) from inclusive
   (`<=`). Backwards-incompatible with any code that relied on the
   2-tuple shape — but no such code exists yet in the workspace.

5. **Probe temp typed as `Tuple(Bool, Error)`.** The cranelift
   backend computes tuple offsets at codegen time, so it needs
   real type info on tuple-typed places. v0.5 sets the for-loop's
   probe temp to a 2-tuple of (Bool, Error); the second slot is
   permissive so any element type fits at runtime. The interpreter
   ignores the static type.

6. **Borrow checker convergence by ledger record count.** The
   `join_ledgers` operation is monotonic in the records vector (a
   record present in either branch is kept in the join), so equal
   counts between two iterations is a sound convergence condition.
   This avoids deep structural comparison on every iteration.

7. **sdust-doc minimal edit.** The plan says "do not modify
   sdust-doc," but adding a new `HirExpr` variant breaks its
   exhaustive match. I added Break/Continue arms to the one match
   in `extract.rs` — the minimum needed to keep the workspace
   building. Documented in the commit message.

## What didn't ship (deferred)

- **Labelled break/continue.** Spec-deferred to v0.6 with the
  finer-grained label syntax pass.
- **`Iter[T]` trait.** Wire protocol now stable; trait-based
  user iterables wait on the v0.6 stdlib expansion.
- **Break-value unification with loop-result type.** Above (point 2).
- **NLL-style liveness on borrow records inside loop bodies.**
  v0.5 takes a conservative join; v0.6 will refine.

## Risks logged

- The borrow checker's 16-iteration cap is a safety valve. If a
  real program hits it, the analysis is conservative (joins all
  iterations seen) rather than unsound, but the cap will need to
  be revisited if reaching it becomes common. None of the
  in-tree examples hit it as of HEAD.
- The probe-temp tuple type `(Bool, Error)` means cranelift's
  field offset for slot 1 is computed against `Error` (size 0?).
  Field 0 access (the exhausted bool) works fine; field 1 access
  goes through the interpreter today so the codegen never has to
  read it. Once AOT for-loops ship, the type will need to be
  `(Bool, <real element type>)`.
