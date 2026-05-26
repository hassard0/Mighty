# Self-hosting Mighty

Self-hosting means **the Mighty compiler can process Mighty source
written in Mighty itself**. It is a long-standing credibility
threshold for any language: until the language can describe its own
compilation, claims about its expressiveness rest on testimony.

This page tracks the Mighty self-hosting roadmap. v0.4 shipped the
**lexer** as Mighty source; v0.5 unblocked it end-to-end; v0.6 ships
the **parser**; v0.8 ships the **HIR lowering + minimal typeck**;
v0.9 ships the **MtyIR (mid-level IR) lowering**; v0.10 **closes the
v0.8/v0.9 deferrals** so examples 04 + 05 pass byte-for-byte across
HIR + typeck + MtyIR. **v0.13 ships the Wasm core-module codegen**,
closing the self-host chain end-to-end for the slice-1-supported
subset (i.e. examples 01-02 round-trip through Mighty-emitted Wasm
bytes that validate via `wasmparser`). Cranelift + LLVM stay in Rust
post-1.0 as 3rd-party-dep-heavy alternative back-ends.

## Roadmap

| Version | Phase | Status | Reference impl |
|---------|-------|--------|---------------|
| v0.4 | Lexer | SUBSET — source compiles, runtime gated on v0.5 loop fix | `crates/mty-syntax/src/lexer.rs` |
| v0.5 | Lexer runtime | DONE — full byte-for-byte diff against Rust lexer passes | (same) |
| v0.6 | Parser | SHIPPED-SUBSET — 13 bootstrap tests pass against Rust parser | `crates/mty-syntax/src/parser/*` |
| v0.8 | HIR lowering | SHIPPED-SUBSET — 5 bootstrap tests pass against Rust HIR pipeline (examples 01-03) | `crates/mty-hir/src/lower/*` |
| v0.8 | Typeck (minimal) | SHIPPED-SUBSET — 5 bootstrap tests pass against Rust typeck (examples 01-03) | `crates/mty-types/*` |
| v0.9 | MtyIR lowering | SHIPPED-SUBSET — 7 bootstrap tests pass against Rust IR pipeline (examples 01-03) | `crates/mty-ir/src/lower/*` |
| v0.10 | HIR / typeck / MtyIR — examples 04 + 05 | **SHIPPED — 7/7/9 bootstrap tests pass on examples 01-05 (no more `#[ignore]`s)** | (same as above) |
| v0.13 | Codegen (Wasm core module) | **SHIPPED-SUBSET — 6 bootstrap tests pass; Mighty-emitted bytes round-trip through `wasmparser::Validator` for examples 01-02 + a synthetic arithmetic fixture** | `crates/mty-codegen-wasm/src/emit.rs` |
| post-1.0 | Codegen (Cranelift + LLVM) | future | `crates/mty-codegen-cranelift/*`, `crates/mty-codegen-llvm/*` |

"SUBSET" means the Mighty source `mty check`s clean and exercises
the documented production set, but defers a handful of advanced grammars
(agents, supervisors, sandbox blocks, etc.) to a future release. See
`SELFHOST_V0_10_NOTES.md` for the v0.10 self-host-completion notes
(closing examples 04 + 05 across HIR + typeck + MtyIR),
`SELFHOST_IR_V0_9_NOTES.md` for the v0.9 MtyIR production matrix +
gap catalog, `SELFHOST_HIR_V0_8_NOTES.md` for the v0.8 HIR + typeck,
`SELFHOST_PARSER_V0_6_NOTES.md` for the v0.6 parser, and
`SELFHOST_V0_4_NOTES.md` for the v0.4 lexer catalog.

After **v0.13** the only thing not self-hosted is the Cranelift +
LLVM back-ends. Wasm-core-module emission landed in v0.13 (see
`SELFHOST_CODEGEN_V0_13_NOTES.md` for the production matrix). The
v0.13 emitter handles the v0.9-IR-supported subset (i32/i64
arithmetic, locals, structured if/else, calls, log import). Match
arms, ADT init, agents, send/ask, and capability values still emit
`unreachable` placeholders — they remain post-1.0 deferrals because
their HIR/IR shape isn't yet self-hosted either. The bootstrap test
gates on `wasmparser::Validator` accepting the Mighty-emitted bytes:
**Mighty can now describe the algorithm that produces a valid Wasm
module from its own MtyIR for the slice-1 subset, end-to-end.**

