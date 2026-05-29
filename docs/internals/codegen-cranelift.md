# Internals — Cranelift native backend (slice 8)

`mty-codegen-cranelift` translates the slice-6 MtyIR into Cranelift
IR and produces either a JIT'd in-process fn pointer (`mty run`)
or a host-format object file linked into an executable
(`mty build --target native`).

It is the slice-8 default native backend — see
[A46](../spec/v0.1-amendments.md#a46) for why LLVM is feature-gated.

## Pipeline

```
.sd → AST → HIR → typed HIR → borrow-checked → MtyIR
                                                ↓
                                       Monomorphizer
                                                ↓
                                    mty-codegen-cranelift
                                       ┌────────┴────────┐
                                       │                 │
                                      JIT              Object
                                       │                 │
                                  fn-ptr               .o
                                       │                 │
                                    transmute         linker
                                       │                 │
                                     run             executable
```

## Crate layout

| Module | Responsibility |
|--------|----------------|
| `lib.rs` | re-exports |
| `error.rs` | `CodegenError`, `CompileResult` |
| `artifact.rs` | `NativeArtifact`, `BuildMode` |
| `abi.rs` | call-conv selection, MtyIR-type → cranelift type |
| `layout.rs` | size / align / field-offset for ADTs |
| `runtime_imports.rs` | C-ABI symbols the runtime exposes |
| `mono.rs` | generic-fn monomorphization (slice-8: strip-and-defer) |
| `lower.rs` | per-fn IR builder |
| `jit.rs` | `cranelift-jit` driver |
| `object.rs` | `cranelift-object` driver + linker invocation |

## Type lowering

| MtyIR type | Cranelift type | Notes |
|----------|----------------|-------|
| `Bool` | `I8` | |
| `Int(I8/U8)` | `I8` | |
| `Int(I16/U16)` | `I16` | |
| `Int(I32/U32/IntInfer)` | `I32` | unsuffixed defaults to i32 |
| `Int(I64/U64/ISize/USize)` | `I64` | 64-bit host only in slice 8 |
| `Int(I128/U128)` | `I128` | |
| `Float(F32)` | `F32` | |
| `Float(F64/FloatInfer)` | `F64` | |
| `Char` | `I32` | UCS-4 codepoint |
| `Duration` / `Size` | `I64` | nanoseconds / bytes |
| `Str` / `String` / `Bytes` | `I64` (ptr) | passed by ptr in slice 8; (len) lives in caller-known register |
| `Ref<T>` / `RawPtr<T>` | `I64` | host pointer |
| `Cap` | `I64` | opaque runtime handle |
| `Dyn` | `I64` | only ptr; vtable elided in slice 8 |
| `Unit` / `Never` | (omitted) | |

Aggregates (`Tuple` / `Array` / `Adt`) flatten to a pointer in the
slice-8 calling convention — caller allocates a stack slot, passes
its address.

## Calling convention (A52)

- SystemV on linux / macOS / non-Windows.
- WindowsFastcall on Windows.
- `extern c` fns: same ABI as host (cranelift handles).
- JIT: cranelift requires `is_pic = false`; object mode uses
  `is_pic = true` for relocatability.

## Runtime bridge (12 imports)

The JIT'd code calls back into the runtime via twelve C-ABI
symbols (see `runtime_imports::RUNTIME_IMPORTS`):

| Symbol | Signature | Purpose |
|--------|-----------|---------|
| `mty_runtime_log` | `(ptr, len)` | `log("...")` |
| `mty_runtime_print` | `(ptr, len)` | `print("...")` |
| `mty_runtime_panic` | `(ptr, len)` | trap with message |
| `mty_runtime_arena_push` | `() -> handle` | open arena frame |
| `mty_runtime_arena_pop` | `(handle)` | close arena frame |
| `mty_runtime_alloc` | `(size, align, zero) -> ptr` | bump-allocate in top arena |
| `mty_runtime_budget_charge` | `(bytes) -> ok?` | charge against budget |
| `mty_runtime_send` | `(target, msg, payload)` | fire-and-forget |
| `mty_runtime_ask` | `(target, msg, payload, deadline_ms) -> reply` | sync request |
| `mty_runtime_spawn` | `(agent_id) -> handle` | start an agent |
| `mty_runtime_extern_call` | `(name_ptr, name_len, args)` | call libc fn |
| `mty_runtime_log_i64` | `(value)` | debug `log` for ints |

The runtime registers these via `JITBuilder::symbol(name, addr)` at
finalize time. AOT mode declares them as imports and lets the host
linker resolve against `libsdust_runtime.so` / `.dylib` / `.lib`
(v0.2 work; slice 8 ships JIT only for the runtime bridge).

## Conservative-by-default lowering

The `FnLower` raises `CodegenError::Unsupported(reason)` when a MtyIR
shape can't be lowered. The driver catches Unsupported in `mty run`
and falls back to the interpreter, so the user sees correct behaviour
transparently. v0.2 closed the major MtyIR-coverage gaps:

- integer / bool / float / char arithmetic & comparisons (incl. float-int
  bitcast for MtyIR-typeck mismatches)
- `let` / `=` / `if` / `goto` / `return` / `unreachable` / `panic`
- direct fn-to-fn calls (with unit-arg filtering + per-param coercion)
- `log` / `print` / `panic` via the runtime ABI bridge
- string constants via the literal pool
- **ADT construct + destructure** (struct + enum literals; field reads;
  variant-field reads; switch-variant lowered as load-tag + brif chain)
- **`?` propagation** — `Term::TryReturnErr` materialises `Result::Err`
  in a fresh stack slot and returns its address
- **Tuple init + read**
- **Agent `send`/`ask`/`spawn`** route through runtime stubs
  (compiled handlers are a v0.2.x follow-up; the runtime still drives
  dispatch through the interpreter for now)
- **`EffectInvoke`** routes through `extern_call` stub
- **`MethodCall` / `IndexRead`** lower to best-effort runtime calls
- **Result constructor extern fns** (`Ok` / `Err`) emit ADT init

Out-of-scope (still interp fallback):

- `dyn Trait` vtables
- closure capture (lambdas-with-environment)
- inline cap dispatch (cap method calls)
- arbitrary `extern { fn }` resolution (requires shared-lib symbol
  resolution beyond the slice-8 stub)

## Aggregate ABI

| Shape | Cranelift representation |
|-------|--------------------------|
| Bool / Int(small) / Char / Float | passed by value |
| Aggregate (struct/enum/tuple/array) | passed by pointer (i64 address) |
| `Str`/`String`/`Bytes` | `(ptr, len)` pair, passed as pointer |
| Reference / RawPtr | i64 pointer |

The function signature is built from the MtyIR types; the lowerer adds
stack slots lazily when a MtyIR local is first written to via
`AdtInit`/`TupleInit`. Parameters that are aggregate-typed keep the
caller's pointer untouched.

Enum layout: `[u32 tag][padding to max-payload-align][payload bytes]`,
matching the `layout` module's natural-alignment scheme. Struct layout:
same minus the tag. See [`aggregate.rs`](https://github.com/hassard0/Mighty/blob/main/crates/mty-codegen-cranelift/src/aggregate.rs).

## JIT driver

```rust
let prog = sdust_sir::lower_package(&pkg, &typed);
let mono = sdust_codegen_cranelift::Monomorphizer::new(&prog).run();
let syms = sdust_runtime::codegen_abi::symbol_table();
let syms = symbols_from(...);
let jc = build_jit(&mono, &syms)?;
jc.call_main(); // transmute fn-ptr; respects Unit-vs-Int return shape
```

## Object + linker

```rust
let obj = compile_object(&prog, &obj_path)?;
let exe = link_executable(&obj, &exe_path, BuildMode::Release)?;
```

Linker discovery (A52, extended in v0.2):
1. `$MTY_LINKER` if set, otherwise `$STARDUST_LINKER` (legacy spelling,
   one-shot deprecation warning).
2. **Windows**: `clang.exe`, `clang`, `gcc.exe`, `gcc`, `cc.exe`, then
   `lld-link.exe`, `lld-link`. (We deliberately do *not* probe bare
   `link` on Windows because the MSYS coreutils `/usr/bin/link.exe`
   shim shadows MSVC's real linker.)
3. **Unix**: `cc`, `gcc`, `clang`, then `ld.lld`, `lld`. `clang` is
   preferred because it can drive both GNU and macOS link lines via
   the same frontend.
4. The MSYS/Git-Bash `/usr/bin/link.exe` (hardlink helper) is actively
   skipped if found in `PATH`.

If none found, `compile_object` succeeds but `link_executable` is
skipped and the caller is told to set `$MTY_LINKER`.

### Why `clang` first

`clang` (and `lld` underneath it) speaks every host's linker line:
on Linux it drives `ld.bfd`/`ld.gold`/`ld.lld` directly; on macOS it
hands off to the system `ld64`; on Windows it embeds `lld-link` which
honours MSVC arg syntax. That makes the cranelift backend's link
step a single code path across hosts.

## Monomorphization (A49)

Slice-8 MVP: `Monomorphizer::run()` clones the program and **strips**
generic fns (any fn whose params or return use `SirTy::Param`). The
resulting program contains only fully-concrete fns. Programs that
exercise generics fall through to the interpreter via the slice-8
Unsupported path.

Real per-(fn, type-args) specialization is v0.2 work. The slice-8
choice is deliberate: it ships a working compiler for monomorphic
programs without blocking on the generic-substitution machinery,
which interacts subtly with effect rows and trait dispatch.

## Future work

- Full ADT lowering (struct construct/project, enum tag-and-payload,
  array elem load/store)
- `?` propagation lowering (compose a Result over the next block)
- Method/trait dispatch via vtables
- Agent dispatch through compiled handler fn-ptrs (replaces the
  slice-7-via-interpreter per-turn callback)
- DWARF debug info (cranelift's stub support → useful frames)
- ThinLTO-style cross-fn optimization (v0.2 LLVM)
