# Parser audit — v0.9 (non-progress-guard family)

The v0.9 fuzz harness (`FUZZ_V0_9_NOTES.md`) uncovered three OOM bugs in
`mty_syntax`, all instances of the same anti-pattern: a
`while !p.at(R_BRACE) && !p.at(EOF)` loop whose body calls a parsing
helper that can fail to consume any tokens. On adversarial input the
helper makes no progress, the outer `while` re-enters with the same
cursor, and we grow the green tree (one CST node per iteration) until
the allocator gives up — typically around 12 GB.

This document is the result of a sweep over every such loop in
`crates/mty-syntax/src/parser/{items.rs, agents.rs, types.rs, exprs.rs,
stmts.rs, concurrency.rs, extern_.rs}`. For each loop we list:

1. The function and source line.
2. Whether the loop body is **guaranteed to advance** (`p.pos` strictly
   increases per iteration) or **vulnerable** (it can be tricked into
   no progress).
3. The fix applied (if any).

The fix shape is identical everywhere:

```rust
let before = p.pos;
loop_body(p);
if p.pos == before {
    p.error("unexpected token in <context> body");
    p.bump_any();
    p.skip_trivia();
}
```

For loops with an internal `if !p.eat(COMMA) { break; }` (struct/map
literals etc.), an `if p.pos == before { break; }` is sufficient and
keeps the error count cleaner.

## Loops audited

### `items.rs`

| Line | Function                                | Body                                            | Status before | Action |
| ---- | --------------------------------------- | ----------------------------------------------- | ------------- | ------ |
| 9    | `item` (attribute prefix)               | `attribute(p);` — bumps `#` or `derive`         | safe          | none   |
| 118  | `attribute` (derive args)               | `name_or_keyword(p); if !eat(COMMA) break;`     | **vulnerable** | **fixed** — added `if p.pos == before { break; }` |
| 159  | `sandbox_decl` (top-level)              | `path(p); if eat(EQ) expr; eat(COMMA);`         | **vulnerable** | **fixed** — added non-progress guard |
| 199  | `use_decl` (`{a, b}` import list)       | `loop { … if !eat(COMMA) break; }`              | safe — `paths::name` always advances or break-on-no-COMMA | none |
| 323  | `struct_decl`                           | `name(p); if eat(COLON) type_expr;`             | **vulnerable** | **fixed** — added non-progress guard |
| 348  | `enum_decl`                             | `name(p); if eat(L_PAREN) … expect(R_PAREN);`   | **vulnerable** (Bug 1) | **fixed** — added non-progress guard |
| 396  | `impl_block`                            | `if FN_KW fn_decl_pub … else error+bump_any`    | safe-on-paper, defensively-fixed | **fixed** — added belt-and-braces guard |
| 427  | `trait_decl`                            | `if FN_KW fn_decl_pub … else error+bump_any`    | safe-on-paper, defensively-fixed | **fixed** — added belt-and-braces guard |

### `agents.rs`

| Line | Function                          | Body                                            | Status before | Action |
| ---- | --------------------------------- | ----------------------------------------------- | ------------- | ------ |
| 31   | `agent_decl`                      | `agent_member(p);` — has `error+bump_any` else  | safe          | none   |
| 144  | `protocol_decl` (body loop)       | `protocol_msg(p);`                              | **vulnerable** (Bug 3) | **fixed** — added non-progress guard |
| 213  | `supervisor_decl`                 | `sup_body(p);`                                  | **vulnerable** | **fixed** — added non-progress guard |
| 232  | `sup_body::on_fail_clause`        | `sup_action(p); eat(SEMI);` — sup_action has `error+bump_any` else | safe | none |

`protocol_msg` (line 154) itself does not loop on the outer body — its
inner `while p.eat(COMMA)` is progress-guaranteed (the COMMA was just
consumed). The OOM was the caller's loop, not the message body.

### `types.rs`

| Line | Function          | Body                       | Status before | Action |
| ---- | ----------------- | -------------------------- | ------------- | ------ |
| 61   | `tuple`           | `while eat(COMMA) …`       | safe (COMMA progress-guaranteed) | none |
| 93   | `fn_type`         | `while eat(COMMA) …`       | safe          | none |
| 124  | `path_type` (TYPE_UNION inside `!{ … }`) | `while eat(COMMA) …` | safe | none |
| 148  | `generic_args`    | `while eat(COMMA) …`       | safe          | none |
| 171  | `generic_params`  | `while eat(COMMA) …`       | safe          | none |
| 187  | `generic_params::param` | `while eat(PLUS) …`  | safe          | none |
| 206  | `effect_clause`   | `while eat(COMMA) …`       | safe          | none |

No vulnerable loops in `types.rs` — every loop here uses the
`while p.eat(SEPARATOR)` shape, which only re-enters if the separator
was consumed (= guaranteed progress).

### `exprs.rs`

