# 13 — Unsafe

Mighty has an explicit `unsafe` block for operations the compiler
cannot prove safe — raw pointer dereferences, transmutes, FFI, and
the like. Unsafe is lexically scoped, audit-tracked, and reported in
package metadata.

## The program

```mty
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

- `unsafe { ... }` is the only place raw pointer reads are allowed.
  The block is the audit unit — tooling reports every one of them.
- `pub unsafe fn from_raw(...)` is a public unsafe function: callers
  themselves must be inside an `unsafe` block to use it, and they
  inherit the responsibility for the function's contracts.
- The `requires` clauses are part of the function signature. Today
  they are documentation only; `const` evaluation and verification
  tools that check them are RFC-006 (DEFER-V1.1).
- Public packages report their unsafe surface in sidecar metadata,
  so downstream consumers can audit the chain.

See [spec §21](../spec/v1.0-rc.md) for the unsafe rules in full.

## Try it

```bash
mty check examples/17_unsafe.mty
```

## Where to go from here

You have seen every primary feature of the language. The remaining
examples assemble these pieces into larger programs:

- [`examples/16_macro.mty`](https://github.com/hassard0/Mighty/blob/main/examples/16_macro.mty)
  — hygienic macros (spec §20.3).
- [`examples/18_sandbox.mty`](https://github.com/hassard0/Mighty/blob/main/examples/18_sandbox.mty)
  — the long-form sandbox with capability lists.
- [`examples/19_backend_service.mty`](https://github.com/hassard0/Mighty/blob/main/examples/19_backend_service.mty)
  — a complete backend service with agents and arenas.
- [`examples/20_frontend_component.mty`](https://github.com/hassard0/Mighty/blob/main/examples/20_frontend_component.mty)
  — a counter component running in a browser via Wasm.

For the normative description of every construct, read the
[language specification](../spec/v1.0-rc.md).

## Next

Continue to [14 — Ownership](14-ownership.md).
