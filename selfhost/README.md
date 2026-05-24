# selfhost — Stardust source written in Stardust

This tree holds the parts of the Stardust compiler that are themselves
written in Stardust source. It is the self-hosting milestone — see
[`docs/internals/self-hosting.md`](../docs/internals/self-hosting.md)
for the full architectural story.

## Status

| Phase | Location | v0.4 status | Bootstrap test |
|---|---|---|---|
| Lexer | `lexer/` | SUBSET — source compiles, runtime gated on v0.5 loop fix | `crates/sdust-driver/tests/selfhost_lexer.rs` |
| Parser | `parser/` | future (v0.5) | — |
| HIR lowering | `hir/` | future (v0.6) | — |
| Codegen | `codegen/` | future (v0.7) | — |

## What "SUBSET" means in v0.4

The lexer source in `lexer/lexer.sd`:

- `sdust check`s clean (no errors, no warnings)
- type-checks and borrow-checks clean
- compiles to SIR via the v0.3 pipeline
- exercises the **full token surface**: every keyword in `spec §3.3`,
  every punctuation token in `spec §3.4`, every literal kind
  (int / float / duration / size / string / char / html-string),
  `//` line comments, `///` doc comments, `/* … */` block comments,
  identifier classification with the 56-entry keyword table.

What it cannot do in v0.4: **execute end-to-end** on a real input.
The v0.3 SIR interpreter lowers `while` / `loop` / `for` as
single-iteration (a documented Slice 6 simplification). The bootstrap
test exercises the path as far as the runtime allows — the first
token round-trips correctly through the `std.io` effect bridge to
the host and back as a token record — then defers the full
token-stream diff to v0.5 with `#[ignore = "v0.5 — gated on
iterative-loop interpreter fix"]`.

## Running the bootstrap test

```bash
cargo test -p sdust-driver --test selfhost_lexer
```

Three live tests pass + one v0.5-gated test is `#[ignore]`d:

```
test selfhost_lexer_compiles ............................ ok
test selfhost_lexer_first_token_matches ................. ok
test rust_lexer_kind_names_stable ....................... ok
test selfhost_lexer_full_diff_against_rust .............. ignored
```

To re-enable the gated test when v0.5 lands real loops, remove the
`#[ignore]` annotation in `crates/sdust-driver/tests/selfhost_lexer.rs`.

## Compiling the lexer source directly

```bash
sdust check selfhost/lexer/lexer.sd
sdust run   selfhost/lexer/lexer.sd
```

`sdust run` with no installed host returns cleanly: the demo `main` in
`lexer.sd` calls `lex("fn main() { log(\"hi\") }")`; the interpreter's
default effect_call returns `Unit` for the host bridge, so the lexer
sees an empty source and exits.

`lib.sd` and `syntax_kind.sd` `sdust check` independently too:

```bash
sdust check selfhost/lexer/lib.sd
sdust check selfhost/lexer/syntax_kind.sd
```

These two files document the intended v0.5+ module layout
(`pub use selfhost_lexer.SyntaxKind`) but the actual runnable code is
all in `lexer.sd` because the v0.4 driver compiles one file at a time.

## v0.4 language gaps the lexer revealed

Catalogued in [`../SELFHOST_V0_4_NOTES.md`](../SELFHOST_V0_4_NOTES.md).
The headline gaps are:

1. Loops execute exactly one iteration (the dominant blocker)
2. `!fn(args)` parses as `(!fn)(args)` — workaround `let b = fn(args); if b == false`
3. `extern { fn ... }` short-circuits to `return Unit` instead of
   hitting the host extern table
4. No cross-file module resolution (single-file compile)
5. Permissive Str method stubs (`.contains` always false, etc.)

Each gap has a v0.5 plan in the notes file.

## Why ship a subset?

Self-hosting is a milestone of intent as much as runtime behavior.
Shipping `lexer.sd` now:

- locks in the lexer's *semantic surface* in Stardust syntax,
  letting future versions diff against a fixed spec when they
  reorganize the Rust implementation
- exercises the language at lexer-level complexity and surfaces gaps
  (cataloged above) before they accumulate
- is faithful to the brief's "ship a SUBSET if you hit gaps,
  document them" working agreement
- means the v0.5 follow-up is unblocked: lift the runtime gap,
  remove the `#[ignore]`, and the bootstrap diff goes green
