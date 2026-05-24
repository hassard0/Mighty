# Mighty v0.2 — Codegen Completion Notes

This file tracks the choices made by the codegen swarm during the
v0.2 build. It complements `SLICE8.md` and the per-backend
`docs/internals/codegen-*.md` documents.

## Scope

1. Close the MtyIR-coverage gaps in `mty-codegen-cranelift` so all 20
   examples either build natively or fall back to the interpreter
   only for known unsupported features (extern resolution, dyn dispatch).
2. Replicate the same coverage in `mty-codegen-wasm`.
3. Stand up the LLVM backend (behind the `llvm` feature gate). The
   build host did **not** have LLVM 17 installed, so the LLVM path is
   shipped as opt-in code that compiles to "feature disabled" without
   LLVM available. Documentation in `docs/internals/codegen-llvm.md`
   covers the installation requirements.

## Memory layout choices

- Aggregates lower to stack-allocated buffers; the address is held in
  an i64 cranelift Variable. ABI is "by pointer" (caller allocates a
  stack slot, passes pointer).
- Enum representation: `[u32 tag][padding to max-payload-align][payload bytes]`.
  Slice-8 used a simpler "tag + max payload" layout in
  `crate::layout`; v0.2 reuses that layout module verbatim.
- Field access for structs walks (offset, size) at codegen time and
  emits `load.<size>` from `base + offset`.
- Field access for enum variants asserts the discriminant tag matches
  the expected variant (in debug builds) and loads from `payload_start
  + field_offset`. Slice-8 lowerer pairs `VariantField` projection with
  a preceding `SwitchVariant` terminator, so the tag check is a
  defensive `trap` after a verified branch.

## Calling convention for aggregates

Aggregates are passed by pointer. Return-by-aggregate uses a hidden
first-pointer parameter (SystemV sret pattern) — slice-8 ABI helpers
already emit this when `ret_ty` is non-scalar.

## `?` propagation

The MtyIR exposes `Term::TryReturnErr(payload)`. The cranelift lowerer
materialises the payload, builds a `Result::Err(payload)` enum value
in a temporary buffer, then returns that buffer's pointer. Wasm uses
the analogous shape with `i32` pointer + linear memory.

## Monomorphization

Slice-8 stripped generic fns entirely. v0.2 implements
per-(fn, type-args) specialization with name mangling `<fn>__<T1>_<T2>_..`.
The `Monomorphizer` walks `main` reachability, discovers generic
call sites, and emits a fresh `Function` per concrete type tuple. The
codegen backend treats each specialized fn as ordinary monomorphic.

Slice-8 did not propagate type-arg lists through MtyIR call sites
because the typeck didn't surface them. v0.2 ships a conservative
walker that picks up explicit `[T=…]` annotations from the typed-HIR
and falls back to leaving the call to interp dispatch when the type
arg list isn't recoverable. This keeps the simple `first[I32]` test
case working, and leaves more complex generic call patterns for
post-v0.2 work.

## Agent compiled handlers

Slice-7 runs agent handlers via the interpreter
(`run_handler_isolated`). v0.2 adds a *compiled* handler path: each
declared `on Msg(...)` lowers to a real fn with the manual signature
`(arena: *mut u8, payload: *mut u8) -> *mut u8`. The runtime's agent
dispatch checks a `Vec<Option<HandlerFn>>` first; missing entries
fall through to the interp path so partially-compiled agents still
work.

`stardust_runtime_register_handler` is the new C-ABI hook the codegen
calls at module-init time to populate the dispatch table.

## LLVM backend status (build host)

- `cargo build -p mty-codegen-llvm --features llvm` fails on this
  host with `No suitable version of LLVM was found system-wide`. This
  is the documented A46 condition.
- The crate's default features remain OFF. CI/dev hosts with LLVM 17
  installed can pass `--features llvm` to exercise the IR-building
  code paths.
- All LLVM-only tests are gated behind `#[cfg(feature = "llvm")]` so
  they skip cleanly when LLVM is absent.

## Cross-platform linker improvements (A52 extension)

`object::find_linker` now also probes `lld` (cross-platform) and
`lld-link`/`ld.lld`/`ld64.lld` per-platform. `STARDUST_LINKER` still
wins over auto-detection.

On Windows, the search order prefers `clang`/`clang.exe` (which
embed `lld`) before `link.exe`. The MSYS coreutils `/usr/bin/link`
shim is still actively skipped.

## Examples coverage matrix (post-v0.2)

| # | File | Native | Wasm | Notes |
|---|------|--------|------|-------|
| 01 | hello | yes | yes | baseline |
| 02 | struct_enum | yes | yes | ADT construct + match |
| 03 | generic_fn | yes | yes | monomorphization |
| 04 | result_propagation | yes | yes | `?` propagation |
| 05 | match_expr | yes | yes | pattern matching |
| 06 | for_while_loop | yes | yes | already worked (loops are If+Goto) |
| 07 | agent_echo | yes (compiled handler) | n/a | runtime drives dispatch |
| 08 | agent_state | yes (compiled handler) | n/a | |
| 09 | send_ask_deadline | yes (with runtime) | n/a | |
| 10 | supervisor | yes (with runtime) | n/a | |
| 11 | budget_block | yes | n/a | budget bridge already wired |
| 12 | arena | yes | yes | arena_push/pop already wired |
| 13 | capabilities | partial | n/a | cap dispatch still interp |
| 14 | extern_c | falls back to interp | n/a | needs real shared lib |
| 15 | extern_js | falls back to interp | n/a | requires JS host |
| 16 | macro | n/a | n/a | macros expand before codegen |
| 17 | unsafe | yes | partial | raw-ptr ops compile |
| 18 | sandbox | yes (with runtime) | n/a | |
| 19 | backend_service | partial | n/a | full HTTP needs effects |
| 20 | frontend_component | n/a | partial | UI runtime not in scope |

## Post-v0.2 follow-ups

- `dyn Trait` dispatch (vtables) — still raises Unsupported.
- Cap dispatch (effect-system handlers) compiled inline — still
  routes through interp.
- Closure capture lowering — slice-8 codegen treats `Operand::Move`
  of a fn-typed value as a fn-pointer; closures with environment
  remain interp-only.
- LLVM optimizer pass tuning (we ship default `PassBuilder::run` —
  custom pipelines TBD).
- Wasm component-model `wit` interface generation for ADT exports
  (slice-8 emits core modules; component-model surfaces is a v0.3
  candidate).

EOF
