# 12 — Extern

Mighty talks to other ABIs through `extern` blocks. The two ABIs
supported in v0.1 are `c` for the C ABI and `js` for JavaScript when
targeting the web.

## C interop

```sd
extern c {
  fn strlen(s: *U8) -> USize
}

export c fn add(a: I32, b: I32) -> I32 = a + b
```

- `extern c { ... }` imports C symbols. The declarations carry no body;
  the linker resolves them.
- `*U8` is a raw pointer to a byte. Raw pointers are unsafe to
  dereference — you need an `unsafe` block. See
  [chapter 13](13-unsafe.md).
- `export c fn add(...) -> I32 = a + b` exports a Mighty function
  through the C ABI. The expression body (`= a + b`) is the canonical
  one-line form (spec §10.1).

Run it:

```bash
mty check examples/14_extern_c.sd
```

## JavaScript interop

```sd
extern js {
  fn alert(msg: Str) effect dom
}
```

- `extern js` imports a JavaScript host function. Web-target builds wire
  these to the host environment through the WebAssembly Component Model.
- `effect dom` records that `alert` touches the DOM. The effect system
  (slice 5) propagates this to callers so DOM-free contexts statically
  reject it.
- Throwing JS functions map to `Result` unless declared trap-only — see
  spec §22.3.

Run it:

```bash
mty check examples/15_extern_js.sd
```

## Next

Continue to [13 — Unsafe](13-unsafe.md).
