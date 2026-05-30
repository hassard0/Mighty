# Mighty v0.40 — Release Notes

**Tag:** `v0.40.0`
**Date:** 2026-05-30
**Status:** SHIPPED — closing v0.39 follow-ups + real-world crypto + parsing.

**Headline:** **Mighty v0.40 — closing v0.39 follow-ups + real-world
crypto + parsing. `std.regex` + AES-GCM + ChaCha20-Poly1305 close the
"every web service eventually needs this" gap left by v0.39's
hashing/encoding/URL/UUID round. The LLVM backend gains Vec
typed-slot parity with the v0.39 native lowering. `Char.from_u32`
lands as a proper `Option` constructor and non-literal `Int as Char`
is now a compile error. BOLT layout optimization is restored on
linux-x86_64 after the v0.39.0 strip/emit-relocs collision via a
separate `release-pgo-bolt` profile. Two new end-to-end demos (web
auth + RAG with regex chunking) showcase the v0.40 surface.**

v0.39 was the "real-world stdlib that ships real apps" round —
hashing, encoding, URLs, UUIDs, the Vec memory overhaul, the cast
surface polish, the BOLT layout-optimization attempt. v0.40 closes
the follow-ups: the cast surface picks the `Option` route for
`Char` (T3), the LLVM backend catches up with the native Vec layout
(T2), BOLT comes back via a clean profile separation (T1), and the
crypto+parsing surface picks up the modules that round out the
"real backend" floor — `std.regex` + the two RustCrypto AEAD ciphers
(T4). T5 grows the hover catalog from 425 to 564 entries to cover
the new surfaces; T6 ships two end-to-end demos that exercise them.

Six tracks pushed in parallel. Total: ~+138 tests, taking the
workspace from ~3417 (v0.39 baseline) to **3555** (verified on
vulcan, see §"Gates").

## Track-by-track

### T1 — BOLT layout optimization restored on linux-x86_64

Branch `v040-track-bolt-strip-fix`, merged at `2a1e1ab`.

v0.39.0 first shipped BOLT then v0.39.1 reverted it: the
`release-pgo` profile sets `strip = "symbols"` (smaller binary), but
BOLT needs the symbol table intact to rewrite the basic-block layout
and needs the linker to keep relocations (`-Wl,-q`, "emit-relocs").
The combination produced a binary that BOLT refused to optimise on
the runner.

The v0.40 fix is structural: a new **`release-pgo-bolt` profile** in
the workspace `Cargo.toml` that inherits from `release-pgo` but sets
`strip = "none"` and pairs with `RUSTFLAGS=-C link-arg=-Wl,-q` in
`release.yml`. The release workflow now builds BOTH the plain
`release-pgo` binary (smaller, no BOLT) and the
`release-pgo-bolt` binary (larger, BOLT-optimised). Linux-x86_64
ships the BOLT-optimised variant; the plain PGO variant stays
available as a fallback artifact for users who prefer a smaller
binary.

Why a separate profile rather than just turning off strip on
`release-pgo`: the plain `release-pgo` profile is also used by
windows-x86_64 and (in v0.40) darwin-arm64. Touching that profile
would balloon their release binaries unnecessarily. The separate
profile keeps the BOLT path opt-in per-platform.

**Tests:** +2 in `crates/mty-cli/tests/pgo_scripts.rs` asserting
(a) the `release-pgo-bolt` profile exists with `strip = "none"` and
(b) the `release.yml` job for that profile threads
`-C link-arg=-Wl,-q` through `RUSTFLAGS`.

### T2 — LLVM backend Vec typed-slot port

Branch `v040-track-llvm-vec`, merged at `bec3959`.

v0.39 T3 changed the native Vec memory layout so the slot width
follows the element type (1 byte for `U8` / `I8` / `Bool`, 2 for
`U16` / `I16`, 4 for `U32` / `I32` / `Char` / `F32`, 8 for `U64` /
`I64` / `USize` / `F64` / `Ptr`, rounded for structs). Header grew
from v1 (24 bytes: len, cap, data) to v2 (32 bytes: + elem_size).
The Cranelift backend was rewritten in v0.39; the LLVM backend
(behind `--features llvm`) still used the v1 header and the
uniform 8-byte slot.

v0.40 T2 ports the LLVM lowering for parity:

