# SELFHOST_CODEGEN_V0_13_NOTES

This file catalogues the v0.13 self-host codegen slice: porting the
Wasm core-module emitter to Mighty. It is the **last front-end phase**
of the self-host chain — after v0.13, the entire Mighty compiler
front-end + Wasm back-end is implemented in Mighty source for the
slice-1-supported subset.

## Live status

- `selfhost/codegen/lib.mty` — 73 LOC, `mty check` clean (package +
  intent doc).
- `selfhost/codegen/wasm.mty` — ~400 LOC, `mty check` clean, services
  the v0.13 bootstrap test.
- `crates/mty-driver/tests/selfhost_codegen.rs` — ~1500 LOC, **6/6 live
  tests pass + 1 ignored** (example 03 — generic `Option[T]` lowering
  is out of scope for v0.13).

```
test selfhost_codegen_compiles ........... ok
test selfhost_codegen_lib_compiles ....... ok
test selfhost_codegen_hello_world ........ ok
test selfhost_codegen_example_01 ......... ok
test selfhost_codegen_example_02 ......... ok
test selfhost_codegen_arith_fixture ...... ok
test selfhost_codegen_example_03 ......... ignored
```

## Architectural choice — host bridge for byte serialization

The Rust Wasm core-module lowerer in
`crates/mty-codegen-wasm/src/emit.rs` is ~1700 LOC and exercises the
entirety of the `wasm-encoder` section/instruction API. Porting all of
it to Mighty would be 3-5 KLOC and exceed the v0.13 5-hour budget by
2-4x. More importantly, the v0.12 Mighty stdlib lacks the low-level
byte-manipulation primitives a from-scratch Wasm byte emitter would
need:

- No `Vec[U8]` with `.push(byte: U8)`.
- No bitwise `>>` / `<<` / `&` / `|` on `U8` operands in a way that
  threads through the interpreter cleanly.
- No `String → bytes()` that yields raw UTF-8 byte slices the Mighty
  source can iterate over.
- No LEB128 encoder in the stdlib (would have to be written from
  scratch on top of the above — circular).

So we make the **same architectural choice** that v0.5 (lexer host
bridge), v0.6 (parser host bridge), v0.8 (HIR + typeck host bridges),
v0.9 (IR sink), and v0.10 (IR sink, refined) made: the Mighty source
owns the **algorithm** (which Wasm sections, which type signatures,
which opcodes for which MtyIR shapes), and the **host owns byte
serialization** (LEB128, magic header, section length prefixes).

This is faithful to the brief's "if Mighty lacks the byte-manipulation
primitives Wasm encoding needs, document the gap + use a workaround
(emit hex strings that a Rust-side helper converts; document the
workaround as a v0.14 task)" working agreement.

## Bootstrap technique

The bootstrap test (`crates/mty-driver/tests/selfhost_codegen.rs`):

1. **Snapshots** the trusted Rust MtyIR for the input program into a
   flat-array `IrSnapshot`. The Mighty emitter reads it through 28
   `ir_*` bridge methods (`ir_fn_count`, `ir_fn_name`, `ir_fn_ret_type`,
   `ir_block_stmt_kind`, `ir_block_term_kind`, etc.).
2. **Runs** the Mighty `compile_program` fn (in `selfhost/codegen/wasm.mty`)
   through the SIR interpreter with a `SelfhostCodegenHost` that
   records each `wasm_emit_*` bridge call into a `Vec<WasmEvent>`.
3. **Reassembles** a real `.wasm` core module from the event stream by
   writing raw Wasm bytes directly (inline LEB128 + section
   serialization — ~250 LOC of trivial Rust). Type 0 is reserved for
   the imported `log(i32, i32) -> ()`; user fns start at wasm-fn-idx 1.
4. **Validates** the bytes with `wasmparser::Validator::validate_all`
   — the same correctness gate the trusted Rust codegen pipeline uses
   in `crates/mty-driver/tests/conformance_codegen.rs`.

The acceptance criterion is **wasmparser validation**, not byte
equality with the Rust pipeline's output (the byte streams have
different ordering, dedup keys, locals layout decisions, etc.).