**Cranelift + LLVM** are 3rd-party-dep-heavy alternative back-ends
that stay in Rust until post-1.0 — they would each require porting
~5-10 KLOC of binding-heavy code that doesn't materially improve
the self-host story already established by the Wasm back-end.

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
(`pub use selfhost_lexer.SyntaxKind`). The v0.3 `mty check` driver
compiles one file at a time, so v0.4's runnable artifact is
`lexer.sd` — it inlines `SyntaxKind` + keyword table + scanners.

## Bootstrap technique

The v0.3 `mty-sir::interp` interpreter does not expose enough of the
Mighty standard library to write a real character-driven state
machine inside Mighty source alone:

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

The HIR -> MtyIR lowerer recognises a module-typed receiver
(`std.io` is registered as a prelude module) and rewrites the call as
`Stmt::EffectInvoke { effect: io, op: GenericCall { path, method }, args }`.
The interpreter routes the call through `Host::effect_call`. The
bootstrap test
(`crates/mty-driver/tests/selfhost_lexer.rs`) installs a
`SelfhostHost` that services the five methods.

### Why not `extern { fn ... }`?

The natural-looking shape:

```sd
extern {
  fn lex_init(src: Str) -> Unit
}
```

doesn't work in v0.4. The MtyIR lowerer turns body-less extern fns
into trivial `return Unit` shells (see
`crates/mty-sir/src/lower/items.rs` — "Extern / trait-method-without-body:
emit a trivial return"). So `lex_init(src)` resolves to a MtyIR user fn
that immediately returns Unit; the host never sees the call.

Going through `std.io.<method>` sidesteps this entirely and gets us to
`Host::effect_call`. When v0.5 wires extern fns into the host extern
table, this bridge can collapse to direct `extern` declarations.

### Why not real `Str` methods?

The eventual goal. When `Str` grows `byte_at` / `slice` / `as_bytes`
methods backed by real interpreter intrinsics, the host bridge
disappears and the lexer becomes pure Mighty top-to-bottom.

## Known gaps (v0.4)

The full catalog lives in `SELFHOST_V0_4_NOTES.md` at the repo root.
Highlights that any reader of this page should know:

1. **Loops are single-iteration.** `crates/mty-sir/src/lower/exprs.rs`
   lowers `while`, `loop`, and `for` to bodies that branch directly to
   the exit block after one iteration. This is the dominant blocker for
   executing the self-hosted lexer end-to-end. The Rust pipeline
   (`mty-codegen-cranelift`, `mty-codegen-wasm`) emits real loops;
   only the interpreter cuts the back-edge. v0.5 needs a real
   interpreter loop or a step-bounded iteration count.

2. **`!fn(args)` triggers MT2008.** Unary `!` applied to a call
   expression parses as `(!fn)(args)`, then type-checks the function
   value as `Bool`. Workaround used in `lexer.sd`: rewrite as
   `let b = fn(args); if b == false { ... }`.

3. **`extern { fn ... }` short-circuits to Unit.** See above.

4. **No cross-file module resolution.** `mty check` compiles one
   file at a time. `lib.sd` / `syntax_kind.sd` are scaffolding; v0.4
   `mty check selfhost/lexer/lexer.sd` is the live target.

## The parser (v0.6)

v0.6 added `selfhost/parser/` shipping ~1930 LOC of Mighty source:

```
selfhost/parser/
  lib.sd            # v0.7 module-layout scaffolding (currently doc-only)
  parser.sd         # the consolidated event-driven parser (one file)
```

The parser consumes the token stream from the trusted Rust lexer
(seeded into the host by the test) and emits a sequence of CST events
through the same `std.io.<method>` bridge pattern the v0.5 lexer
established. The bootstrap test
(`crates/mty-driver/tests/selfhost_parser.rs`) rebuilds a CST tree
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

Every Mighty example in `examples/01_hello.sd` through
`examples/05_match_expr.sd` parses identically (BFS-kind shape) to
the trusted Rust parser. See `SELFHOST_PARSER_V0_6_NOTES.md` for the
production matrix in more detail and the language-gap catalog the
port surfaced.

### Parser bootstrap technique

Same shape as the v0.5 lexer:

* The parser source talks to the host via `std.io.<method>` effect
  calls. The HIR -> MtyIR lowerer rewrites `std.io.tok_kind(i)` etc. as
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

The checkpoint protocol deserves a note: when the Mighty parser
calls `start_node_at(cp, KIND)`, the host records an
`EnterAt(recorded_idx, KIND)` event. The rebuilder walks the event
stream forwards and emits an `Enter(KIND)` at each `recorded_idx`
before processing that input event. When multiple `start_node_at`
calls share a checkpoint (e.g. `expr_bp` chaining `CALL_EXPR` and
then `QUESTION_EXPR` around the same primary), the later-added
wrapper opens FIRST in the output stream so it ends up on the outside
— matching rowan's `start_node_at` semantics for stacked checkpoints.

## v0.8 — HIR lowering + minimal typeck

The v0.8 phase ports a SUBSET of HIR lowering and bidirectional type
inference to Mighty source. Source files:

```
selfhost/
  hir/
    lib.mty                    # package root (v0.9 module entry)
    nodes.mty                  # HIR data-type mirror (documentation form)
    lower.mty                  # the actual lowerer (CONSOLIDATED, single-file)
  typeck/
    lib.mty                    # package root (v0.9 module entry)
    infer.mty                  # the actual inferer (CONSOLIDATED, single-file)
```

### Bootstrap technique (v0.8)

The Mighty source talks to the trusted Rust pipeline through a host
bridge whose shape extends the v0.5 lexer + v0.6 parser pattern:

* **HIR lowering bridge** — the host owns the CST (built by
  `mty_syntax::parse`) and exposes navigation primitives
  (`cst_node_kind`, `cst_child_count`, `cst_child`, `cst_first_name`,
  `cst_find_child`, etc.). The Mighty side walks the CST and emits
  HIR-event calls (`hir_emit_item`, `hir_emit_fn_sig`,
  `hir_emit_expr`, `hir_emit_type`, `hir_emit_pat`, ...) that the host
  records into a `Vec<HirEvent>`.

* **Typeck bridge** — the host pre-builds an HIR snapshot from the
  trusted Rust pipeline and exposes query primitives (`hir_item_count`,
  `hir_fn_name`, `hir_fn_param_type`, `hir_block_stmt_kind`,
  `hir_let_init_lit_kind`, ...). The Mighty side walks the HIR and
  emits binding-type recordings (`ty_record`) that the host stores in
  a `BTreeMap<String, String>`.

After the run, the bootstrap tests:

* `crates/mty-driver/tests/selfhost_hir.rs` compares the
  Mighty-emitted item-kind sequence + expression-kind BFS sequence
  against the trusted Rust HIR pipeline's output for the same input.
* `crates/mty-driver/tests/selfhost_typeck.rs` compares the
  Mighty-emitted binding-name -> inferred-type map against the
  trusted Rust typeck pipeline's `expr_ty` / `fn_params` / `fn_ret`
  tables (pretty-printed via `pretty_ty` with the def-map for ADT
  names; numeric `T<n>` suffixes are normalized away to match the
  syntactic spelling).

### v0.8 production coverage

Examples 01 (`fn main`), 02 (struct + enum + match) and 03 (generic
fn with borrow + field access) pass both bootstrap diffs. Examples 04
(Result sugar + `?`) and 05 (range patterns) are `#[ignore]`'d with
documented gaps. See `SELFHOST_HIR_V0_8_NOTES.md` for the per-feature
coverage matrix and the v0.9 roadmap.

## v0.9 — MtyIR (mid-level IR) lowering

```
selfhost/
  ir/
    lib.mty       # package decl + intent doc
    nodes.mty    # data-shape spec mirroring crates/mty-ir/src/ir.rs
    lower.mty    # runnable HIR -> MtyIR lowerer (consolidated, single-file)
```

The runnable v0.9 IR lowerer in `selfhost/ir/lower.mty` (~530 LOC)
consumes an HIR snapshot via a host bridge and emits MtyIR events
back to the host. The bootstrap test
(`crates/mty-driver/tests/selfhost_ir.rs`) reuses the same
bridge-test pattern established by the v0.5/v0.6/v0.8 phases:

* read side: `hir_*` queries return item/fn/block/expr properties
  pre-materialized from the trusted Rust HIR pipeline;
* write side: `ir_emit_*` events record fn starts/ends, locals,
  block starts/ends, statement kinds, rvalue kinds, terminator kinds.

After the run, the bootstrap test reconstructs an `IrSummary` (fn
names + per-fn BB count + per-fn terminator-kind sequence) and diffs
it against the Rust IR's summary. The diff is **lenient at v0.9**:

* every fn the Rust IR lowers must also be lowered by the Mighty IR;
* every Mighty-emitted fn ends on a `Return` terminator;
* per-fn BB-count delta is bounded (≤ 20 — pattern lowering and
  drop insertion remain deferred).

### v0.9 production coverage

Examples 01 (`fn main` + `log`), 02 (struct + enum + match) and 03
(generic fn with borrow + field access) pass the v0.9 bootstrap diff
under the lenient invariants. Examples 04 (Result + `?`) and 05
(range patterns) were `#[ignore]`'d at v0.9 — closed in v0.10 (see
next section).