- **Header v2 emit** — `Vec.new` and `Vec.with_capacity` now allocate
  32 bytes for the header and store `elem_size` in the new slot.
- **Per-elem-size load/store** — `Vec.push` / `Vec.get` / `Vec.set`
  pick the LLVM integer type at codegen time (`i8` / `i16` / `i32`
  / `i64`) and cast the data pointer to the corresponding typed
  pointer before the load/store. Struct elements go through
  `memcpy_bytes` with the rounded layout, same as the native side.
- **Bounds check** — `Vec.get` and `Vec.set` emit the same OOB
  trap shape as the native backend (compare against `len`, branch
  on overflow, call `mty_runtime_panic`, then `unreachable`).
- **Header constant alignment** — `crates/mty-stdlib/src/vec.rs`'s
  `VEC_HEADER_V2` constant is now consumed by both backends so the
  on-disk layout stays single-sourced.

**Tests:** +16 in `crates/mty-codegen-llvm/tests/vec_typed_slots_v040.rs`
(LLVM-gated behind `--features llvm`). Coverage matches the native
suite: header shape, per-type slot widths, bounds-check, struct
elements with rounded layout, the AOT and JIT paths both.

### T3 — `Char.from_u32` Option API + reject non-literal `Int as Char`

Branch `v040-track-cast-char`, merged via T5 absorb.

v0.39 T2 closed the literal-source path for `Int as Char` (MT2028
INVALID_CODEPOINT fires at compile time for the surrogate gap +
codepoints ≥ `0x110000`). The non-literal path was left open as a
v0.40 follow-up: do we trap at runtime, or do we force the caller
to handle invalid codepoints up front?

v0.40 T3 picks the **`Option` route**:

- **New API — `Char.from_u32(U32) -> Option<Char>`** in the
  stdlib prelude. Returns `Some(c)` for `0x0..0xD800` and
  `0xE000..0x110000`, `None` otherwise. Pure, no capability.
- **New diagnostic — MT2027 REQUIRE_CHAR_FROM_U32.** Non-literal
  `Int as Char` is now a typeck error. The fix envelope points at
  `Char.from_u32(x)?` (or `.unwrap_or('\0')` for callers that just
  want a fallback). Literal casts still compile under the v0.39
  MT2028 validator.
- **Docs** — `docs/reference/casts.md` rewritten so the cast surface
  reads as a single page (Bool↔Int from v0.39, reference cast from
  v0.39, Char codepoint validity from v0.39+v0.40).

**Tests:** +8 spanning typeck rejection (the new MT2027), runtime
behaviour of `Char.from_u32` on edge codepoints (surrogate gap +
0x110000 boundary), and the v0.39 MT2028 path stays unchanged.

### T4 — std.regex + AES-GCM + ChaCha20-Poly1305

Branch `v040-track-regex-aead`, merged via T5+T6 absorb.

Three foundational stdlib surfaces that close the "real-world web
service" gap left by v0.39's hashing+URL+UUID round.

**`std.regex`** (`crates/mty-stdlib/src/regex/`)
- `Regex::new(pattern: &str) -> Result<Regex, RegexErr>` — compile.
- `Regex::find(&self, hay: &str) -> Option<Match>` — first match.
- `Regex::find_all(&self, hay: &str) -> Vec<Match>` — all matches.
- `Regex::captures(&self, hay: &str) -> Option<Captures>` — groups
  for the first match.
- `Regex::captures_all(&self, hay: &str) -> Vec<Captures>` — groups
  for every match.
- `Regex::replace(...)` / `Regex::replace_all(...)` — `$0`/`$1`/...
  backrefs.
- `Regex::is_match(...)` — cheap predicate.
- `Regex::split(...)` — split-on-match (CSV-style).
- `Regex::as_str(...)` — the original pattern.
- `Match { text, start, end }` — byte offsets (UTF-8 accurate).
- `Captures { groups: Vec<Option<Match>> }` — `get(i)` + `len()`.
- `RegexErr::Compile(String)` — only error variant; carries the
  underlying crate diagnostic.

Backed by Rust's `regex` 1.12.3 crate — RE2-style finite automata,
guaranteed linear time, no catastrophic backtracking. Look-around is
intentionally NOT supported. Pure (no capability).

