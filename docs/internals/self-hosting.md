# Self-hosting Stardust

Self-hosting means **the Stardust compiler can process Stardust source
written in Stardust itself**. It is a long-standing credibility
threshold for any language: until the language can describe its own
compilation, claims about its expressiveness rest on testimony.

This page tracks the Stardust self-hosting roadmap. v0.4 shipped the
**lexer** as Stardust source; v0.5 unblocked it end-to-end; v0.6 ships
the **parser**. HIR lowering and codegen are queued for v0.7 / v0.8.

## Roadmap

| Version | Phase | Status | Reference impl |
|---------|-------|--------|---------------|
| v0.4 | Lexer | SUBSET — source compiles, runtime gated on v0.5 loop fix | `crates/sdust-syntax/src/lexer.rs` |
| v0.5 | Lexer runtime | DONE — full byte-for-byte diff against Rust lexer passes | (same) |
| v0.6 | Parser | **SHIPPED-SUBSET — 13 bootstrap tests pass against Rust parser** | `crates/sdust-syntax/src/parser/*` |
| v0.7 | HIR lowering | future | `crates/sdust-hir/src/lower/*` |
| v0.8 | Codegen | future | `crates/sdust-codegen-cranelift/*` |

"SUBSET" means the Stardust source `sdust check`s clean and exercises
the documented production set, but defers a handful of advanced grammars
(agents, supervisors, sandbox blocks, etc.) to a future release. See
`SELFHOST_PARSER_V0_6_NOTES.md` for the v0.6 production matrix +
gap catalog and `SELFHOST_V0_4_NOTES.md` for the v0.4 lexer catalog.

## Where the lexer lives

```
selfhost/
  README.md                  # top-level guide + status table
  lexer/
    lib.sd                   # package root (v0.5 module entry)
    syntax_kind.sd           # SyntaxKind enum mirror
    lexer.sd                 # the actual lexer (CONSOLIDATED, single-file)
```

`lib.sd` and `syntax_kind.sd` document the intended v0.5+ module layout
(`pub use selfhost_lexer.SyntaxKind`). The v0.3 `sdust check` driver
compiles one file at a time, so v0.4's runnable artifact is
`lexer.sd` — it inlines `SyntaxKind` + keyword table + scanners.

## Bootstrap technique

The v0.3 `sdust-sir::interp` interpreter does not expose enough of the
Stardust standard library to write a real character-driven state
machine inside Stardust source alone:

* `Str.contains(...)`, `Str.starts_with(...)`, `Str.ends_with(...)`
  always return `false` in the interpreter (permissive stubs).
* `Str.chars()` is not lowered.
* There is no `Str.byte_at(i)` or `Str.slice(start, end)` in the
  stdlib surface.

Workaround: the self-hosted lexer talks to the source through a
**host bridge** — five methods exposed as `std.io.*` effect calls:

```sd
std.io.lex_init(src)              // cache the source
std.io.lex_len()                  // USize length
std.io.lex_byte_at(i)             // U32 byte, 256 for OOB
std.io.lex_slice(start, end)      // substring as Str
std.io.lex_emit(kind, start, end) // token sink
```

The HIR -> SIR lowerer recognises a module-typed receiver
(`std.io` is registered as a prelude module) and rewrites the call as
`Stmt::EffectInvoke { effect: io, op: GenericCall { path, method }, args }`.
The interpreter routes the call through `Host::effect_call`. The
bootstrap test
(`crates/sdust-driver/tests/selfhost_lexer.rs`) installs a
`SelfhostHost` that services the five methods.

### Why not `extern { fn ... }`?

The natural-looking shape:

```sd
extern {
  fn lex_init(src: Str) -> Unit
}
```

