# Self-hosting Stardust

Self-hosting means **the Stardust compiler can process Stardust source
written in Stardust itself**. It is a long-standing credibility
threshold for any language: until the language can describe its own
compilation, claims about its expressiveness rest on testimony.

This page tracks the Stardust self-hosting roadmap. v0.4 ships the
**lexer** as Stardust source. Parser, HIR lowering, and codegen are
queued for v0.5 / v0.6 / v0.7.

## Roadmap

| Version | Phase | Status | Reference impl |
|---------|-------|--------|---------------|
| v0.4 | Lexer | SUBSET — source compiles, runtime gated on v0.5 loop fix | `crates/sdust-syntax/src/lexer.rs` |
| v0.5 | Parser | future | `crates/sdust-syntax/src/parser/*` |
| v0.6 | HIR lowering | future | `crates/sdust-hir/src/lower/*` |
| v0.7 | Codegen | future | `crates/sdust-codegen-cranelift/*` |

"SUBSET" means the Stardust source `sdust check`s clean and demonstrates
the full token surface, but the v0.3 interpreter cannot execute the
state machine end-to-end (see "Known gaps" below).

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

## Extending to the parser

v0.5 will add `selfhost/parser/` with the same shape:

```
selfhost/parser/
  lib.sd
  parser.sd        # the rowan-style event-driven parser
  events.sd        # OpenNode / CloseNode / Token event ADT
```

The parser will consume the token stream from `selfhost/lexer/lexer.sd`
(once the runtime gap closes) and emit a sequence of CST events. The
trusted Rust parser in `crates/sdust-syntax/src/parser/*` is the diff
target.

The parser is much larger than the lexer (~1500 LOC across 13 files)
and will exercise more of the language: nested matches over
SyntaxKind enums, mutable accumulator structs, recursive descent with
backtracking checkpoints. Several language features the lexer didn't
need will become load-bearing for the parser:

* Real iterative loops (already required by the lexer; will become
  unavoidable for the parser's recovery passes).
* `Vec[T].push` / `pop` with persistent mutation across loop iterations.
* `Option[T]` chained with `?`.
* Disjoint field borrows on the parser state struct (v0.3 borrow
  checker supports this — see `EFFECTS_V0_3_NOTES.md`).

## See also

* `selfhost/README.md` — top-level overview + how to run the bootstrap
  test.
* `SELFHOST_V0_4_NOTES.md` — full v0.4 gap catalog.
* `crates/sdust-driver/tests/selfhost_lexer.rs` — the bootstrap diff
  test.
* `crates/sdust-syntax/src/lexer.rs` — the trusted Rust reference impl.
* `docs/internals/lexer.md` — internals doc for the Rust lexer.