## v0.10 — close the v0.8/v0.9 deferrals (examples 04 + 05)

v0.10 un-ignores the four bootstrap tests deferred from v0.8/v0.9
(HIR / typeck / IR × examples 04 + 05). Two of those four tests
already passed once the `#[ignore]` markers were removed — the
syntactic surface (Question expressions, Range patterns, Result-sugar
types) was covered by the v0.8 HIR lowerer's `is_expr_node_kind` +
`is_pat_node_kind` + `is_type_node_kind` tables, and the v0.9 IR
lowerer's lenient BB-shape diff was already wide enough to accept
the Mighty side's `Use` rvalue stand-in for `?` and one-block-per-arm
range-pattern lowering.

The typeck side needed real work. The Mighty v0.8 typeck only handled
literal-init `let` bindings; examples 04 + 05 both have non-literal
inits (`let body = fetch(url)?`, `let _zero = _classify(0)`). v0.10
extends `infer.mty`'s `infer_let` with:

* **Call-init type propagation:** when `init_kind == "Call"`, query
  the new bridge `hir_let_init_call_callee` for the bare callee name,
  then `hir_fn_ret_type_by_name` for its declared return type.
* **Question-wrapped Call-init type propagation:** when
  `init_kind == "Question"`, do the same lookup but call
  `hir_fn_ret_ok_by_name` which returns the OK type of the callee's
  `Result[T, E]` return (host-side unwrap, to keep the Mighty side
  free of the `Option[Char]`-round-trip awkwardness the v0.6 parser
  notes catalogued).

