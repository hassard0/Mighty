# RAG-with-regex changelog

## 2026-05-01 — v0.40 T4

- New: `std.regex.Regex` surface — `find`, `find_all`, `captures`,
  `captures_all`, `replace`, `replace_all`, `is_match`, `split`.
- New: `std.crypto.aes_gcm` + `std.crypto.chacha20_poly1305` AEAD
  ciphers.

## 2026-04-12 — v0.40 T1

- Doc: regex internals design note landed at
  [docs/internals/std-regex.md](docs/internals/std-regex.md).

## 2026-03-30 — v0.39 T1

- New: `std.crypto.sha256` / `sha512` / `blake3` hashes.
- New: `std.crypto.hmac_sha256` + `std.crypto.random_bytes`.
- New: `std.encoding.base64` + `std.encoding.hex`.
- New: `std.url.Url` builder + `percent_encode`.
- New: `std.uuid.Uuid.v4` + `v7`.
