# Self-hosting v0.6 — parser phase notes

This file is the running log of v0.5+ language gaps discovered while
porting the parser to Stardust source. Every entry has:

- a minimal reproducer
- the v0.6 behavior
- the expected v0.7+ behavior
- the workaround used in `selfhost/parser/parser.sd`

See [`docs/internals/self-hosting.md`](docs/internals/self-hosting.md)
for the v0.6 status overview and roadmap, and
[`selfhost/README.md`](selfhost/README.md) for how to run the bootstrap
test.

## Status: SHIPPED-SUBSET

The Stardust source in `selfhost/parser/parser.sd` (~1930 LOC):

- `sdust check`s clean (no errors, no warnings beyond what other crates
  emit globally)
- type-checks and borrow-checks clean
- **runs end-to-end through the SIR interpreter** via the
  `SelfhostParserHost` bootstrap bridge (see
  `crates/sdust-driver/tests/selfhost_parser.rs`)
- produces a CST tree whose BFS-kind shape matches the trusted Rust
  parser's output for the entire subset described in
  "Production matrix" below

**13/13 bootstrap tests pass.** The original v0.6 scope brief expected
"at least 5"; the wider subset means examples 01-05 all pass with no
`#[ignore]` markers.

## Production matrix

| Production group        | v0.6 status | Notes |
|-------------------------|-------------|-------|
| `fn` decls              | full        | params, return type, body, `= expr`, no-body trait methods |
| `struct` decls          | full        | named fields, generic params |
| `enum` decls            | full        | tuple + unit variants, generic params |
| `type` aliases          | full        |       |
| `use` decls             | full        | `use X.{a, b}`, `use X as Y` |
| `mod` / `package` decls | full        |       |
| `impl` blocks           | full        | `impl Trait for Type` + `impl Type` |
| `trait` decls           | full        | with `pub` methods |
| `const` decls           | full        |       |
| `extern { fn ... }`     | full        | (bodies parsed as no-body fn) |
| Attributes              | full        | `#[derive(...)]` + `derive Copy` shorthand |
| Types (path/borrow/...) | full        | path, borrow, ptr, tuple, array, fn-type, dyn, generic args, T!E sugar, T!{E1,E2} union |
| Patterns                | full        | literal, binding, wildcard, enum, struct, tuple, range, `&` ref |
| Blocks + `let`          | full        |       |
| `if` / `else`           | full        | including `if let Pattern = ...` |
| `match`                 | full        | with guard `if expr`, block + expr arms |
| `for` / `while` / `loop`| full        |       |
| Pratt expressions       | full        | every operator + binding power from the Rust impl |
| Postfix `()` `[]` `.`   | full        | method calls, field access, indexing |
| Postfix `?`             | full (subset) | propagate operator only; `?Msg(args)` ask sugar deferred |
| Postfix `!Msg(args)`    | not shipped | send sugar deferred to v0.7 |
| Deadlines `@duration`   | not shipped | deferred to v0.7 |
| Macro calls `Path!(...)`| full        |       |
| Lambda `fn() { ... }`   | full        |       |
| Generic params + bounds | full        | including `T: Trait + Bound` |
| Turbofish `Path::[T]`   | full        |       |
| Effects clause `effect ...` | full    |       |
| `requires` clauses      | full        | (parsed; not validated) |
| HTML literals           | not shipped | leaf token recognised, expression form deferred to v0.7 |
| `agent` / `protocol` / `supervisor` | not shipped | deferred to v0.7 |
| `arena` / `task` / `budget` / `sandbox` blocks | not shipped | deferred to v0.7 |
| `move` / `spawn` exprs  | partial     | `move` shipped; `spawn` parsed only via the `SPAWN_KW` token in `unary_or_primary` is not implemented in v0.6 — falls through to unknown |
| `unsafe` block expressions | not shipped | deferred to v0.7 |
| `detach` / `join`       | not shipped | deferred to v0.7 |
| `run <expr>`            | not shipped | deferred to v0.7 |
| Macro / proc macro decls | not shipped | deferred to v0.7 |
| Error recovery (sync_to)| not shipped | the v0.6 parser bails on unknown tokens with a single error event |
| String concatenation in error messages | shipped via `+` | not previously exercised by any Stardust source — see Gap 4 below |