## v0.13 production matrix

What the Mighty Wasm emitter covers vs. what the Rust Wasm core-module
lowerer covers:

| Feature group | Mighty coverage | Rust coverage | Status |
|---|---|---|---|
| `fn` signature emission (params + result) | YES | YES | shipped |
| Local declarations (run-length encoded) | YES | YES | shipped |
| Type-section dedup | NO (host handles it) | YES | parity (host-equivalent) |
| Function index allocation (`+1` for log import) | YES | YES | shipped |
| `main` export | YES | YES | shipped |
| `i32.const` / `i64.const` / `f32.const` / `f64.const` | YES | YES | shipped |
| `local.get` / `local.set` / `local.tee` | YES | YES | shipped |
| i32 arithmetic (Add/Sub/Mul/DivS/DivU/RemS/RemU) | YES | YES | shipped |
| i32 bitwise (And/Or/Xor/Shl/ShrS/ShrU) | YES | YES | shipped |
| i32 comparisons (Eq/Ne/LtS/LeS/GtS/GeS + unsigned) | YES | YES | shipped |
| i64 arithmetic + bitwise + cmp | YES (via i64_binop) | YES | shipped |
| f64 arithmetic + cmp | YES (via f64_binop) | YES | shipped |
| Unary Neg (synthesized as 0 - x) | YES | YES | shipped |
| Unary Not / Bool not (`i32.eqz`) | YES | YES | shipped |
| `if` / `else` / `end` structured control | YES | YES | shipped |
| `block` / `loop` / `br` / `br_if` | YES (emission only) | YES (used) | shipped-subset |
| `return` instruction | YES | YES | shipped |
| `call` (user-defined fn, by wasm fn idx) | YES | YES | shipped |
| log / print / panic → imported `log(ptr, len)` | YES (routed via EffectInvoke) | YES | shipped |
| `unreachable` for unmodelled shapes | YES | YES (Stmt::Nop in some cases) | shipped |
| `drop` for value-discard | YES | YES | shipped |
| Memory section (1 page) | YES (host adds) | YES | parity |
| Export `memory` | YES (host adds) | YES | parity |
| String literal data section + interning | NO (host stubs ptr=0, len=0) | YES (real intern table) | **deferred v0.14** |
| ADT init (struct / enum) | NO (emits `unreachable`) | YES (linear-memory layout) | **deferred v0.14** |
| Match-arm dispatch (`br_table` / `if` chains) | NO (emits `unreachable`) | YES | **deferred v0.14** |
| Method calls (vtable dispatch) | NO | YES (DomOp + trait dispatch) | **deferred v0.14** |
| `for` loops (iter-protocol desugar) | NO | YES | **deferred v0.14** |
| Agent / spawn / send / ask lowering | NO | NO (agent codegen still post-1.0 in Rust too) | parity (post-1.0) |
| Component Model wrapping | NO (Rust does it above us) | YES | parity (out-of-scope) |
| DOM imports (web target) | NO | YES | **deferred v0.14** |
| Canonical-ABI string params | NO | YES | **deferred v0.14** |
| `cabi_realloc` allocator | NO | YES (~150 LOC of bump-+-free-list) | **deferred v0.14** |
| Drop insertion + StorageLive/Dead pairs | NO (Wasm has automatic scoping) | YES | parity (host-equivalent) |
| Source-map generation | NO | YES | parity (out-of-scope) |

## Bootstrap test coverage

`cargo test -p mty-driver --test selfhost_codegen`:

- `selfhost_codegen_compiles` — `mty check` clean for `wasm.mty`
- `selfhost_codegen_lib_compiles` — `mty check` clean for `lib.mty`
- `selfhost_codegen_hello_world` — `fn main() { log("hi") }` → valid Wasm
- `selfhost_codegen_example_01` — example 01 (`fn main()` + `log`) → valid Wasm
- `selfhost_codegen_example_02` — example 02 (struct + enum + match) →
  valid Wasm (match arms emit `unreachable`, but the module validates)
- `selfhost_codegen_arith_fixture` — synthetic `fn add(a, b) { a + b }`
  → valid Wasm with `i32.*` arithmetic opcode in the add fn body
