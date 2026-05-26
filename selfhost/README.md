# selfhost — Mighty source written in Mighty

This tree holds the parts of the Mighty compiler that are themselves
written in Mighty source. It is the self-hosting milestone — see
[`docs/internals/self-hosting.md`](../docs/internals/self-hosting.md)
for the full architectural story.

## Status

| Phase | Location | Latest status | Bootstrap test |
|---|---|---|---|
| Lexer | `lexer/` | v0.5: DONE — full byte-for-byte diff against Rust lexer | `crates/mty-driver/tests/selfhost_lexer.rs` |
| Parser | `parser/` | v0.6: SHIPPED-SUBSET — 13 bootstrap tests pass | `crates/mty-driver/tests/selfhost_parser.rs` |
| HIR lowering | `hir/` | v0.10: SHIPPED-SUBSET — 7 bootstrap tests pass (examples 01-05) | `crates/mty-driver/tests/selfhost_hir.rs` |
| Typeck (minimal) | `typeck/` | v0.10: SHIPPED-SUBSET — 7 bootstrap tests pass (examples 01-05) | `crates/mty-driver/tests/selfhost_typeck.rs` |
| MtyIR lowering | `ir/` | v0.10: SHIPPED-SUBSET — 9 bootstrap tests pass (examples 01-05) | `crates/mty-driver/tests/selfhost_ir.rs` |
| Codegen (Wasm core) | `codegen/` | **v0.16: SHIPPED-SUBSET — 21 bootstrap tests pass; Mighty-emitted bytes validate via `wasmparser` for examples 01-03 + arith + pattern-match + string-pool + variant-call + SwitchInt cascade + for-range + MethodCall (resolved + graceful-unresolved) + custom-iter desugar fixtures** | `crates/mty-driver/tests/selfhost_codegen.rs` |
| Codegen (Cranelift / LLVM) | — | future (post-1.0) | — |

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

## v0.8 — HIR lowering + minimal typeck

```bash
mty check selfhost/hir/lower.mty
mty check selfhost/typeck/infer.mty
cargo test -p mty-driver --test selfhost_hir
cargo test -p mty-driver --test selfhost_typeck
```

Seven HIR tests + seven typeck tests pass on examples 01-05 (v0.10
un-ignored the four deferred 04/05 cases; see
[`../SELFHOST_V0_10_NOTES.md`](../SELFHOST_V0_10_NOTES.md)):

```
test selfhost_hir_compiles ........... ok
test selfhost_hir_hello_world ........ ok
test selfhost_hir_example_01 ......... ok
test selfhost_hir_example_02 ......... ok
test selfhost_hir_example_03 ......... ok
test selfhost_hir_example_04 ......... ok    (v0.10 un-ignored)
test selfhost_hir_example_05 ......... ok    (v0.10 un-ignored)

test selfhost_typeck_compiles ........ ok
test selfhost_typeck_hello_world ..... ok
test selfhost_typeck_example_01 ...... ok
test selfhost_typeck_example_02 ...... ok
test selfhost_typeck_example_03 ...... ok
test selfhost_typeck_example_04 ...... ok    (v0.10 un-ignored)
test selfhost_typeck_example_05 ...... ok    (v0.10 un-ignored)
```

The v0.8 HIR source covers the v0.5+v0.6 syntactic surface plus item
lowering (fn / struct / enum / type-alias / use / mod / extern), full
expression lowering (literal / path / call / method-call / field /
index / binary / unary / paren / tuple / array / struct / block / if /
match / for / while / loop / return / break / continue / question /
borrow / cast), pattern lowering (literal / binding / wildcard / tuple /
struct / enum / range / ref) and type lowering (path / borrow / tuple /
array / fn / Result-sugar / Union / Dyn / Unit / Unknown).

The v0.8 typeck source records fn parameter types from explicit
annotations, fn return types from annotations, let-binding types from
annotations, and let-binding types defaulted from literal init
(Int→I32, Float→F64, Str→Str, Char→Char, Bool→Bool). Unification,
trait dispatch, capability narrowing, and effect inference are
deferred to v0.9.

See [`../SELFHOST_HIR_V0_8_NOTES.md`](../SELFHOST_HIR_V0_8_NOTES.md)
for the per-feature coverage matrix, the v0.8 language-gap catalog,
and the v0.9 roadmap.

## v0.9 — MtyIR (mid-level IR) lowering

```bash
mty check selfhost/ir/lib.mty
mty check selfhost/ir/nodes.mty
mty check selfhost/ir/lower.mty
cargo test -p mty-driver --test selfhost_ir
```

