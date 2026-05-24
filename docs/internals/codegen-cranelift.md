# Internals — Cranelift native backend (slice 8)

`sdust-codegen-cranelift` translates the slice-6 SIR into Cranelift
IR and produces either a JIT'd in-process fn pointer (`sdust run`)
or a host-format object file linked into an executable
(`sdust build --target native`).

It is the slice-8 default native backend — see
[A46](../spec/v0.1-amendments.md#a46) for why LLVM is feature-gated.

## Pipeline

```
.sd → AST → HIR → typed HIR → borrow-checked → SIR
                                                ↓
                                       Monomorphizer
                                                ↓
                                    sdust-codegen-cranelift
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
| `abi.rs` | call-conv selection, SIR-type → cranelift type |
| `layout.rs` | size / align / field-offset for ADTs |
| `runtime_imports.rs` | C-ABI symbols the runtime exposes |
| `mono.rs` | generic-fn monomorphization (slice-8: strip-and-defer) |
| `lower.rs` | per-fn IR builder |
| `jit.rs` | `cranelift-jit` driver |
| `object.rs` | `cranelift-object` driver + linker invocation |

## Type lowering

| SIR type | Cranelift type | Notes |
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
| `stardust_runtime_log` | `(ptr, len)` | `log("...")` |
| `stardust_runtime_print` | `(ptr, len)` | `print("...")` |
| `stardust_runtime_panic` | `(ptr, len)` | trap with message |
| `stardust_runtime_arena_push` | `() -> handle` | open arena frame |
| `stardust_runtime_arena_pop` | `(handle)` | close arena frame |
| `stardust_runtime_alloc` | `(size, align, zero) -> ptr` | bump-allocate in top arena |
| `stardust_runtime_budget_charge` | `(bytes) -> ok?` | charge against budget |
| `stardust_runtime_send` | `(target, msg, payload)` | fire-and-forget |
| `stardust_runtime_ask` | `(target, msg, payload, deadline_ms) -> reply` | sync request |
| `stardust_runtime_spawn` | `(agent_id) -> handle` | start an agent |
| `stardust_runtime_extern_call` | `(name_ptr, name_len, args)` | call libc fn |
| `stardust_runtime_log_i64` | `(value)` | debug `log` for ints |

The runtime registers these via `JITBuilder::symbol(name, addr)` at
finalize time. AOT mode declares them as imports and lets the host
linker resolve against `libsdust_runtime.so` / `.dylib` / `.lib`
(v0.2 work; slice 8 ships JIT only for the runtime bridge).

## Conservative-by-default lowering

The `FnLower` consciously fails on shapes it can't handle, raising
`CodegenError::Unsupported(reason)`. The driver catches Unsupported
in `sdust run` and falls back to the interpreter, so the user sees
correct behaviour transparently — slower, but correct. Slice 8's
covered surface:

- integer / bool / float arithmetic & comparisons
- `let` / `=` / `if` / `goto` / `return` / `unreachable`
- direct fn-to-fn calls (no method dispatch, no traits)
- `log("...")` / `print("...")` (string literal arg)
- string constants via the literal pool

Out-of-scope (interp fallback):

- aggregate construction / projection (struct / tuple / enum / array)
- `?` propagation
- `match` over enums (only `if`-trees work)
- agent `spawn` / `send` / `ask`
- capabilities / `dyn` / effect calls

These all land as v0.2 backend coverage.

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

Linker discovery (A52):
1. `$STARDUST_LINKER` if set.
2. `clang` / `gcc` / `cc` (preferred — speak both GNU and MSVC arg
   syntax via the C frontends).
3. Skips `/usr/bin/link.exe` on MSYS (it's a coreutils shim).

If none found, `compile_object` succeeds but `link_executable` is
skipped and the caller is told to set `$STARDUST_LINKER`.

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