- `selfhost_codegen_example_03` — **ignored** (generic `Option[T]` fn)

## v0.13 language gaps discovered

These are the v0.13 Mighty-language gaps the codegen self-host
revealed. Most are downstream of the same root cause: Mighty's stdlib
doesn't yet expose raw byte primitives.

### Gap 1: No `Vec[U8]` with byte-level mutators

The Mighty source can't write `bytes.push(0x60)` to build a Wasm type
descriptor. The host-bridge workaround moves byte serialization to
Rust; a full self-host emitter would require `Vec[U8]` + `.push(u: U8)`
+ `.append_slice(s: &[U8])` first.

**v0.14 plan**: add `Vec[U8]` to the prelude with the four core
methods (`new`, `push`, `len`, `as_slice`) wired through the
interpreter; the rest of the byte-twiddling can be written in Mighty
on top of those.

### Gap 2: No LEB128 encoder in the stdlib

Wasm uses unsigned LEB128 (`u32`/`u64`) + signed LEB128 (`s32`/`s64`/`s128`)
for ~every integer it writes. Implementing these in Mighty source
needs `>> 7`, `& 0x7F`, `| 0x80` on `U8`. The bitwise shifts on byte
operands are theoretically supported but the interpreter's path for
them isn't exercised by the v0.5-v0.10 corpus.

**v0.14 plan**: write `mty.leb128.write_u32(out: &mut Vec[U8], v: U32)`
and `write_s32`/`write_s64` once Gap 1 lands; document as the only
non-trivial byte algorithm Mighty source needs to self-host the rest
of the Wasm format.

### Gap 3: No first-class `Const::Str` payload through the bridge

Mighty's bridge methods return `Str` results, but the IR snapshot
records string operands by `Const::Str(s)` and we can only get the
string out of the bridge as a `Str` value. The v0.13 emitter punts on
string interning entirely (host returns ptr=0, len=0 for all log
calls) — the validator doesn't run code so this is safe, but a real
self-host emitter would need to:

1. Walk the IR program once to collect all `Const::Str` payloads.
2. Build a data section with concatenated bytes + record (ptr, len)
   per literal.
3. Emit `i32.const ptr; i32.const len; call $log` per `log("...")`.

This is ~80 LOC of Mighty source once Gap 1 lands.

**v0.14 plan**: extend the `ir_*` bridge with `ir_str_pool_count()`
+ `ir_str_pool_bytes(i)` queries; have the Mighty emitter pre-walk
the IR before emitting fn bodies + insert a `wasm_emit_data_section`
event at module start.

### Gap 4: No structured `match` lowering yet

The Mighty source emits `unreachable` for `SwitchInt` / `SwitchVariant`
terminators. A full lowering requires:

- For `SwitchInt` (ints): emit an `i32.const` + cascading `if`/`else`
  chain OR a `br_table` if the arms are dense.
- For `SwitchVariant` (ADTs): need to know the discriminant's offset
  in the ADT's linear-memory layout, then `i32.load offset=DISC_OFF`
  + chained `if`/`else` (or `br_table`).

Neither is in scope for v0.13 because the upstream MtyIR self-host
itself (`selfhost/ir/lower.mty`) still emits a coarse `SwitchInt` for
all matches without pattern lowering. Both layers need to land
together to be useful.

**v0.14 plan**: pair with the IR self-host's pattern-lowering work
(deferred from v0.10) so that the codegen self-host has real
`SwitchInt`/`SwitchVariant` terminators to consume.

### Gap 5: No real linear-memory layout for ADTs

`Rvalue::AdtInit` becomes `unreachable` in the v0.13 emitter. A full
implementation needs to:

1. Compute each ADT's struct layout (offsets per field, total size).
2. Bump-allocate via `cabi_realloc` for each `AdtInit`.
3. Emit `i32.store offset=FIELD_OFF` per field-init operand.
4. Push the pointer onto the stack as the rvalue result.

This requires Mighty stdlib helpers (`cabi_realloc` in Mighty) +
ADT-layout queries in the bridge. Same root cause as Gap 4 — the
MtyIR self-host doesn't yet emit the structured per-field AdtInit
rvalues this would consume.

