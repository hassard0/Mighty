# Internals — Wasm backend (slice 8)

`sdust-codegen-wasm` translates SIR into a core Wasm module via
`wasm-encoder`. Two targets: `wasm32-wasi` (server-side, WASI
preview1) and `wasm32-web` (browser host).

The Component Model wrapper (`wit-component`) is deferred to v0.2
(see [A47](../spec/v0.1-amendments.md#a47)). Slice 8 emits core
modules with capability imports declared under the `stardust`
module namespace.

## Module shape

```
(module
  (type ...)              ; deduped fn signatures
  (import "stardust" "log" (func (param i32 i32)))
  (func ...)              ; user fn declarations
  (memory 16)             ; 16-page initial linear memory
  (export "memory" (memory 0))
  (export "main" (func N))
  (code ...)              ; fn bodies
  (data (i32.const 1024) "...")  ; string literal pool
)
```

## Linear memory layout

| Range | Use |
|-------|-----|
| `0..1024` | reserved for shadow stack (v0.2 use) |
| `1024..` | string literal pool; grown as needed |

The shadow stack is unused in slice 8 because aggregate values are
not yet lowered. v0.2 will add a shadow-stack-pointer global and
spill/load slots for ADTs / strings that exceed register capacity.

## Type lowering

| SIR type | Wasm type |
|----------|-----------|
| `Bool`, `Char`, `Int(i32 family)`, `IntInfer` | `i32` |
| `Int(i64 family)`, `Duration`, `Size`, `USize` | `i64` |
| `Float(F32)` | `f32` |
| `Float(F64)`, `FloatInfer` | `f64` |
| `Unit`, `Never` | omitted |
| `I128` / `U128` | unsupported (slice-8) |
| `Str` / `String` / `Bytes` | `(i32 ptr, i32 len)` — caller-known pair |
| aggregates | unsupported (slice-8) |

## Capability imports

Each capability surface the user touches becomes an import. Slice 8
covers `log`:

```wat
(import "stardust" "log" (func (param i32 ptr) (param i32 len)))
```

WASI bridge: the runtime's WASI host receives the call and routes to
`fd_write(STDERR_FILENO, ...)`. The web target uses a JS shim:

```js
const imports = {
  stardust: {
    log(ptr, len) {
      const view = new Uint8Array(instance.exports.memory.buffer, ptr, len);
      console.log(new TextDecoder().decode(view));
    }
  }
};
```

## Eventual WIT shape (v0.2)

The component-model wrapper Stardust will emit looks roughly like:

```wit
package stardust:host;

interface log {
  log: func(msg: string);
}

interface fs {
  read: func(path: string) -> result<list<u8>, fs-error>;
  write: func(path: string, data: list<u8>) -> result<_, fs-error>;
}

interface net {
  get: func(url: string) -> result<list<u8>, net-error>;
}

world stardust-agent {
  import log;
  import fs;
  import net;
  export run: func();
}
```

Slice 8 ships the imports as plain core-module imports; v0.2 will
wrap them in this WIT vocabulary via `wit-component::ComponentEncoder`.

## Conservative lowering

Like the Cranelift backend, the Wasm backend raises
`WasmError::Unsupported(reason)` on shapes it can't handle. Unlike
native, there is **no interpreter fallback** for Wasm — there's no
Wasm interpreter to fall back to. Slice-8-covered surface for
`wasm32-wasi`:

- integer / bool arithmetic and comparisons (`i32`-flavored)
- locals (`local.get` / `local.set`)
- `log("...")` via the import
- straight-line control flow (`block` / `return`)

Out-of-scope:

- non-linear control flow (loops, multi-target br_table)
- aggregate construction / projection
- agent / spawn / send / ask (no wasm runtime in slice 8)
- effect dispatch beyond `log`

## Validation

Every emitted module is round-tripped through `wasmparser::Validator`
in `WasmArtifact::validate()`. The conformance tests
(`tests/conformance/codegen/wasm_*`) all validate before signaling
success.

## File output

```rust
let art = compile_program_to_file(&prog, WasmTarget::Wasi, &out_path)?;
println!("wrote {}", art.path.unwrap().display());
```

The `WasmArtifact` also returns the bytes in-memory so embedders
can avoid the disk round-trip.

## Running the output

```bash
sdust build --target wasm32-wasi examples/01_hello.sd
wasmtime target/01_hello.wasm
```

(slice 8 ships byte-only Wasm; the runtime's wasmtime integration is
a v0.2 task. For now the user runs the emitted module under their
own wasmtime/wasmer/JS host.)