**`std.crypto.aes_gcm`** (`crates/mty-stdlib/src/crypto/aes_gcm.rs`)
- `encrypt(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], pt: &[u8]) -> Result<Vec<u8>, AeadErr>`
- `decrypt(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], ct: &[u8]) -> Result<Vec<u8>, AeadErr>`

AES-256-GCM — the AEAD that backs TLS 1.3, AWS S3-SSE, Signal.
Returns ciphertext + 16-byte tag concatenated. Constant-time tag
verify on decrypt. Tamper / wrong-key / wrong-AAD all yield the
opaque `AeadErr::Decrypt` (no padding-oracle leak). KAT-tested
against NIST CAVP vectors.

**`std.crypto.chacha20_poly1305`** (`crates/mty-stdlib/src/crypto/chacha20_poly1305.rs`)
- Identical encrypt/decrypt shape as `aes_gcm` so the caller swaps
  by changing one function name.

RFC 8439 ChaCha20-Poly1305 — the mandatory TLS 1.3 ciphersuite for
hosts without AES-NI (ARM, embedded). Backed by RustCrypto's audited
`chacha20poly1305` 0.10. KAT-tested against the RFC vectors.

The shared error type `AeadErr` is module-scoped in
`crates/mty-stdlib/src/crypto/mod.rs` so the two ciphers share one
diagnostic surface.

**Tests:** +58 total. 24 for `std.regex` (compile + match + captures
+ replace + split paths), 18 for `aes_gcm` (NIST CAVP KAT + tamper
detection + AAD binding), 16 for `chacha20_poly1305` (RFC 8439 KAT +
tamper + AAD).

**Example:** `examples/43_secure_session.mty` — end-to-end signed +
encrypted session cookie using `hmac_sha256` + `aes_gcm` + `url`.

### T5 — Hover catalog 425 → 564

Branch `v040-track-hover-500`, merged at `b61a18b` (carries T3 + T4).

99 of the 139 new entries cover the T4 surfaces (regex 18, AEAD 6
= 24 hot entries plus the cross-references). The T3 `Char.from_u32`
symbol + the MT2027 fix-suggestion docs land. The other ~75 entries
close v0.39 hover-catalog gaps that the v0.39 T5 round didn't reach:
`std.string` slice / replace / repeat / lines / chars / bytes,
`std.vec` first / last / iter_mut / drain / chunks / windows /
slice_at, `std.collections` HashMap.drain / values / values_mut +
HashSet.union / intersection / difference, `std.json` is_object /
is_array / as_bool / get_mut, `std.io` lines() iterator + tee +
Cursor, `std.time` Duration arithmetic + Instant.elapsed +
SystemTime UNIX_EPOCH offsets.

Drift gate (`hover_catalog_no_drift`) byte-for-byte clean: 564
curated entries match 564 extracted entries from the docstubs.

The docstub regen tool (`cargo run -p mty-doc --bin
regen-stdlib-docstubs`) now writes `crates/mty-stdlib/docs/regex.docstub`
as part of the per-module output.

### T6 — Demos 12 (web auth) + 13 (RAG with regex)

Branch `v040-track-new-demos`, merged at `1248695` (carries T4).

Two new end-to-end demos taking the demo count from 11 to 13, both
shipping the standard `mighty.toml` + `smoke.sh` shape so the demo
runner picks them up automatically.

**Demo 12 — web auth.** Cookie-based session auth. `std.uuid.v7`
mints monotonically-sortable session IDs. `std.crypto.hmac_sha256`
signs the cookie body. `std.crypto.aes_gcm.encrypt` wraps the
payload (user_id + role + expiry) so the cookie reveals nothing if
intercepted. `std.url` builder constructs the redirect URL after
login. Server-side verify is constant-time via `subtle_eq`.

**Demo 13 — RAG with regex chunking.** Loads a small markdown
corpus, uses `std.regex` for paragraph-level chunking
(`\n\n+` split) and section-heading extraction (`^#+\s+(.+)$`),
embeds chunks via the in-tree mock embedder, indexes via
`std.memory.VectorStore`, then answers questions via `std.rag`. The
regex preprocessing exists specifically to show off the new surface
— alternatives (whitespace-split, line-by-line) would be inferior
in real corpora.

Both demos exercise the new surfaces end-to-end so the v0.40 release
binary CAN run the demos out of the box. `smoke.sh` for both fits
inside the standard demo template.

## Gates