| Line | Function                          | Body                                | Status before | Action |
| ---- | --------------------------------- | ----------------------------------- | ------------- | ------ |
| 58   | `expr_bp` (Pratt loop)            | postfix + binary; each branch bumps an operator | safe | none |
| 279  | `paren_or_tuple` (tuple tail)     | `expr(p); if !eat(COMMA) break;`    | safe — `expr` either consumes a primary or returns false; the `!eat(COMMA)` `break` handles the no-comma case | none |
| 334  | `block_or_map_or_struct` (map)    | `name(p); expect(COLON); expr; if !eat(COMMA) break;` | safe — `break` on no-COMMA terminates if name failed to advance | none |
| 377  | `path_expr_or_call` (struct lit)  | `name(p); if eat(COLON) expr; if !eat(COMMA) break;` | safe — `break` on no-COMMA | none |
| 524  | `args`                            | `while eat(COMMA) …`                | safe          | none |

### `stmts.rs`

| Line | Function           | Body                                          | Status before | Action |
| ---- | ------------------ | --------------------------------------------- | ------------- | ------ |
| 8    | `block`            | dispatch on LET/expr/else (has `error+bump_any`) | safe         | none  |
| 88   | `match_expr`       | `match_arm(p);` — `pattern + expect(FAT_ARROW) + …` | **vulnerable** | **fixed** — added non-progress guard |

### `concurrency.rs`

| Line | Function          | Body                       | Status before | Action |
| ---- | ----------------- | -------------------------- | ------------- | ------ |
| 77   | `budget_block`    | explicit `if !at(IDENT) { error+bump_any; continue; }` | safe (already pre-guarded) | none |
| 117  | `sandbox_block`   | explicit `if !at(IDENT) { error+bump_any; continue; }` | safe (already pre-guarded) | none |

The concurrency module was already defensive about this — the
`error+bump_any+continue` prelude was added in v0.6 or so when the
sandbox grammar was first wired up. Good pattern. Worth migrating
the rest of the parser to this shape over time (post-v1.0 refactor).

### `extern_.rs`

| Line | Function          | Body                                            | Status before | Action |
| ---- | ----------------- | ----------------------------------------------- | ------------- | ------ |
| 16   | `extern_block`    | `extern_fn(p);` (starts with `expect(FN_KW)`)   | **vulnerable** | **fixed** — added non-progress guard |
| 92   | `consume_brace_balanced` | tracks `depth`; every branch bumps      | safe          | none   |

### `macros.rs`, `unsafe_.rs`, `patterns.rs`, `paths.rs`

No `while !p.at(R_BRACE)` or unconditional `loop {}` body that calls
a non-progress-guarding helper. The `paths::path` while-loop only
re-enters if both `DOT` and the following `IDENT` are present (= the
`DOT` will be bumped on `eat`), so it's progress-guaranteed.

## Bugs fixed (FUZZ_V0_9_NOTES.md cross-reference)

| Bug | Locus                          | Fix file                             | Status |
| --- | ------------------------------ | ------------------------------------ | ------ |
| 1   | `items.rs::enum_decl`          | `parser/items.rs`                    | fixed  |
| 2   | (fmt OOM — inherits Bug 1)     | n/a                                  | fixed by inheritance |
| 3   | `agents.rs::protocol_decl/msg` | `parser/agents.rs`                   | fixed  |

## Audit-sweep extras (not originally reported, fixed defensively)

These were not crashed by the fuzz harness but share the anti-pattern
exactly. Fixing them in the same slice avoids a future fuzz run
finding them as P0 v1.0 blockers immediately after v0.9 ships.

- `items.rs::struct_decl`
- `items.rs::trait_decl` (defensive — else branch already bumps)
- `items.rs::impl_block` (defensive — else branch already bumps)
- `items.rs::sandbox_decl` (top-level form)
- `items.rs::attribute` derive-args loop
- `agents.rs::supervisor_decl`
- `stmts.rs::match_expr`
- `extern_.rs::extern_block`

## Regression test coverage

`crates/mty-syntax/tests/parser_non_progress.rs` — 15 tests, all
adversarial inputs that previously triggered the anti-pattern (or
would have triggered it had the fuzz harness been pointed at the
sibling productions). The two "well-formed counterpart" sanity tests
confirm the fixes don't regress happy-path parsing.

Each test has a 5-second wall-clock budget; pre-fix, every one of
these took ~5 s + 12 GB before aborting. Post-fix, they all return in
microseconds (the entire suite runs in <1 ms).

## Follow-ups (v0.10+)

1. **Refactor**: lift the `let before = p.pos; … if p.pos == before
   { error+bump }` idiom into a `Parser::bounded_loop` helper so new
   `while !p.at(R_BRACE)` sites can't reintroduce the bug. Discussed
   for v0.10.
2. **Fuzz re-run on Linux CI**: the Windows-MSVC fuzz path needs the
   asan DLL trick (see FUZZ_V0_9_NOTES.md). Run a 30-minute sweep on
   Linux nightly to look for any new OOMs after these fixes.
3. **Bug 4 (Cranelift egraph stack overflow)**: still open — upstream
   bytecodealliance issue. Workaround is to disable the egraph pass
   in `mty-codegen-cranelift`. Tracked separately.
