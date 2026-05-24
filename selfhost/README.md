# selfhost — Mighty source written in Mighty

This tree holds the parts of the Mighty compiler that are themselves
written in Mighty source. It is the self-hosting milestone — see
[`docs/internals/self-hosting.md`](../docs/internals/self-hosting.md)
for the full architectural story.

## Status

| Phase | Location | Latest status | Bootstrap test |
|---|---|---|---|
| Lexer | `lexer/` | v0.5: DONE — full byte-for-byte diff against Rust lexer | `crates/mty-driver/tests/selfhost_lexer.rs` |
| Parser | `parser/` | **v0.6: SHIPPED-SUBSET — 13 bootstrap tests pass** | `crates/mty-driver/tests/selfhost_parser.rs` |
| HIR lowering | `hir/` | future (v0.7) | — |
| Codegen | `codegen/` | future (v0.8) | — |

## What "SUBSET" means in v0.4

The lexer source in `lexer/lexer.sd`:

- `mty check`s clean (no errors, no warnings)
- type-checks and borrow-checks clean
- compiles to MtyIR via the v0.3 pipeline
- exercises the **full token surface**: every keyword in `spec §3.3`,
  every punctuation token in `spec §3.4`, every literal kind
  (int / float / duration / size / string / char / html-string),
  `//` line comments, `///` doc comments, `/* … */` block comments,
  identifier classification with the 56-entry keyword table.

What it cannot do in v0.4: **execute end-to-end** on a real input.
The v0.3 MtyIR interpreter lowers `while` / `loop` / `for` as
single-iteration (a documented Slice 6 simplification). The bootstrap
test exercises the path as far as the runtime allows — the first
token round-trips correctly through the `std.io` effect bridge to
the host and back as a token record — then defers the full
token-stream diff to v0.5 with `#[ignore = "v0.5 — gated on
iterative-loop interpreter fix"]`.

## Running the bootstrap test

```bash
cargo test -p mty-driver --test selfhost_lexer
```

Three live tests pass + one v0.5-gated test is `#[ignore]`d:

```
test selfhost_lexer_compiles ............................ ok
test selfhost_lexer_first_token_matches ................. ok
test rust_lexer_kind_names_stable ....................... ok
test selfhost_lexer_full_diff_against_rust .............. ignored
```

To re-enable the gated test when v0.5 lands real loops, remove the
`#[ignore]` annotation in `crates/mty-driver/tests/selfhost_lexer.rs`.

## Compiling the lexer source directly

```bash
mty check selfhost/lexer/lexer.sd
mty run   selfhost/lexer/lexer.sd
```

`mty run` with no installed host returns cleanly: the demo `main` in
`lexer.sd` calls `lex("fn main() { log(\"hi\") }")`; the interpreter's
default effect_call returns `Unit` for the host bridge, so the lexer
sees an empty source and exits.

`lib.sd` and `syntax_kind.sd` `mty check` independently too:

```bash
mty check selfhost/lexer/lib.sd
mty check selfhost/lexer/syntax_kind.sd
```

These two files document the intended v0.5+ module layout
(`pub use selfhost_lexer.SyntaxKind`) but the actual runnable code is
all in `lexer.sd` because the v0.4 driver compiles one file at a time.

## v0.4 language gaps the lexer revealed

Catalogued in [`../SELFHOST_V0_4_NOTES.md`](../SELFHOST_V0_4_NOTES.md).
The headline gaps were:

1. Loops execute exactly one iteration (the dominant blocker) — **fixed in v0.5**
2. `!fn(args)` parses as `(!fn)(args)` — workaround `let b = fn(args); if b == false`
3. `extern { fn ... }` short-circuits to `return Unit` instead of
   hitting the host extern table
4. No cross-file module resolution (single-file compile)
5. Permissive Str method stubs (`.contains` always false, etc.)

## v0.6 language gaps the parser revealed

Catalogued in [`../SELFHOST_PARSER_V0_6_NOTES.md`](../SELFHOST_PARSER_V0_6_NOTES.md).
The headline gaps are:

1. `if X { foo() } else { let y = ... }` triggers MT2001 when the
   if-branch ends with a Bool call and the else-branch ends with a
   Unit statement (workaround: return Unit from helper fns when
   possible, or `let _ = ...` to discard)
2. No first-class `SyntaxKind` enum across files — the parser passes
   kinds as Strings because v0.6 still compiles one file at a time
3. `Option[T]` chained with `?` not yet practical at the host-bridge
   boundary; sentinel values used instead
4. String concatenation `"foo " + bar` works in the interpreter but
   not in AOT backends; v0.7 should formalize via `Str + Str` trait
5. Unreachable trailing expressions after `loop { ... }` blocks
   silently type-check (minor; works correctly, no lint)

Each gap has a v0.7+ plan in the parser notes file.

## Running the parser bootstrap test

```bash
cargo test -p mty-driver --test selfhost_parser
```

13 live tests pass (no `#[ignore]` markers):

```
test rust_parser_baseline_hello ............ ok
test selfhost_parser_compiles .............. ok
test selfhost_parser_empty_input_yields_file_root ... ok
test selfhost_parser_event_protocol_smoke .. ok
test selfhost_parser_hello_world ........... ok
test selfhost_parser_struct ................ ok
test selfhost_parser_pratt_arith ........... ok
test selfhost_parser_match_simple .......... ok
test selfhost_parser_example_01 ............ ok
test selfhost_parser_example_02 ............ ok
test selfhost_parser_example_03 ............ ok
test selfhost_parser_example_04 ............ ok
test selfhost_parser_example_05 ............ ok
```

## Compiling the parser source directly

```bash
mty check selfhost/parser/parser.sd
```

## Why ship a subset?

Self-hosting is a milestone of intent as much as runtime behavior.
Shipping `lexer.sd` now:

- locks in the lexer's *semantic surface* in Mighty syntax,
  letting future versions diff against a fixed spec when they
  reorganize the Rust implementation
- exercises the language at lexer-level complexity and surfaces gaps
  (cataloged above) before they accumulate
- is faithful to the brief's "ship a SUBSET if you hit gaps,
  document them" working agreement
- means the v0.5 follow-up is unblocked: lift the runtime gap,
  remove the `#[ignore]`, and the bootstrap diff goes green
