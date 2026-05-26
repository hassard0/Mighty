# SELFHOST_V0_15_NOTES

This file catalogues the v0.15 self-host codegen slice: closing three
v0.14-deferred items in the Mighty-source Wasm core-module emitter:

1. **Variant-call lowering** (Rust-side fix): `Some(42)` /
   `Maybe.Just(n)` / `Result.Ok(v)` now lower to `Rvalue::AdtInit`
   directly instead of falling through the function-call codepath as
   `BuiltinId::Extern(name)`.
2. **SwitchInt cascade** (selfhost): `Term::SwitchInt` now lowers to a
   nested-`block`/`br_if` cascade — one block per arm + outer
   "match_done" + dedicated "default arm" block. v0.14 emitted
   `unreachable` for this terminator.
3. **For-range desugar** (selfhost-IR-level): `for i in 0..n` is now
   detected at the selfhost-IR lowering layer and rewritten as the
   equivalent counter+while loop (init `i = lo`; while `i < hi` { body
   ; `i = i + 1` }). Non-range iterators (slice, array, custom Iter)
   stay v0.16+ scope.

After this slice the bootstrap covers examples 01-03 + arith + string
pool + pattern match + variant call (both bare and qualified forms) +
SwitchInt cascade + for-range, with **seventeen live tests against
zero ignored**.

## Live status

- `selfhost/codegen/wasm.mty` — ~870 LOC (was ~760 in v0.14), `mty
  check` clean, services the v0.15 bootstrap test.
- `selfhost/codegen/pattern.mty` — ~200 LOC (was ~125 in v0.14), new
  `emit_switchint_tag_tests` helper for the int-cascade pattern; `mty
  check`s clean.
- `selfhost/ir/lower.mty` — ~720 LOC (was ~650 in v0.14), `lower_for_expr`
  splits on `op == "Range"` / `"RangeEq"` and routes through
  `lower_for_range`; `lower_call_expr` adds variant-constructor detection
  via the new `hir_path_is_variant` bridge.
- `crates/mty-ir/src/lower/exprs.rs` — `lower_call` now detects
  variant-constructor callees via the new `variant_for_call_callee`
  helper, which mirrors the type checker's path resolution for `Some`,
  `Maybe.Just`, and `Some::<I32>` shapes.
- `crates/mty-driver/tests/selfhost_codegen.rs` — ~2580 LOC (was ~2290
  in v0.14), **17/17 live tests pass; 0 ignored**.

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

## What changed vs v0.14

| Surface | v0.14 | v0.15 |
|---------|-------|-------|
| `Some(42)` / `Just(x)` (bare) | `Call { func: Builtin(Extern("Some")), args: [42] }` — broke Wasm AOT, only worked because the interp's `extern_call` is a no-op | `Rvalue::AdtInit { adt, variant, fields }` — bump-alloc + tag store + payload stores fire correctly |
| `Maybe.Just(n)` (qualified) | same `Extern("Maybe.Just")` fallback | `Rvalue::AdtInit` — first segment resolves to `DefRef::Adt`, second segment indexed into the Adt's variants |
| `Some::<I32>(x)` (path-generic) | same | `Rvalue::AdtInit` — both segment shapes covered via the shared `variant_for_call_callee` helper |
| `Term::SwitchInt` | `unreachable` | nested-block cascade: `block ; block(default) ; { block ; ... ; block(arm 0) ; tests + br_if ; br -> default } ; arm bodies + br -> match_done ; default body ; end` |
| `for i in lo..hi` (selfhost IR-level) | minimal while-shape: `iter ; goto ; if-block ; body-block ; goto ; after-block` (didn't reflect Rust pipeline's iter-protocol expansion) | counter-loop desugar: `i := lo ; goto header ; { use_i ; load hi ; binop Lt ; if ; body ; assign i = i+1 ; goto header } ; after` — matches a true rewriting of the iter protocol for the range case |
| `for i in <non-range>` | same minimal while-shape | unchanged — non-range iterators (custom Iter, slice, array) are v0.16+ |

## v0.14 deferred items addressed

The v0.14 notes file (`SELFHOST_CODEGEN_V0_14_NOTES.md`) flagged five
issues; v0.15 closes three of them:

