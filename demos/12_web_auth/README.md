# demo 12 — web_auth

v0.39 + v0.40 forcing-function demo: real-world web authentication
built end-to-end from the new stdlib surface.

## What it shows

- **`std.crypto.hmac_sha256`** — server-side password digest (with a
  pepper) and constant-time verification of the candidate tag.
- **`std.crypto.aes_gcm.encrypt` / `decrypt`** — authenticated
  encryption with associated data; the version tag is the AAD so a
  future key rotation rejects stale cookies cryptographically (not
  just by policy).
- **`std.crypto.random_bytes`** — fresh per-session nonce material.
- **`std.uuid.Uuid.v7`** — time-ordered session identifier suitable
  for use as a DB primary key (better B-tree locality than UUIDv4).
- **`std.url.percent_encode`** — defence-in-depth percent-encode of
  the sealed cookie value before it lands inside a Set-Cookie header.
- **`std.encoding.hex` / `std.encoding.base64`** — text shapes for the
  derived key + sealed cookie.

## Build

```
cargo build -p mty-cli
```

## Smoke (no LLM call, no server bind)

```
bash demos/12_web_auth/smoke.sh
```

Asserts:
- `mty check` and `mty fmt --check` pass.
- `mty run` exercises the full login pipeline and prints every
  expected event marker (`evt:auth:start` ... `evt:auth:roundtrip-ok`
  followed by `web_auth: login pipeline OK ...`).
- Every v0.39 + v0.40 surface marker (`std.crypto.hmac_sha256(`,
  `std.crypto.aes_gcm.encrypt(`, `std.uuid.Uuid.v7(`,
  `std.url.percent_encode(`, ...) appears in the demo body.
- The `web/index.html` UI fixture exists and references the
  expected `/login` POST shape.

## What the demo wires together

```
       client (web/index.html)
            |
            v   POST /login (form-urlencoded)
       _hash_password(pepper, password)        HMAC-SHA-256 tag
            v
       _verify_password(...)                   constant-time compare
            v
       _new_session_id()                       Uuid.v7 (time-ordered)
            v
       _derive_session_key(master_key, sid)    HMAC-derived AEAD key
       _fresh_nonce()                          12-byte random nonce
            v
       _seal_cookie(key, nonce, ver, payload)  AES-GCM seal
            v
       _set_cookie_header("sid", sealed)       percent-encoded
            v
       _login_response(sid, header)            HTTP 200 JSON
```

## Security caveats (read before deploying)

This demo pins the **shape** of a real login flow but is **not** a
drop-in production handler. Before adopting any of this:

1. **TLS only.** The Set-Cookie header is marked `Secure`; serve the
   handler over HTTPS or the cookie will be silently dropped by the
   browser anyway. Mighty's hosted HTTP surface is TLS-only.
2. **Use a slow KDF for passwords.** This demo uses HMAC-SHA-256
   because it is what v0.39 T1 surfaces. Real password storage needs
   argon2id or scrypt (v0.41 backlog). HMAC + pepper is fine for
   *session-cookie* AEAD; it is **not** fine as the only barrier
   between a stolen DB and offline brute force.
3. **Rotate the master key.** The AAD on `_seal_cookie` is a version
   tag (`v=1`) precisely so a master-key rotation can invalidate
   every previously-issued cookie by bumping to `v=2` and refusing
   to decrypt `v=1`.
4. **Per-session nonces.** AES-GCM nonces MUST be unique per
   (key, message). `_fresh_nonce` returns 12 random bytes; the
   collision space is 2^-96, safe for any realistic session volume.
   Do *not* reuse a nonce across messages with the same key — that
   breaks AES-GCM catastrophically.
5. **Don't roll your own.** Mighty's stdlib wraps audited Rust crates
   (`hmac`, `aes-gcm`, `chacha20poly1305`, `rand`). Stay on the
   stdlib path; don't reimplement these primitives in user code.

## Files

- `src/main.mty` — the login pipeline.
- `web/index.html` — minimal login form fixture.
- `mighty.toml` — package manifest.
- `smoke.sh` — surface + runtime marker validation.

## See also

- `examples/42_crypto_url.mty` — v0.39 T1 canonical example
  (`std.crypto` + `std.encoding` + `std.url` + `std.uuid`).
- `examples/43_secure_session.mty` — v0.40 T4 canonical example
  (`std.regex` + AEAD).
- `docs/internals/std-crypto.md` — design notes for the crypto
  module.
- `crates/mty-stdlib/src/crypto/` — Rust implementation.