Vulcan (`admin@192.168.4.178`, Ubuntu 24.04, rustc 1.95.0):

```
cargo build --workspace       OK (50.10s)
cargo test --workspace        3555 passed, 0 failed, 24 ignored
cargo clippy --workspace
  --all-targets -- -D warnings  OK (41.07s)
cargo fmt --all -- --check    OK
```

Vulcan disk pressure forced a `cargo clean` mid-gate: the build tree
had grown to 73 GiB and the home partition hit 100% (740 MB free),
which caused `rust-lld` to bus-error on link. After the clean, the
gates ran end-to-end without incident. Worth noting for the next
integrator: vulcan target/ growth pattern needs revisit.

Pre-push hook (`cargo fmt --check` + `cargo clippy -D warnings` +
`mty fmt --check` on 65 `.mty` files) passed on every push.

Local Windows build (`cargo build --workspace`): OK (1m 07s,
unoptimized + debuginfo).

## CI / binaries

Six required CI workflows post-merge:

| Workflow | Status |
|----------|--------|
| `test` | (see §Verify) |
| `test-minimal` | (see §Verify) |
| `msrv` | (see §Verify) |
| `clippy-strict` | (see §Verify) |
| `bench` | (see §Verify) |
| `security` | (see §Verify) |

Five release binaries on the v0.40.0 tag:
- `linux-x86_64` (PGO via `release-pgo`)
- `linux-x86_64-bolt` (PGO + BOLT via `release-pgo-bolt`)
- `linux-aarch64`
- `macos-arm64` (PGO via per-matrix `toolchain: "1.96.0"`)
- `macos-x86_64`
- `windows-x86_64` (PGO via in-tree `scripts/build-pgo.ps1`)

## v0.41 candidates

Rolled up from the six v0.40 track reports:

- **Ed25519 / X25519 / Argon2 / HKDF** — the rest of the
  "real-world web service crypto" floor. T4 covered symmetric AEAD;
  asymmetric + KDF + password hashing close the round.
- **`std.regex.RegexSet`** — multi-pattern single-pass matching;
  the `regex` crate already exposes it, surface it.
- **Raw-string literals (`r"..."` / `r#"..."#`)** — T4 demos paper
  over the missing surface with double-backslash escapes; regex /
  crypto code reads naturally with raw strings.
- **Dynamic-string `log()` codegen.** T4 demo 13 routes
  runtime-built strings through a wrapper because the Cranelift
  `log()` codegen assumes a literal symbol. Unblock the dynamic
  path so demos read cleanly.
- **BOLT on darwin / Windows.** T1 restored BOLT on linux-x86_64.
  llvm-bolt 20 keeps improving the Mach-O path; PE/COFF stays too
  rough. v0.41 re-evaluates per upstream.
- **SWE-bench actual run.** Still deferred behind the user's API
  key. v0.41 picks it up + posts the comparison on the v0.40
  PGO+BOLT binary vs. the v0.38 PGO-only binary.
- **Hover catalog field-mismatch check.** Drift gate currently
  flags only missing/extra symbols. v0.41 extends the comparison to
  signature + description + example bodies so a curated-side edit
  that doesn't round-trip is caught.

## v1.0 freeze status (unchanged from v0.39)

8 RFC comment windows opened 2026-05-26. Earliest close
2026-06-09 (RFC-005), latest 2026-07-25 (RFC-002 + RFC-006).
Proposed freeze date 2026-09-01; earliest tag 2026-07-26.

## Operational notes

- **Vulcan disk hygiene.** target/ hit 73 GiB during the v0.40 gate
  and the home partition went to 100%, requiring an emergency
  `cargo clean` before tests would link. Either schedule a
  per-cycle clean or move target/ off the home partition.
- **Integrator fixup.** T5 added the `regex` module include in
  `crates/mty-doc/src/stdlib_walker.rs` but did NOT stage the
  generated `crates/mty-stdlib/docs/regex.docstub` file. The
  integrator regenerated it via `cargo run -p mty-doc --bin
  regen-stdlib-docstubs` and committed it under
  `106fb4d v0.40 integrator fixup: add missing regex.docstub`. Future
  T5-shape work needs to remember the docstub regen in the same
  commit.
- **Single-branch workflow held** — all six tracks merged to main,
  no PRs, branches deleted post-merge.
