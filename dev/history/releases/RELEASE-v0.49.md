# Mighty v0.49 Release Notes

**Tag:** `v0.49.0`
**Date:** 2026-06-04
**Status:** SHIPPED — native `std.crypto` + `std.encoding` codegen, plus
a transparent interpreter fall-back for not-yet-native stdlib that
clears the last two `VecOfAggregate` conformance examples.

**Headline:** Mighty v0.49 — native crypto/encoding + graceful stdlib
fall-back: `mty run` now hashes and encodes on the Cranelift path, and
any stdlib surface without native codegen runs the program through the
interpreter instead of crashing (marquee: examples `42_crypto_url` and
`43_secure_session` now pass — the `VecOfAggregate` class is fully
closed).

## Summary

v0.49 finishes the native-stdlib correctness story the v0.48 codegen
pass opened. The native Cranelift backend gains real `std.crypto`
(SHA-256/512, BLAKE3, HMAC-SHA-256) and `std.encoding` (hex, base64,
base64url) lowerings, calling the same RustCrypto crates the
interpreter uses. Just as importantly, the backend stops *silently
crashing* on stdlib it hasn't lowered yet: `is_interpreter_hosted_stdlib`
now routes `std.url` / `std.uuid` / `std.regex` / crypto-AEAD /
`random_bytes` (and the other interpreter-only modules) to a
`CodegenError::Unsupported`, which `mty run` turns into a transparent
per-program interpreter fall-back. The program runs correctly instead
of dereferencing a stubbed null pointer — which is exactly what made
examples 42/43 SIGSEGV. With both halves in place, all three
`VecOfAggregate` examples (26/42/43) now pass at interpreter parity.

## Shipped

- **Native `std.crypto` + `std.encoding` (PR #41).** New runtime ABI
  functions in `mty-runtime/codegen_abi.rs` —
  `mty_runtime_crypto_{sha256,sha512,blake3,hmac_sha256}` and
  `mty_runtime_encoding_{hex_encode,base64_encode,base64_encode_url_no_pad}`
  — backed by `sha2`/`hex`/`hmac`/`base64`/`blake3` depended on
  directly (not via `mty-stdlib`, to avoid a dependency cycle). Digests
  return raw `Bytes`, encoders return `String`; both pipe through the
  existing `(ptr,len)` aggregate model so a `hex.encode(sha256(x))`
  chain JITs end-to-end. Codegen dispatches by `EffectInvoke` full-name
  (`is_native_crypto_encoding` → `emit_crypto_encoding_call`). Validated
  with known-answer vectors (SHA-256, HMAC-SHA-256 RFC 4231, base64url).

- **Transparent interpreter fall-back for not-yet-native stdlib.**
  `is_interpreter_hosted_stdlib` previously returned `false` for
  everything, so an unimplemented stdlib call hit the silent
  `mty_runtime_extern_call` stub (returns 0) and SIGSEGV'd the instant
  the 0 was used as an aggregate result. It now reports the call as
  interpreter-hosted, and `mty run` (see `cmd::run::run`) falls back to
  the interpreter for the whole program — same capability checks and
  output as `--legacy-interp`, no crash. **Examples `42_crypto_url` and
  `43_secure_session` now pass** and are removed from `KNOWN_FAILING`;
  the `VecOfAggregate` class is fully closed.

- **DX docs refresh.** `docs/reference/cli/mty-run.md` now states the
  real native coverage (`std.fs`, `std.crypto` digests, `std.encoding`
  encoders run natively; everything else falls back transparently),
  replacing the stale pre-v0.45 "fs falls back to the interpreter" note.

## Carry-forward priorities

- **More native stdlib** (so these surfaces run on the Cranelift path
  rather than falling back): `std.url` parse/builder, `std.uuid` v4/v7,
  `std.regex`, crypto AEAD (`aes_gcm`, `chacha20_poly1305`) and
  `random_bytes`, the `std.encoding` decoders. These return structs the
  caller field-accesses, so they need opaque-handle codegen + runtime
  accessor functions (model on the `DirIter` handle) — tracked in #297.
- **Native `String` methods** (`len`/`push_str`/…) — needs the String
  value-model rework (typing String bindings currently breaks struct
  `String` fields, e.g. example 28); recorded in #297.
- Pending tasks #253 (SWE-bench), #262 (BOLT training profile path).

## Acknowledgements

The fall-back fix turns a class of mystery segfaults into "it just
runs": any stdlib the native backend can't lower yet now degrades to
the interpreter transparently, so adding native coverage is a pure
performance/optionality improvement rather than a correctness gate.