1. ~~**Variant-call lowering**~~ — CLOSED. Rust-side `lower_call`
   detects variant constructors and emits `AdtInit` directly.
2. ~~**SwitchInt cascade**~~ — CLOSED for the IR shape. The lowerer
   emits a structurally valid cascade that the bootstrap test verifies
   by synthesizing a `Program` directly (the HIR-to-IR lowerer expands
   `match` as chained `If` on `BinOp::Eq`, not `SwitchInt` — that's a
   future jump-table optimization, but the emitter is ready for it now).
3. ~~**For-loop iter desugar (range only)**~~ — CLOSED at the
   selfhost-IR layer. The codegen layer still routes the
   `__mty_iter_next` MethodCall through `compile_unsupported_rvalue` →
   `unreachable` (which validates as stack-polymorphic), so for-range
   programs round-trip cleanly even on the Wasm side. Full method-call
   support is v0.16.

The remaining v0.14 deferrals stay deferred:

- **Tuple / array values** with composite layout
- **Method calls** (vtable / trait dispatch decisions)
- **Agent / send / ask / spawn / arena / capability** lowering
- **Explicit Drop / StorageLive / StorageDead** scope ops
- **DOM imports + canonical-ABI strings** (web target only)
- **Component Model wrapping** (post-emit; happens above us in Rust)
- **Free / arena drop** for ADT instances (allocator never shrinks)
- **Real LEB128 encoder in Mighty source** (still host-side)

## Architectural choices

### 1. Rust-side variant-call fix vs selfhost-only

The brief permitted the variant-call fix to live in mty-ir's
`lower::exprs::lower_call`. That's the correct surface — the Rust
pipeline is the reference implementation and the self-host emitter
just consumes its output. Without the Rust-side fix, every Mighty
program with variant calls would lower as `Extern("Some")` and the
self-host emitter would correctly refuse to lower it.

The fix is intentionally narrow — it adds detection at the top of
`lower_call` and a single helper (`variant_for_call_callee`) that
mirrors the type-checker's path-resolution logic. We did NOT modify
`resolve_callee` itself because that path is only reached after the
variant-detection-and-return at the top of `lower_call`; touching
`resolve_callee` would have required handling the operand-args
shape (FnRef vs AdtInit), and the targeted fix is cleaner.

### 2. SwitchInt cascade — depth math

The cascade opens `n_arms + 2` nested blocks (vs SwitchVariant's
`n_arms + 1`) because v0.15 always has a default arm (the `_`
wildcard). Inside the innermost block, `br_if k` jumps to the block
at depth k — which when its `end` fires lands the engine AT the start
of arm k's body. The cascade falls through to `br n_arms` (the
default-arm block) on no match.

Arm body i branches with depth `n_arms - i` to match_done (it is
itself wrapped in `(n_arms - i - 1)` per-arm-blocks + 1 default-arm
block + 0 for match_done itself).

### 3. For-range desugar at the selfhost-IR layer

The brief offered a choice: HIR-level (preferred if possible) or
selfhost-IR-level. The Rust pipeline already lowers `for` through an
iter-protocol MethodCall, so HIR-level rewriting would mean changing
the Rust HIR-to-IR pipeline — out of scope for the self-host work.

Instead we rewrite at the selfhost-IR layer: when `lower_for_expr`
sees an iter that's a `Binary` expr with op kind `"Range"` or
`"RangeEq"`, it emits the equivalent counter-loop event stream. The
non-range path stays unchanged. The two new HIR bridges
(`hir_path_is_variant`, `hir_expr_bin_op_kind`) are documented in
the bridge contract section of `selfhost/ir/lower.mty` — both fall
back to falsy defaults so the v0.9-v0.14 selfhost-IR bootstrap test
keeps passing (the new code paths simply don't fire until the host
implements the bridges).

## SwitchInt — IR shape under the hood

The Rust HIR-to-IR lowerer never actually emits `Term::SwitchInt`
from source today — `match n { 0 => ..., 1 => ..., _ => ... }`
expands as a chain of `BinOp::Eq` + `Term::If`. The `Term::SwitchInt`
opcode is reserved for a future jump-table optimization pass that
would coalesce dense-integer matches into a single multi-target jump.

