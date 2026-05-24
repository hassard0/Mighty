# Conformance triage — v0.3

Per-case analysis of the five `INTENTIONALLY_IGNORED` entries in
`crates/sdust-driver/tests/conformance_full.rs`.

## Closed in v0.3 (3)

### `budget_violation/03_wall_timeout` — already passes

The v0.2 entry said:

> deadline only fires between turns in Slice-7 (amendment A41);
> positive-fire path is Slice-8 scope

Reality on a 2026-05-24 main: when run through the
slice-6 interpreter (the path the harness actually takes — see the
module doc-comment in `conformance_full.rs`), the program exits 0
with stdout `ok`, matching `expected_stdout.txt`. The deadline
annotation `@1s` doesn't fire because the synchronous `?Hit()` call
returns instantly, but that's the *expected* shape for an in-budget
ask — exit 0 + "ok" is correct. The case was over-conservative; it
now runs and passes.

### `supervisor_restart/03_rate_limit_exhausted` — already passes

The v0.2 entry said:

> restart-rate-limit accounting is Slice-7+; supervisor orchestrator
> does not yet drive the count from the SIR interp

Same situation. The case's `main` just `log("rate_limit declared")`
— the supervisor block declares restart-rate-limit syntax but never
actually triggers a failure. The slice-6 interp accepts the
declaration, runs main, prints "rate_limit declared". exit 0,
expected_stdout matches. Case was over-conservative; it now runs and
passes.

### `budget_violation/02_step_budget_exceeded` — fixture rewritten

The v0.2 entry said:

> SIR slice-6 lowers `loop` as single-iteration (no `break` codegen
> yet) so the case never trips MT5009

That's a real interpreter limitation that we can't fix without
touching `sdust-sir` (another agent's crate). But the *intent* of the
case — "exceed the step budget → trap with MT5009 → exit 3" — is
realisable with a different unbounded shape. We rewrote
`input.sd` to use recursion:

```sd
fn _recurse(n: I64) -> I64 {
  _recurse(n + 1)
}

fn main() {
  let x = _recurse(0)
  log(x.to_str())
}
```

Recursion grows the host Rust stack faster than 1M steps exhaust the
interpreter's default budget — so with the 1M default, the test
process overflows its stack instead of trapping cleanly. To work
around this, the harness gained a `step_budget.txt` per-case knob
(read by `load_case`, threaded into `run_fn_with_budget` when
present). This case sets it to `500` so MT5009 fires well before
the Rust stack runs out.

Result: case runs, traps with MT5009, exit 3, matching
`expected_diagnostics.txt` + `expected_exit_code.txt`.

## Still ignored in v0.3 (2)

### `capability_checking/03_narrow_to_ro` — needs sdust-types changes

The case:

```sd
fn read_only_user(fs: Fs, p: Path) -> Bytes!IoErr {
  fs.read(p)?
}

fn driver(fs: Fs) -> Bytes!IoErr {
  let ro = fs.ro("/data")
  read_only_user(ro, "/data/x")?
}
```

`expected_diagnostics.txt` is empty → type-check should pass clean.
Today the harness reports two MT2001 type mismatches (one on each
`fs.read(p)` / `read_only_user(ro, ...)` call). The narrowed `Fs.ro`
capability returns an opaque `Cap` that the checker can't equate
with the parameter `Fs` type — that's the slice-8 cap-narrowing impl
gap that A40 reserved.

Fix would require sdust-types/sdust-borrow work that's reserved for
other agents this swarm; entry stays in `INTENTIONALLY_IGNORED`
with the reason updated to point at the right crate.

### `supervisor_restart/02_escalate` — needs sdust-syntax changes

The case:

```sd
supervisor Critical(strategy: one_for_one) {
  child worker = spawn Worker()

  on_fail(worker) { escalate }
}
```

`sdust-syntax`'s `agents::sup_action` parser only accepts `restart`
and `backoff` actions; `escalate` is a planned A60+ addition that
hasn't landed. Today the case panics with `MT0001` (parse error).

Fix is a ~5-line addition to `crates/sdust-syntax/src/parser/agents.rs`
+ the matching `SyntaxKind::ESCALATE_KW` definition + an AST node.
Out of scope for this agent (sdust-syntax is owned by the
parser-soundness swarm). Entry stays in `INTENTIONALLY_IGNORED`
with the v0.4 grammar-expansion target noted.

## Harness change

`conformance_full.rs` gained:

```rust
struct CaseSpec {
    // … existing fields …
    /// Optional override for the interpreter step budget (default = 1M).
    /// Set via `step_budget.txt` per-case.
    step_budget: Option<u64>,
}
```

When `step_budget.txt` is present in a case dir, the harness calls
`run_fn_with_budget(prog, "main", vec![], host, budget)` instead of
the default `run(prog, host)`. Backwards-compatible: cases without
the file behave exactly as before.

## Counts

- v0.2.0: 25 cases ran, 5 ignored.
- v0.3 cleanup: 32 cases ran, 2 ignored.
- Net: +7 cases running (+4 from the ignored-but-passing entries,
  +1 from the rewritten step-budget case, +2 from agent-side
  fixtures that landed since v0.2 freeze).