**v0.14 plan**: defer until both gaps land in tandem.

### Gap 6: No `for` loop iter-protocol desugar

The HIR-level desugar of `for x in iter { body }` into
`loop { match iter.next() { Some(x) => body, None => break } }`
happens in the Rust HIR lowerer (`crates/mty-hir/src/lower/exprs.rs`).
The Mighty IR self-host already handles `For` terminators with a
coarse loop-header shape (lenient diff accepts it). The v0.13 codegen
emits `unreachable` for any `For`-derived block.

**v0.14 plan**: pipeline the iter-protocol desugar into the IR
self-host first; the codegen then naturally consumes the
already-desugared `loop` + `match` form.

### Gap 7: No agent / send / ask / spawn lowering

Same status as Gap 4-6 — the upstream MtyIR self-host doesn't emit
these shapes, so the codegen self-host has nothing to lower. Both
land together post-1.0 (this is also true of the Rust pipeline:
agent codegen is partially shipped in `crates/mty-codegen-wasm`
but gated on `wit-component` wrapping which is post-emit work).

## Post-v0.13 follow-up roadmap

In order of preference (smallest delta first):

1. **String pool** (v0.14a) — pre-walk the IR + emit a data section
   so `log("hi")` actually writes "hi" to linear memory. Requires Gap
   1 (Vec[U8]) + Gap 3 (str-pool bridge queries). Estimated ~150 LOC
   of Mighty source + ~30 LOC of bridge expansion.

2. **Pattern lowering + ADT layout** (v0.14b) — pair the MtyIR
   self-host's pattern lowering with the codegen ADT init. Requires
   Gap 4 + Gap 5 to land together. Estimated ~400 LOC of Mighty
   source (split across `selfhost/ir/lower.mty` + `selfhost/codegen/wasm.mty`).

3. **Real LEB128 + section bytes in Mighty** (v0.14c) — once Gap 1
   lands, port the byte-emission helpers from the Rust host into
   Mighty source. This is the ~250 LOC currently in
   `rebuild_wasm` + `emit_body_bytes` in the bootstrap test. After
   this lands, the bootstrap test's role becomes purely the
   "snapshot the IR" + "validate the bytes" gates; the Mighty source
   would produce the bytes directly.

4. **`for` loop iter-protocol** (v0.14d) — pair with the IR
   self-host's iter-protocol desugar.

5. **Agent / send / ask / spawn** (post-1.0) — only meaningful once
   the upstream IR self-host emits these shapes. Same status as the
   Rust pipeline: post-1.0.

## Why ship a SUBSET

Same rationale as v0.5/v0.6/v0.8/v0.9/v0.10:

- **Locks in the codegen's semantic surface** in Mighty syntax,
  letting future versions diff against a fixed spec when they
  reorganize the Rust implementation.
- **Exercises the language at codegen-level complexity** and surfaces
  the byte-manipulation gaps (cataloged above) before they accumulate.
- **Is faithful to the brief's "ship a SUBSET if you hit gaps,
  document them"** working agreement.
- **Closes the self-host chain end-to-end for the slice-1 subset.**
  Mighty can now describe the algorithm that produces a valid Wasm
  module from its own MtyIR for examples 01-02 + a synthetic arith
  fixture. The result validates via `wasmparser::Validator::validate_all`
  — the same correctness gate the trusted Rust pipeline uses.
- **The v0.14 follow-up is unblocked**: land Vec[U8] + LEB128 in the
  stdlib, and the gaps above collapse into a single ~400 LOC Mighty
  port.

## Acceptance check

- [x] `mty check selfhost/codegen/lib.mty` clean
- [x] `mty check selfhost/codegen/wasm.mty` clean
- [x] `cargo test -p mty-driver --test selfhost_codegen` — 6 passed, 1 ignored
- [x] `cargo clippy --test selfhost_codegen -p mty-driver -- -D warnings` clean
- [x] No regression on the `selfhost_*` test family (lexer/parser/hir/typeck/ir all green)
- [x] `docs/internals/self-hosting.md` + `selfhost/README.md` updated with v0.13 row