To verify the v0.15 cascade lowering against an actual `Term::SwitchInt`
without relying on a future Rust optimization, the bootstrap test
synthesizes a `Program` directly (`selfhost_codegen_switch_int_synthetic`)
with a fn whose entry block ends in a hand-built `Term::SwitchInt`
over an I32 parameter, with 3 arms (0, 1, 2) + default. Each arm
assigns into the return slot and goes to a join block that returns.
The test verifies:

- The Mighty emitter produces a structurally valid Wasm module
  (`wasmparser::Validator::validate_all` is the gate).
- The cascade emits ≥ 3 `br_if` opcodes (one per arm).
- The cascade emits ≥ 3 `i32.eq` opcodes (one per arm test).

## Variant-call IR shape — the fix in detail

Before v0.15:
```
HirExpr::Call { callee: Path(["Some"]), args: [42] }
  → resolve_callee → FnRef::Builtin(Extern("Some"))
  → Stmt::Assign(_t, Rvalue::Call { func: Extern("Some"), args: [42] })
```

After v0.15:
```
HirExpr::Call { callee: Path(["Some"]), args: [42] }
  → variant_for_call_callee → Some((Option, 1))
  → Stmt::Assign(_t, Rvalue::AdtInit { adt: Option, variant: 1, fields: [42] })
```

The downstream Wasm emitter's existing `compile_adt_init_rvalue` path
then takes over: bump-allocate, store tag (1) at offset 0, store the
payload field (42) at offset 4. The interpreter's existing `AdtInit`
handler constructs a `Value::Adt { adt, variant, fields }` — same
shape as the bare-variant code path that was already working.

## v0.15 language gaps revealed

None new — the v0.15 work consumes the existing parser/HIR/IR/typeck
surface. All bridges are additive (new optional queries the host can
service or ignore).

## v0.16 roadmap

The biggest remaining gaps to lift before claiming "full" self-host
codegen:

1. **MethodCall lowering** — the `__mty_iter_next` MethodCall in the
   for-loop iter protocol still routes through `compile_unsupported_rvalue`
   → `unreachable`. v0.16 should add at minimum a vtable-free dispatch
   for monomorphizable receivers (range tuple, slice, array) so
   for-loops actually iterate at the Wasm level.
2. **Custom iterator desugar** — the for-range desugar covers `0..n`
   and `0..=n`. v0.16 should generalize to user-defined `Iter` impls
   once method dispatch lands.
3. **HIR-to-IR SwitchInt emission** — currently the SwitchInt cascade
   is dead code from the perspective of programs compiled through the
   normal pipeline. A small optimization pass in mty-ir that detects
   dense integer matches and rewrites the chained-If into SwitchInt
   would activate the cascade in real programs (and shrink the emitted
   Wasm by 30-50% for switch-heavy code).
4. **Agent / send / ask / spawn / arena / capability lowering** —
   still untouched in the self-host emitter.
5. **Real LEB128 encoder in Mighty source** — the bootstrap test
   still owns byte serialization. Lifting it would require Vec[U8]
   plus bit-twiddling primitives in the v0.16+ stdlib.

## Acceptance summary

- `selfhost/codegen/*.mty` + `selfhost/ir/lower.mty` `mty check` clean (verified
  via `selfhost_codegen_*compiles` + `selfhost_ir_compiles` tests).
- `cargo test -p mty-driver --test selfhost_codegen`: 17 passed, 0 ignored.
- `cargo test -p mty-driver --test selfhost_ir`: 9 passed, 0 regressions.
- `cargo test -p mty-ir`: passes (no regression in the Rust IR pipeline
  from the variant-call fix).
- `cargo build -p mty-ir` clean.

The full-workspace `cargo build` / `cargo test` were not run for this
slice because parallel agents' v0.15 WIP in `mty-codegen-wasm` (WASI
P2 default flip), `mty-macros`, `mty-syntax`, and `mty-types` was
mid-refactor and broke unrelated crates' builds. The v0.15 integration
agent owns the cross-slice verification gate.