Nine live tests pass on examples 01-05 (v0.10 un-ignored the two
deferred 04/05 IR cases — the lenient BB-shape diff already accepted
the v0.9 Mighty output once the test was actually run; see
[`../SELFHOST_V0_10_NOTES.md`](../SELFHOST_V0_10_NOTES.md)):

```
test selfhost_ir_compiles ........... ok
test selfhost_ir_lib_compiles ....... ok
test selfhost_ir_nodes_compiles ..... ok
test selfhost_ir_hello_world ........ ok
test selfhost_ir_example_01 ......... ok
test selfhost_ir_example_02 ......... ok
test selfhost_ir_example_03 ......... ok
test selfhost_ir_example_04 ......... ok    (v0.10 un-ignored)
test selfhost_ir_example_05 ......... ok    (v0.10 un-ignored)
```

The v0.9 IR source covers fn / struct / enum item lowering, full
v0.8 expression lowering (literal / path / call + EffectInvoke
dispatch for log/print/panic / method-call / field / index / binary /
unary / tuple / array / borrow / cast / if / while / loop / for /
return / break / continue / match / block), and synthesizes BB
transitions with `Goto` / `If` / `SwitchInt` / `Return` terminators.

The bootstrap diff is **lenient at v0.9**: every Rust-IR fn is
required to be lowered, every Mighty-lowered fn ends on a `Return`
terminator, and the per-fn BB-count delta is bounded (≤ 20). Tighter
diffing requires landing 8 specific gaps catalogued in
[`../SELFHOST_IR_V0_9_NOTES.md`](../SELFHOST_IR_V0_9_NOTES.md) (the
biggest are: rvalue-to-local linkage, match-arm pattern lowering,
agent + send/ask + arena lowering, drop insertion).

After v0.9, the only thing not self-hosted is the back-end codegen
(Cranelift + LLVM + Wasm — 3rd-party-dep-heavy and probably
post-1.0).

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

## v0.13 — Wasm core-module codegen

```bash
mty check selfhost/codegen/lib.mty
mty check selfhost/codegen/wasm.mty
cargo test -p mty-driver --test selfhost_codegen
```

Six live tests pass (one v0.13-gated test is `#[ignore]`d for the
generic `Option[T]` fn in example 03):

```
test selfhost_codegen_compiles ........... ok
test selfhost_codegen_lib_compiles ....... ok
test selfhost_codegen_hello_world ........ ok
test selfhost_codegen_example_01 ......... ok
test selfhost_codegen_example_02 ......... ok
test selfhost_codegen_arith_fixture ...... ok
test selfhost_codegen_example_03 ......... ignored
```

The v0.13 codegen source covers fn signatures (params + result),
local declarations, i32/i64/f64 const literals, local.get / local.set
for var reads/writes, i32 BinOps (Add/Sub/Mul/Div/Rem/And/Or/Xor/
Shl/Shr/Eq/Ne/Lt/Le/Gt/Ge — signed + unsigned variants), i32 UnOps
(Eqz for Bool-not, Neg synthesized via 0-sub-x), if/else structured
control, return + unreachable + nop terminators, user fn calls, and
log/print/panic builtin sinks routed to an imported `log(ptr, len)`.