## Bootstrap technique

The parser runs through the `SelfhostParserHost` bridge — same shape
as the v0.5 lexer host. The parser state lives in the host:

- **Token cursor** (`tok_count`, `tok_kind(i)`, `tok_text(i)`,
  `tok_start(i)`, `tok_end(i)`, `tok_is_trivia(i)`, `tok_is_keyword(i)`)
  — read-only access to the Rust-lexed token stream that the test
  seeds via `host.seed(input)`.
- **Mutating cursor** (`cur_pos`, `cur_set`, `cur_skip_trivia`) — the
  current parse position and the trivia-skipping primitive.
- **Event sink** (`ev_start`, `ev_finish`, `ev_token`, `ev_error`) —
  the CST event stream the test consumes to rebuild a tree.
- **Checkpoint API** (`ev_checkpoint`, `ev_start_at(cp, kind)`) —
  retroactive node wrapping, same semantics as rowan's
  `GreenNodeBuilder::checkpoint` / `start_node_at`.
- **Struct-literal context** (`no_struct_lit_get` / `_set`) — the
  parser's `no_struct_literal` flag used to disambiguate
  `if x { ... }` from `x { ... }` struct expressions.

The test rebuilds a CST from the event stream using a forward walk:
each `EnterAt(recorded_idx, kind)` event becomes an `Enter(kind)`
inserted at the recorded checkpoint position; the wrapper closes via
the next regular `finish_node` event. Multiple `start_node_at` calls
sharing the same checkpoint open in reverse insertion order so the
LATER call wraps the EARLIER one (rowan semantics — see
`expr_bp` chaining `CALL_EXPR(QUESTION_EXPR(...))` around the same
primary).

## Language gaps surfaced

### Gap 1. `if X { ... } else { ... }` requires both branches to be the same type

**Reproducer:**

```sd
fn f() {
  if cond {
    expect("L_BRACK")   // -> Bool
  } else {
    let x = at_kind("DERIVE_KW")  // statement; block evaluates to Unit
  }
}
```

**v0.6 behavior:** MT2001 "expected Bool, found Unit" (or vice versa)
because the `if/else` is parsed as an expression and both branches
must unify.

**Workaround in parser.sd:** changed `expect(k: Str) -> Bool` to
`expect(k: Str) -> Unit`. The previous Bool was rarely used by callers
anyway, and making it Unit lets all the `expect("X")` standalone calls
sit comfortably alongside `let X = ...` statements in the opposite
branch. (43 call sites freed.)

**v0.7+ fix:** the spec already allows blocks with mixed expression /
statement positions, so v0.7 should treat a trailing `expect("X")` in
a block as "expression in statement position; discard value". This
matches Rust's behavior. The minimal patch: in the type checker, when
the if/else result is unused (parent is `EXPR_STMT` or a discarded
let), don't unify the branch types. Until then, the convention is to
explicitly discard Bool-returning calls with `let _ = ...` when they
land at the end of an if-branch facing a Unit sibling.

### Gap 2. Match arms returning different types is permitted (good); BUT match in expression position triggers gap 1

The parser uses many `match k { ... => { foo(); true } }` arms. These
work fine because every arm explicitly returns a Bool literal. The
issue is only when the LAST expression of the block is itself
type-disjoint from siblings. Documenting this here so the v0.7 fixer
doesn't break the working case.

### Gap 3. `loop { ... return X ... }` trailing-expression unreachability

The lookahead helpers like:

```sd
fn next_nontrivia_kind(off: USize) -> Str {
  let n = std.io.tok_count()
  let mut i = pos() + off
  loop {
    if i >= n { return "EOF" }
    ...
  }
  "EOF"   // unreachable
}
```