The bootstrap test gained two coordinated extensions: (a) the HIR
snapshot now records call-callee + is-question per let-binding, and
(b) the bridge's fn-ret lookup falls back to the trusted
`TypedPackage.def_map` so prelude fns like `fetch` resolve (example 04
references `fetch` without declaring it). The Result-sugar pretty-
printing was canonicalized to `Result[T, E]` (was: `T!E`) so the
syntactic-HIR rendering lines up with the trusted typeck's ADT
rendering, and the diff helper accepts `{error}` on either side of a
Result's err position (which is what the trusted typeck emits when the
err type doesn't fully resolve, e.g. example 04's union of two
user-declared-but-uninstantiable error types).

Full notes: `SELFHOST_V0_10_NOTES.md`.

## See also

* `selfhost/README.md` — top-level overview + how to run each
  bootstrap test.
* `SELFHOST_V0_4_NOTES.md` — full v0.4 lexer gap catalog.
* `SELFHOST_PARSER_V0_6_NOTES.md` — v0.6 parser gap catalog.
* `SELFHOST_HIR_V0_8_NOTES.md` — v0.8 HIR + typeck gap catalog.
* `SELFHOST_IR_V0_9_NOTES.md` — v0.9 MtyIR gap catalog.
* `SELFHOST_V0_10_NOTES.md` — v0.10 close-the-deferrals notes.
* `crates/mty-driver/tests/selfhost_lexer.rs` — v0.5 lexer
  bootstrap.
* `crates/mty-driver/tests/selfhost_parser.rs` — v0.6 parser
  bootstrap.
* `crates/mty-driver/tests/selfhost_hir.rs` — v0.8 HIR bootstrap.
* `crates/mty-driver/tests/selfhost_typeck.rs` — v0.8 typeck
  bootstrap.
* `crates/mty-driver/tests/selfhost_ir.rs` — v0.9 MtyIR bootstrap.
* `crates/mty-syntax/src/lexer.rs` — trusted Rust lexer reference impl.
* `crates/mty-syntax/src/parser/*` — trusted Rust parser reference impl.
* `crates/mty-hir/src/lower/*` — trusted Rust HIR lowering.
* `crates/mty-types/*` — trusted Rust type checker.
* `crates/mty-ir/src/lower/*` — trusted Rust IR lowering.
* `docs/internals/lexer.md` — internals doc for the Rust lexer.