The bootstrap test reassembles a real Wasm core module from the
Mighty-emitted event stream (Mighty owns the *algorithm*; the host
handles byte serialization — magic header, LEB128 — because the v0.12
stdlib doesn't yet have Vec[U8] + bitwise byte primitives) and
validates the resulting bytes with `wasmparser::Validator::validate_all`
— the **same correctness gate** the trusted Rust codegen pipeline
uses in `crates/mty-driver/tests/conformance_codegen.rs`.

After v0.13, **the entire Mighty compiler front-end + Wasm back-end
is implemented in Mighty source for the slice-1-supported subset**.
The Cranelift + LLVM back-ends stay in Rust post-1.0 — they don't
materially improve the self-host story already established by the
Wasm back-end (which is the most-portable + most-validated target).

See [`../dev/history/notes/SELFHOST_CODEGEN_V0_13_NOTES.md`](../dev/history/notes/SELFHOST_CODEGEN_V0_13_NOTES.md)
for the per-feature coverage matrix, the v0.13 language-gap catalog,
and the v0.14 roadmap (full match-arm pattern lowering, ADT init
with linear-memory layout, real LEB128 encoder in Mighty source).

## v0.14 — Wasm codegen extended: string pool + ADT layout + patterns

```bash
mty check selfhost/codegen/lib.mty
mty check selfhost/codegen/wasm.mty
mty check selfhost/codegen/string_pool.mty
mty check selfhost/codegen/adt_layout.mty
mty check selfhost/codegen/pattern.mty
cargo test -p mty-driver --test selfhost_codegen
```

Thirteen live tests pass (example 03 is no longer ignored):

```
test selfhost_codegen_compiles .............................. ok
test selfhost_codegen_lib_compiles .......................... ok
test selfhost_codegen_string_pool_compiles .................. ok
test selfhost_codegen_adt_layout_compiles ................... ok
test selfhost_codegen_pattern_compiles ...................... ok
test selfhost_codegen_hello_world ........................... ok
test selfhost_codegen_example_01 ............................ ok
test selfhost_codegen_example_02 ............................ ok
test selfhost_codegen_example_03 ............................ ok
test selfhost_codegen_example_03_option ..................... ok
test selfhost_codegen_arith_fixture ......................... ok
test selfhost_codegen_pattern_match_full .................... ok
test selfhost_codegen_string_const .......................... ok
```

v0.14 closes three v0.13 deferral items:

- **String pool**: every `Const::Str` is now interned into a single
  active data segment exported as `__strings`. The IR const rewrites
  to `i32.const <offset>` (the canonical-ABI ptr; len is resolved at
  use sites via the bridge).
- **ADT linear-memory layout**: tag at offset 0, payload at offset 4+.
  `Rvalue::AdtInit { adt, variant, fields }` now emits a real
  bump-allocated layout via a mutable `$heap_ptr` global instead of
  `unreachable`. Variants compute size = 4 (tag) + max_field_count * 4.
- **Pattern lowering**: `Term::SwitchVariant` now emits the nested
  `block`/`br_if` tag-test cascade so each arm body is reachable real
  code. Variant-field projections lower to `i32.load offset=…`.

The new helper files (`string_pool.mty`, `adt_layout.mty`,
`pattern.mty`) document the intended modular layout. The runnable
emitter inlines them into `wasm.mty` because the v0.12 driver still
compiles one `.mty` file at a time (single-file-compile constraint —
see `selfhost/codegen/lib.mty` for the multi-file design intent).

See [`../dev/history/notes/SELFHOST_CODEGEN_V0_14_NOTES.md`](../dev/history/notes/SELFHOST_CODEGEN_V0_14_NOTES.md)
for the per-feature coverage matrix, the v0.14 language-gap catalog,
and the v0.15 roadmap (variant-call lowering, for-loop iter desugar,
SwitchInt multi-arm support, real LEB128 encoder in Mighty source,
allocator-side arena drop integration).

## v0.15 — variant calls + SwitchInt cascade + for-range desugar

```bash
mty check selfhost/codegen/lib.mty
mty check selfhost/codegen/wasm.mty
mty check selfhost/codegen/string_pool.mty
mty check selfhost/codegen/adt_layout.mty
mty check selfhost/codegen/pattern.mty
mty check selfhost/ir/lower.mty
cargo test -p mty-driver --test selfhost_codegen
```

Seventeen live tests pass (four new fixtures since v0.14):

```
test selfhost_codegen_compiles ............................... ok
test selfhost_codegen_lib_compiles ........................... ok
test selfhost_codegen_string_pool_compiles ................... ok
test selfhost_codegen_adt_layout_compiles .................... ok
test selfhost_codegen_pattern_compiles ....................... ok
test selfhost_codegen_hello_world ............................ ok
test selfhost_codegen_example_01 ............................. ok
test selfhost_codegen_example_02 ............................. ok
test selfhost_codegen_example_03 ............................. ok
test selfhost_codegen_example_03_option ...................... ok
test selfhost_codegen_arith_fixture .......................... ok
test selfhost_codegen_pattern_match_full ..................... ok
test selfhost_codegen_string_const ........................... ok
test selfhost_codegen_variant_call ........................... ok    (v0.15 — new)
test selfhost_codegen_variant_call_qualified ................. ok    (v0.15 — new)
test selfhost_codegen_switch_int_synthetic ................... ok    (v0.15 — new)
test selfhost_codegen_for_range .............................. ok    (v0.15 — new)
```

v0.15 closes three v0.14 deferral items:

- **Variant-call lowering** (Rust-side fix): `Some(42)`, `Maybe.Just(n)`,
  `Result.Ok(v)`, and `Some::<I32>(x)` now all lower to
  `Rvalue::AdtInit { adt, variant, fields }` directly instead of being
  routed through the function-call codepath as `BuiltinId::Extern(name)`.
  The fix lives in `crates/mty-ir/src/lower/exprs.rs::lower_call` via
  a new `variant_for_call_callee` helper that mirrors the type
  checker's path resolution (single segment short name, dotted name,
  and `Enum.Variant` shapes).