…have an unreachable trailing `"EOF"` after the `loop`. v0.6 accepts
this (the type checker correctly types the `loop` as `never` and the
trailing expression unifies as the function's return type).

**v0.7+ improvement:** lint for unreachable trailing expressions after
`loop` blocks that never break with a value — currently they're silent
no-ops.

### Gap 4. String concatenation with `+` for diagnostic messages

The parser builds error messages with string concatenation:

```sd
std.io.ev_error("expected " + k, s, e)
```

**v0.6 behavior:** works. `Str + Str` resolves to concatenation via
the interpreter's permissive method dispatch.

**v0.7+ improvement:** the type checker should formally recognize
`Str + Str -> Str` as a built-in trait impl (or special-case via
`std::ops::Add` once the trait surface gels). Today it works by
accident of the interpreter; the Cranelift / Wasm backends don't
handle this lowering, so source that compiles for AOT will fail.
Workaround: use `format!` or a stdlib `str_concat` fn until that
lands.

### Gap 5. No first-class enums for the parser's `SyntaxKind`

The parser passes node kinds as `Str` (`"FN_DECL"`, `"L_PAREN"`, etc.)
because v0.6's `sdust check` driver compiles one file at a time —
there's no way to `import selfhost_lexer.SyntaxKind` and then write
`SyntaxKind.FN_DECL` in the parser source. Same gap the v0.5 lexer
hit (#4 in `SELFHOST_V0_4_NOTES.md`).

**Workaround:** match on string literals, which is verbose but
unambiguous. Both the Rust host and the Stardust parser agree on the
debug-format spelling (`format!("{:?}", kind)`).

**v0.7+ fix:** `sdust-pkg` cross-file module resolution. Then
`syntax_kind.sd` can hold the enum, both lexer and parser can `use
selfhost.SyntaxKind`, and the bootstrap test can compare enum values
directly without going through strings.

### Gap 6. No `Option[T]` in the parser's idiom

The Rust parser uses `Option<u8>` for "is this an infix operator?
what's its binding power?". The Stardust source uses sentinel 0
because `Option` chained with `match` would require generic enums
with `match Some(x) => ...` semantics that v0.6 supports but the
Stardust-side host bridge doesn't yet expose. Acceptable for v0.6;
v0.7 should switch to `Option[U32]` once the bridge can ferry
generics across.

### Gap 7. `match` arms with side-effect-only blocks need explicit unit values

In `pattern()`, every arm of the big `match k { ... }` has to return
Unit explicitly (via `_ => {}`). The `{}` empty-block makes it
unambiguous — but the arms ALSO need a `let ok = true; ...` assignment
since pattern() returns a Bool indicating success.

**Workaround:** introduce a `let mut ok = false` before the match, set
`ok = true` in each successful arm, and return `ok` after.

**v0.7+ fix:** allow `_ => { false }` syntactically without the
intermediate variable. The plumbing is straightforward in the type
checker; the parser's match-arm production already accepts an
expression in the arm body.

## Why ship a subset?

For v0.6 the wider goal is the **two-phase pipeline demonstration**:

1. The Stardust source is a faithful description of the parser
   algorithm. Re-reading it as a v0.7+ implementation guide is the
   reference shape.
2. The host bridge proves that the v0.5 SIR interpreter can drive a
   real recursive-descent loop end-to-end. The 13 passing bootstrap
   tests are direct evidence that:
   - the v0.5 loop fix actually works under heavy use (the parser's
     main loop iterates over every token)
   - `start_node_at` / checkpoint semantics round-trip cleanly
   - the interpreter doesn't blow its budget on the depth of recursion
     induced by Pratt expression parsing or nested struct literals

The five productions deferred to v0.7 (agents, supervisors, etc.)
have substantially more complex grammars (~700 LOC of additional Rust
in `agents.rs` + `concurrency.rs`). Their absence from the Stardust
source doesn't block any of the v0.6 examples 01-05 from parsing
identically to the Rust impl.

## Re-enabling the deferred productions in v0.7

The handoff is mechanical:

1. Pick a deferred group from the table above (e.g. `agent_decl` from
   `crates/sdust-syntax/src/parser/agents.rs`).
2. Translate the Rust function into Stardust source using the same
   patterns as the rest of `selfhost/parser/parser.sd` — checkpoints,
   `start_node` / `finish_node`, the cursor primitives.
3. Add a string-literal arm in `item()` for the new top-level keyword
   (e.g. `"AGENT_KW" => { agent_decl(cp); true }`).
4. Add a new test in `crates/sdust-driver/tests/selfhost_parser.rs`
   targeting that production (e.g. parsing
   `examples/07_agent_echo.sd`) and assert the BFS-kind shape matches
   the Rust parser.

Total v0.7 budget estimate: ~2-3 KLOC of Stardust source for the
remaining productions, all using patterns the v0.6 file already
demonstrates.
