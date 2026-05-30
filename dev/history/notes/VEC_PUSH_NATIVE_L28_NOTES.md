# Native growable `Vec` — L28 fix (v0.38)

## Symptom

Under native `mty build` (Cranelift), a `Vec` grown in a loop with the
`v = v.push(x)` capture-rebind idiom came back **empty** — the loop ran
the right number of iterations but `v.len()` was `0`. The same code is
correct under the interpreter (`mty run` / `mty test`).

Minimal repro (`mighty-ide/repro/repro.mty`, also mirrored as a JIT
regression test — see below):

```mty
fn main() {
  let mut v: Vec[I32] = Vec.new()
  let mut i: USize = 0
  while i < 5 { v = v.push(65); i = i + 1 }
  // v.len() observed 0, expected 5
}
```

This is the IDE blocker documented as **L28** in
`mighty-ide/docs/mighty-language-lessons.md` (shares a root cause with
**L21**: native Vec ops were all stubs, so any nested-loop Vec access
read garbage / could crash).

## Root cause

It was **not** a liveness / loop-back-edge bug as originally suspected.
The SIR for the repro shows the Vec locals are typed `{error}`
(`IrTy::Error`), so in the Cranelift backend they are plain scalar `i64`
Cranelift `Variable`s — `v = v.push(...)` already threaded its value
across the back-edge correctly via SSA. The real problem: **the native
backend had no Vec runtime at all**. Every Vec operation lowered to a
stub:

- `Vec.new()` → `Rvalue::Call { func: Builtin(Extern("Vec.new")), .. }`
  → routed through `mty_runtime_extern_call`, which returns `0`.
- `v.push(x)` / `v.len()` / `v.get(i)` →
  `Rvalue::MethodCall { method, .. }` → the slice-8 stub
  (`crates/mty-codegen-cranelift/src/lower.rs`, the `Rvalue::MethodCall`
  arm) which also routed through `mty_runtime_extern_call` → `0`.

So `v` was always the integer `0`, `push` was a no-op returning `0`, and
`len` always returned `0`. The loop ran (driven by the scalar counter
`i`) but nothing was stored.

## Fix

`crates/mty-codegen-cranelift/src/lower.rs` — implement a real native
growable `Vec` entirely in emitted code, backed by the existing
`mty_runtime_alloc` arena allocator (no new runtime symbol, so existing
runtime archives keep working).

A native `Vec[T]` value is an `i64` pointer to a 24-byte arena header:

```
off  0 : len  (i64)  element count
off  8 : cap  (i64)  capacity in elements
off 16 : data (i64)  pointer to cap*8 bytes of element storage
```

Each element occupies an 8-byte slot (losslessly holds every scalar
element type we currently codegen: U8/I32/USize/I64/bool/char). The
header pointer is **stable** across `push`, so the SIR
`v = v.push(x)` rebind keeps the same `i64` flowing through the loop.

New lowering helpers + dispatch:

- `emit_vec_new` — `Vec.new()` / `Vec.with_capacity(n)` allocate a
  zeroed header. Dispatched from the `lower_call` `Extern("Vec.new")`
  arm (added before the generic-extern fallthrough).
- `emit_vec_push` — ensure capacity (grow to `max(4, cap*2)` via a fresh
  arena alloc + `emit_memcpy_dynamic` of the live prefix when
  `len == cap`), store the element at `data[len]`, bump `len`, return
  the same header pointer.
- `emit_vec_len` / `emit_vec_get` / `emit_vec_pop` / `emit_vec_clear`.
- `Rvalue::IndexRead` is now Vec-aware: for a non-aggregate receiver
  (the Vec header) it loads `data` from `header+16` first; for a true
  `IrTy::Array` aggregate it uses the slot address directly (unchanged
  behavior).

Growth re-allocates and leaks the old buffer into the arena (freed when
the arena frame pops) — fine for the bump allocator that backs every
native build.

## Verification

- `mighty-ide/repro/repro.exe` (built with the patched compiler, real
  bumpalo arena via `mty_rt_abi.lib`) prints **`repro: v.len()=5`**
  (was `0`).
- New regression suite `crates/mty-codegen-cranelift/tests/vec_push_native.rs`
  JIT-runs the L28 shape and asserts the vec grows: push-loop→5,
  push→len, push→index-read sum, empty→0, and 100-push multi-realloc→100.
  All pass.
- Full `mty-codegen-cranelift`, `mty-driver`, `mty-ir` suites stay green;
  `cargo fmt --check` + `cargo clippy -D warnings` clean for the crate.

## Follow-ups (out of scope here)

- Index *assignment* `v[i] = x` still hits `place_addr`'s
  `Projection::Index => Unsupported`. The read path is fixed; the write
  path can reuse the same `data`-load + `idx*8` store.
- Element width is fixed at 8 bytes per slot. Fine for scalars; a packed
  `Vec[U8]` would waste 7/8 of storage (correctness is unaffected since
  reads/writes use the same slot width).
