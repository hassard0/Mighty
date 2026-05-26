# v0.18 cross-cut notes — multi-row-var parser surface, MT4059, MSRV gate

This is the small-track v0.18 entry that lands three otherwise
unrelated cleanups in one sweep. The unifying theme is "close v0.17
loose ends":

1. v0.17 RFC-008 broadened the HIR + typeck layers to handle
   `Vec<HirRowVar>` in `HirEffectRow::Open`, but the v0.15 parser
   only emitted a single `EFFECT_ROW_VAR` per signature — users had
   no way to actually WRITE the multi-var shape at the source level.
2. v0.17 reserved MT4059 (`row_var_subsumption_fail`) "pending the
   multi-var parser surface". With the parser shipped, MT4059 has
   the surface it needs to fire meaningfully on user-authored
   `!{| E1, E2}` row-poly fns.
3. KNOWN_ISSUES #3 (MSRV gate runs only `cargo build`, never
   compiles dev-deps) has been an open paper-cut since v0.10. v0.18
   replaces the bare `cargo build --workspace` step with `cargo
   build --workspace --tests`, a strictly larger compile surface
   that catches dev-dep MSRV bumps without adding wall-clock cost.

## Parser: multi-row-var tail

`crates/mty-syntax/src/parser/types.rs::effect_row_tail`

Before v0.18:

```rust
fn effect_row_tail(p: &mut Parser) {
    p.start_node(EFFECT_ROW_TAIL);
    p.bump(PIPE);
    p.skip_trivia();
    if p.at(IDENT) || p.peek().is_keyword() {
        p.start_node(EFFECT_ROW_VAR);
        paths::name_or_keyword(p);
        p.finish_node();
        p.skip_trivia();
    } else {
        p.error("expected row variable identifier after `|`");
    }
    p.finish_node();
    p.skip_trivia();
}
```

After v0.18:

```rust
fn effect_row_tail(p: &mut Parser) {
    p.start_node(EFFECT_ROW_TAIL);
    p.bump(PIPE);
    p.skip_trivia();
    parse_one_row_var(p);
    while p.eat(COMMA) {
        p.skip_trivia();
        parse_one_row_var(p);
    }
    p.finish_node();
    p.skip_trivia();
}
```

The new `parse_one_row_var` helper is the same single-row-var
emit, factored out so both the first row var (mandatory) and any
trailing ones (optional, comma-separated) share one source of
truth. CST shape: each row var still becomes its own
`EFFECT_ROW_VAR` node, all parented under the same single
`EFFECT_ROW_TAIL` — consumers can iterate `.children()` filtered
on `EFFECT_ROW_VAR` uniformly.

### Disambiguation rule (unchanged)

The v0.15 disambiguation contract still holds:

* `!{a, b}` is a closed effect set (two concrete effects, no row).
* `!{a | E}` is concrete + row tail (one row var).
* `!{a | E, F}` is concrete + multi-var tail (NEW in v0.18).
* `!{| E, F}` is row-only multi-var tail (NEW in v0.18).
* `!E` is bare row var (no braces).
* `!{NetErr, ParseErr}` (first ident uppercase, no `|`) stays
  TYPE_RESULT_SUGAR (legacy error union).

The leading `|` token in `!{| E, F}` is the unambiguous tell that
the body is an effect-row clause, not error sugar — `peeks_as_
effect_row_clause` already handles that case correctly.

### Trailing comma rejected

`!{| E,}` is NOT accepted. After `effect_row_tail` consumes the
trailing comma it calls `parse_one_row_var`, which surfaces an
"expected row variable identifier after `|`" diagnostic. Pin in
test `parse_trailing_comma_after_row_var_rejected`.

### Keyword form also gets multi-var

Both the new braced form (`!{| E1, E2}`) and the legacy keyword
form (`effect a, b | E1, E2`) route through `effect_row_tail`, so
the keyword form picks up multi-var "for free". Pin in test
`parse_keyword_form_with_multi_row_tail`.

## HIR lowering note

`crates/mty-hir/src/lower/items.rs::lower_effect_clause` is still
limited to the v0.15 single-row-var path — it calls
`EffectClause::row_var_name()` which returns only the first row
var. The HIR layer is read-only for this v0.18 slice (mandate);
upgrading the lowerer to emit `Vec<HirRowVar>` from the multi-var
parser surface is a v0.19 follow-up. Until then, source-level
`!{| E1, E2}` parses cleanly and the HIR collapses to a single
row var — observationally equivalent to the v0.17 SHIPPED-SUBSET
behaviour at typeck.

## MT4059 active emit

`crates/mty-types/src/effects.rs::walk_expr_for_user_row_violations`
already emits MT4059 at the call site when `enforce_subsumption`
is true and the caller's declared closed row would reject the
closure's effects. v0.18 adds a new test
(`multi_row_var_closed_caller_emits_mt4059`) in
`crates/mty-types/tests/effect_row_multi.rs` exercising MT4059
through the multi-row-var SOURCE-LEVEL fn signature now that
`!{| E, F}` parses.

## MSRV gate hardening

`.github/workflows/ci.yml::msrv`

The previous block ran three steps:

1. `cargo build --workspace` — non-test compile.
2. `cargo test --workspace --no-run` — test compile.
3. `cargo test -p mty-syntax -p mty-types -p mty-fmt -p
   mty-diagnostics` — actually run bedrock tests.

v0.18 collapses 1 + 2 into a single `cargo build --workspace
--tests` invocation, which:

* Compiles every lib / bin target (covering 1).
* Compiles every test / bench / example target (covering 2).
* Pulls in the full `[dev-dependencies]` graph, so a dev-dep MSRV
  bump fails the gate here.
* Skips the test EXECUTION cost (which 2 also skipped).

The bedrock-tests step (3) is kept verbatim so real test
behaviour regressions tied to the MSRV toolchain still get caught.

## Spec ambiguities discovered

None. The RFC-008 grammar already specified the multi-var tail
shape (`| RowVar (, RowVar)*`); v0.15 just shipped the single-var
subset.

## v0.19 follow-ups

* HIR lowerer: read every `EFFECT_ROW_VAR` child of
  `EFFECT_ROW_TAIL` and emit a fully-populated `Vec<HirRowVar>`
  instead of the current first-var-only path. The HIR shape and
  typeck layer are already ready (v0.17).
* Once the lowerer is multi-var-aware, add an end-to-end test
  that exercises the `cross[E1, E2](a, b) -> !{| E1, E2}`
  signature with TWO closure args bringing distinct effects, and
  verify the inferred caller-side row is the union of both
  closures' effects.
* Optional: extend `mty-ast::EffectClause` with a
  `row_var_names()` iterator (multi-var equivalent of the
  existing `row_var_name()` first-only accessor) so the lowerer
  has a clean API to upgrade to.