- **SwitchInt cascade**: `Term::SwitchInt` now emits a nested-`block`/
  `br_if` cascade — one block per arm + outer "match_done" + dedicated
  "default arm" block. The cascade falls through to the default block
  on no match; each arm body branches back to match_done. v0.14
  emitted `unreachable` for this terminator.
- **For-range desugar** (selfhost-IR-level): `for i in 0..n` and
  `for i in 0..=n` are now detected at the `selfhost/ir/lower.mty`
  layer and rewritten as the equivalent counter+while loop. Non-range
  iterators (slice, array, custom Iter) stay v0.16+.

See [`../dev/history/notes/SELFHOST_V0_15_NOTES.md`](../dev/history/notes/SELFHOST_V0_15_NOTES.md)
for the per-feature coverage matrix, the v0.15 language-gap catalog,
and the v0.16 roadmap (MethodCall lowering for iter-protocol, agent /
send / arena lowering, real LEB128 encoder in Mighty source).

## v0.16 — MethodCall + custom-iter desugar

```bash
mty check selfhost/codegen/lib.mty
mty check selfhost/codegen/wasm.mty
mty check selfhost/codegen/string_pool.mty
mty check selfhost/codegen/adt_layout.mty
mty check selfhost/codegen/pattern.mty
mty check selfhost/codegen/method_call.mty   # v0.16 — new
mty check selfhost/codegen/iter.mty          # v0.16 — new
mty check selfhost/ir/lower.mty
cargo test -p mty-driver --test selfhost_codegen
```

Twenty-one live tests pass (five new fixtures since v0.15):

```
test selfhost_codegen_compiles ............................... ok
test selfhost_codegen_lib_compiles ........................... ok
test selfhost_codegen_string_pool_compiles ................... ok
test selfhost_codegen_adt_layout_compiles .................... ok
test selfhost_codegen_pattern_compiles ....................... ok
test selfhost_codegen_method_call_helper_compiles ............ ok    (v0.16 — new)
test selfhost_codegen_iter_helper_compiles ................... ok    (v0.16 — new)
test selfhost_codegen_hello_world ............................ ok
test selfhost_codegen_example_01 ............................. ok
test selfhost_codegen_example_02 ............................. ok
test selfhost_codegen_example_03 ............................. ok
test selfhost_codegen_example_03_option ...................... ok
test selfhost_codegen_arith_fixture .......................... ok
test selfhost_codegen_pattern_match_full ..................... ok
test selfhost_codegen_string_const ........................... ok
test selfhost_codegen_variant_call ........................... ok
test selfhost_codegen_variant_call_qualified ................. ok
test selfhost_codegen_switch_int_synthetic ................... ok
test selfhost_codegen_for_range .............................. ok    (updated for v0.16)
test selfhost_codegen_method_call_simple ..................... ok    (v0.16 — new)
test selfhost_codegen_method_call_with_args .................. ok    (v0.16 — new)
test selfhost_codegen_method_call_unresolved_graceful ........ ok    (v0.16 — new)
test selfhost_codegen_iter_custom ............................ ok    (v0.16 — new)
```

v0.16 closes the two biggest v0.15 deferral items:

- **MethodCall lowering**: `Rvalue::MethodCall { receiver, method, args }`
  now produces a real Wasm call sequence — push receiver + args + call
  the resolved fn idx (looked up via a new `ir_method_resolve(name)`
  host bridge). On unresolved methods (trait/dyn dispatch the Rust
  pipeline didn't monomorphize), the emitter degrades gracefully to
  an `i32.const 0` placeholder so the module stays validatable. v0.15
  fell through to `unreachable`.
- **Custom-iter for-loop desugar** (selfhost-IR layer): `for x in
  <non-range-iter> { body }` now expands into the iter-protocol
  loop-match-Some/None shape. Combined with the MethodCall lowering,
  for-loops over user-defined iterators now emit real iteration code
  at the Wasm level (no more `unreachable` for the `iter.next()`
  site).

See [`../dev/history/notes/SELFHOST_V0_16_NOTES.md`](../dev/history/notes/SELFHOST_V0_16_NOTES.md)
for the per-feature coverage matrix, the v0.16 interpretation calls,
and the v0.17 roadmap (trait/dyn dispatch, agent/send/arena lowering,
in-Mighty method resolution, real LEB128 encoder in Mighty source,
HIR-to-IR SwitchInt emission for dense matches).