doesn't work in v0.4. The SIR lowerer turns body-less extern fns
into trivial `return Unit` shells (see
`crates/sdust-sir/src/lower/items.rs` — "Extern / trait-method-without-body:
emit a trivial return"). So `lex_init(src)` resolves to a SIR user fn
that immediately returns Unit; the host never sees the call.

Going through `std.io.<method>` sidesteps this entirely and gets us to
`Host::effect_call`. When v0.5 wires extern fns into the host extern
table, this bridge can collapse to direct `extern` declarations.

### Why not real `Str` methods?

The eventual goal. When `Str` grows `byte_at` / `slice` / `as_bytes`
methods backed by real interpreter intrinsics, the host bridge
disappears and the lexer becomes pure Stardust top-to-bottom.

## Known gaps (v0.4)

The full catalog lives in `SELFHOST_V0_4_NOTES.md` at the repo root.
Highlights that any reader of this page should know:

1. **Loops are single-iteration.** `crates/sdust-sir/src/lower/exprs.rs`
   lowers `while`, `loop`, and `for` to bodies that branch directly to
   the exit block after one iteration. This is the dominant blocker for
   executing the self-hosted lexer end-to-end. The Rust pipeline
   (`sdust-codegen-cranelift`, `sdust-codegen-wasm`) emits real loops;
   only the interpreter cuts the back-edge. v0.5 needs a real
   interpreter loop or a step-bounded iteration count.

2. **`!fn(args)` triggers SD2008.** Unary `!` applied to a call
   expression parses as `(!fn)(args)`, then type-checks the function
   value as `Bool`. Workaround used in `lexer.sd`: rewrite as
   `let b = fn(args); if b == false { ... }`.

3. **`extern { fn ... }` short-circuits to Unit.** See above.

4. **No cross-file module resolution.** `sdust check` compiles one
   file at a time. `lib.sd` / `syntax_kind.sd` are scaffolding; v0.4
   `sdust check selfhost/lexer/lexer.sd` is the live target.

## The parser (v0.6)

v0.6 added `selfhost/parser/` shipping ~1930 LOC of Stardust source:

```
selfhost/parser/
  lib.sd            # v0.7 module-layout scaffolding (currently doc-only)
  parser.sd         # the consolidated event-driven parser (one file)
```

The parser consumes the token stream from the trusted Rust lexer
(seeded into the host by the test) and emits a sequence of CST events
through the same `std.io.<method>` bridge pattern the v0.5 lexer
established. The bootstrap test
(`crates/sdust-driver/tests/selfhost_parser.rs`) rebuilds a CST tree
from the events and diffs it BFS against the Rust parser's output.

### Parser status table

| Production group        | v0.6 |
|-------------------------|------|
| `fn` / `struct` / `enum` / `type` decls | shipped |
| `use` / `mod` / `package` decls | shipped |
| `impl` / `trait` / `const` / `extern` | shipped |
| Attributes (`#[derive(...)]` + `derive Copy`) | shipped |
| Types (path, borrow, tuple, array, fn, dyn, generics, `T!E` sugar) | shipped |
| Patterns (literal, binding, wildcard, enum, struct, tuple, range, `&`) | shipped |
| Blocks + `let` + `if`/`else`/`match`/`for`/`while`/`loop` | shipped |
| Pratt expressions (all operators + binding power) | shipped |
| Postfix `()` / `[]` / `.field` / `.method()` / `?` | shipped |
| Macro calls `Path!(...)` | shipped |
| Lambda `fn() { ... }` | shipped |
| Effects clause + `requires` | shipped |
| Send sugar `!Msg(args)`, ask sugar `?Msg(args)`, deadlines `@dur` | deferred to v0.7 |
| `agent` / `protocol` / `supervisor` / `sandbox` / `arena` / `task` / `budget` | deferred to v0.7 |
| `unsafe` / `detach` / `join` / `run` / macro decls / HTML literals | deferred to v0.7 |
| Error recovery (sync_to) | deferred to v0.7 |

Every Stardust example in `examples/01_hello.sd` through
`examples/05_match_expr.sd` parses identically (BFS-kind shape) to
the trusted Rust parser. See `SELFHOST_PARSER_V0_6_NOTES.md` for the
production matrix in more detail and the language-gap catalog the
port surfaced.

### Parser bootstrap technique

Same shape as the v0.5 lexer:

* The parser source talks to the host via `std.io.<method>` effect
  calls. The HIR -> SIR lowerer rewrites `std.io.tok_kind(i)` etc. as
  `EffectOp::GenericCall { path: ["std","io"], method: "tok_kind" }`.
* The bootstrap test installs a `SelfhostParserHost` that:
  1. seeds the token stream from `sdust_syntax::lex(input)`
  2. services the read-only cursor methods (`tok_count`, `tok_kind`,
     `tok_text`, etc.)
  3. records each `ev_start` / `ev_finish` / `ev_token` / `ev_error`
     call as an event
  4. resolves checkpoints (`ev_checkpoint` / `ev_start_at`) into
     retroactive node openings at the saved positions
* After the run, the test rebuilds a CST tree from the event stream
  and compares its BFS-kind shape against the Rust parser's output.

The checkpoint protocol deserves a note: when the Stardust parser
calls `start_node_at(cp, KIND)`, the host records an
`EnterAt(recorded_idx, KIND)` event. The rebuilder walks the event
stream forwards and emits an `Enter(KIND)` at each `recorded_idx`
before processing that input event. When multiple `start_node_at`
calls share a checkpoint (e.g. `expr_bp` chaining `CALL_EXPR` and
then `QUESTION_EXPR` around the same primary), the later-added
wrapper opens FIRST in the output stream so it ends up on the outside
— matching rowan's `start_node_at` semantics for stacked checkpoints.

## See also

* `selfhost/README.md` — top-level overview + how to run the bootstrap
  test.
* `SELFHOST_V0_4_NOTES.md` — full v0.4 gap catalog.
* `crates/sdust-driver/tests/selfhost_lexer.rs` — the bootstrap diff
  test.
* `crates/sdust-syntax/src/lexer.rs` — the trusted Rust reference impl.
* `docs/internals/lexer.md` — internals doc for the Rust lexer.
