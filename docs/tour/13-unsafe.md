# 13 — Unsafe

Stardust has an explicit `unsafe` block for operations the compiler
cannot prove safe — raw pointer dereferences, transmutes, FFI, and the
like. Unsafe is lexically scoped, audit-tracked, and reported in package
metadata.

## The program

```sd
fn read_byte(addr: USize) -> U8 {
  unsafe {
    let p = raw_ptr(addr)
    p.read()
  }
}

pub unsafe fn from_raw(ptr: *U8, len: USize) -> Bytes
  requires ptr != null
  requires valid(ptr, len)
```

## What is interesting

- `unsafe { ... }` is the only place raw pointer reads are allowed. The
  block is the audit unit — tooling reports every one of them.
- `pub unsafe fn from_raw(...)` is a public unsafe function: callers
  themselves must be inside an `unsafe` block to use it, and they
  inherit the responsibility for the function's contracts.
- The `requires` clauses are part of the function signature. In v0.1
  they are documentation only; later slices will let `const` evaluation
  and verification tools check them.
- Public packages report their unsafe surface in sidecar metadata, so
  downstream consumers can audit the chain.

See [spec §21](../spec/v0.1.md) for the unsafe rules in full.

## Run it

```bash
sdust check examples/17_unsafe.sd
```

## Where to go from here

You have seen every primary feature of the language. The remaining
examples assemble these pieces into larger programs:

- [`examples/16_macro.sd`](../../examples/16_macro.sd) — hygienic
  macros (spec §20.3).
- [`examples/18_sandbox.sd`](../../examples/18_sandbox.sd) — the
  long-form sandbox with capability lists.
- [`examples/19_backend_service.sd`](../../examples/19_backend_service.sd)
  — a complete backend service with agents and arenas.
- [`examples/20_frontend_component.sd`](../../examples/20_frontend_component.sd)
  — a counter component running in a browser via Wasm.

For the normative description of every construct, read the
[language specification](../spec/v0.1.md).
