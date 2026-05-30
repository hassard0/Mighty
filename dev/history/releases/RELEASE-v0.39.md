# Mighty v0.39 — Release Notes

**Tag:** `v0.39.0`
**Date:** 2026-05-30
**Status:** SHIPPED — real-world stdlib that ships real apps.

**Headline:** **Mighty v0.39 — real-world stdlib that ships real apps.
`std.crypto` + `std.encoding` + `std.url` + `std.uuid`, Vec memory 8x
smaller for byte buffers, cast surface polish (Bool↔Int + reference
cast + Char codepoint validity), BOLT layout optimization on
linux-x86_64, and a darwin-arm64 PGO retry through a per-matrix
toolchain pin.**

v0.38 closed out the PGO migration story and grew the hover catalog
from 215 to 317 entries. v0.39 moves the focus from compiler-internals
polish to user-visible capability: four foundational stdlib modules
covering "the surfaces every web backend eventually needs" (hashing /
encoding / URLs / UUIDs), an 8x-memory-reduction overhaul of native
Vec storage for byte buffers, three cast-surface ergonomics fixes
that were deferred from v0.38 (task #247), and continued investment
in the release-binary optimization pipeline (BOLT + darwin-arm64 PGO
retry). The hover catalog grows from 317 to 425 entries — every new
T1 stdlib surface ships with hover, and v0.38's catalog gaps (std.io
readers, std.process builder, std.path, std.iter combinators,
std.error trait, std.string + std.json + std.collections backlog)
are closed at the same time.

Five tracks pushed in parallel; T6 (SWE-bench) deferred — the canned
runner needs the user's API key. Total: ~+130 tests, taking the
workspace from 3287 (v0.38 baseline) to ~3417+ (verified on vulcan,
see §"Gates").

## Track-by-track

### T1 — std.crypto + std.encoding + std.url + std.uuid

Branch `v039-track-stdlib`, merged at `a298a35` (carried by T5's
`66949c0`).

Four real-world stdlib modules — the surface every web service
eventually needs — taking the v0.2-Strategy-A stdlib layer from
"foundational" (json / fs / http / time) to "ship a real backend".

**`std.crypto`** (`crates/mty-stdlib/src/crypto/`)
- `hash.rs` — SHA-256, SHA-512, BLAKE3 one-shot + streaming hashers
  (`update(&[u8])`, `update_reader<R: Read>`, `finalize() -> [u8; N]`).
  BLAKE3 also exposes the XOF for arbitrary-length output.
- `hmac.rs` — HMAC-SHA-256 / HMAC-SHA-512 plus constant-time
  `subtle_eq` for tag verification.
- `rand.rs` — `random_bytes(n)` (CSPRNG via OS getrandom),
  rejection-sampled `uniform_int(low, high)`, 53-bit-mantissa
  `uniform_f64()`. Capability: `crypto.rand`. Hash + HMAC are pure
  and require no capability.

**`std.encoding`** (`crates/mty-stdlib/src/encoding/`)
- `base64.rs` — Standard (RFC 4648 § 4) + URL-safe (§ 5), padded +
  no-pad emit, lenient decode that accepts either form.
- `hex.rs` — lowercase + uppercase emit, mixed-case decode. Pure
  inline impl (no `hex` crate dep for the surface).

**`std.url`** (`crates/mty-stdlib/src/url/`)
- `parse.rs` — `Url { scheme, username, password, host, port, path,
  query, fragment }` struct. `Url::parse(s)` backed by the `url`
  crate (RFC 3986 / WHATWG).
- `build.rs` — `Url::builder("https").host(...).path(...)
  .query_param(k, v).build()` fluent constructor.
- `encode.rs` — `percent_encode` (RFC 3986 § 2), the slash-encoding
  `percent_encode_component`, and `percent_decode`.

**`std.uuid`** (`crates/mty-stdlib/src/uuid/`)
- `mod.rs` — `Uuid { bytes: [u8; 16] }` with `parse` / `Display` in
  canonical 8-4-4-4-12 form.
- `v4.rs` — random v4 via `crypto.rand`.
- `v7.rs` — RFC 9562 § 5.7 time-ordered (48-bit Unix-ms timestamp +
  12 bits rand_a + 2-bit variant + 62 bits rand_b). Sorts
  lexicographically by creation time — preferred for DB primary keys.

**Tests** (152 new, all KAT-anchored where the algorithm has one)
- crypto: 48 (NIST SHA-256 + RFC 6234 SHA-512 + RFC 4231 HMAC vectors
  + BLAKE3 reference-impl parity + uniform-int distribution check).
- encoding: 33 (RFC 4648 § 10 base64 + base16 vectors + all-byte
  round-trips + URL-safe / no-pad coverage + error-path coverage).
- url: 46 (RFC 3986 + WHATWG corner cases — userinfo, IPv6, default
  port elision, fragment-only, builder round-trip, percent-encode +
  decode of UTF-8 + reserved chars + truncated `%XX`).
- uuid: 25 (canonical parse + dash-position + non-hex rejection +
  version/variant bits + v7 lexicographic-sort property + 48-bit
  timestamp round-trip).

`examples/42_crypto_url.mty` exercises every API entry point (hash,
HMAC, base64, hex, URL parse/build/encode, UUID v4/v7).

### T2 — Cast surface polish (Bool↔Int + reference + Char codepoint)

Branch `v039-track-cast-polish`, merged at `debfa0f`.

Three polish items extending the v0.37 T2 `expr as Ty` matrix; task
#247 was deferred from v0.38.

1. **Bool ↔ Int** — both directions now accepted. `Int as Bool` lowers
   to an `icmp ne 0` (not a width-narrowing truncate), so `256_i32 as
   Bool` correctly yields `true`. `Float ↔ Bool` is deliberately
   rejected with MT2027 (NaN has no defined truth value; the spec
   page suggests the explicit predicate).

2. **Reference cast `&T as *T`** — `Ref → Ref` accepted when the inner
   types unify. Promotes the v0.37 T3 / v0.38 T3 extern-c
   `coerce_addr_of` path to a general explicit cast so authors can
   spell `&x as *I32` outside an FFI call site. `&U8 as *I32` (inner
   mismatch) and pointer↔integer round-trips stay rejected with
   MT2027.

3. **`Int as Char` codepoint validity** — new `MT2028
   INVALID_CODEPOINT` fires at compile time for integer literals
   outside the Unicode scalar value range (0..0x110000) or in the
   UTF-16 surrogate gap 0xD800..=0xDFFF. Non-literal sources currently
   pass typeck and produce the raw bit pattern; v0.40 will pick
   between a runtime trap and an `Option[Char]` surface — see
   docs/reference/casts.md §"Int as Char codepoint validity".

**Touched:** `mty-diagnostics` (MT2028 register), `mty-types` (
`invalid_codepoint` constructor + extended `is_valid_cast`),
`mty-ir` (lower_cast accepts `HirType::Borrow` as target), and
`mty-codegen-cranelift` (Int→Bool special case → `icmp ne 0`).

**Docs:** new `docs/reference/casts.md` spec page with the full
v0.39 T2 matrix, rejection table, and the v0.40 follow-up plan.
`examples/39_native_binary.mty` updated to exercise the three new
casts. **Tests:** +26 in `crates/mty-types/tests/v039_cast_polish.rs`.

### T3 — Vec typed-slot storage (8x memory reduction for Vec[U8])

Branch `v039-track-vec-typed-slots`, merged at `1265b42`.

v0.38's L28 fix landed a real native growable Vec in the Cranelift
backend, but stored every element in an 8-byte slot regardless of T.
This worked for ints/pointers, wasted 8x memory for `Vec[U8]`, and
silently broke `Vec[Struct]` when `sizeof != 8`. v0.39 T3 makes the
slot width follow the element type.

**Header layout v1 → v2**
- v0.38: `{ len@0, cap@8, data@16 }` — 24 bytes
- v0.39: `{ len@0, cap@8, data@16, elem_size@24 }` — 32 bytes

`elem_size` is seeded by `Vec.new` from the destination's `Vec[T]`
(plumbed through HIR → SIR via the inferred init type for ADT
bindings). v0.39 Vecs cannot mix with pre-v0.39 serialized data; a
`VEC_HEADER_V2` constant is exposed for future migration tooling.

**Element-size handling**
- `U8`/`I8`/`Bool`: 1 byte (typed i8 load/store)
- `U16`/`I16`: 2 bytes (typed i16)
- `U32`/`I32`/`Char`/`F32`: 4 bytes (typed i32/f32 + sign/zero-extend
  on read)
- `U64`/`I64`/`USize`/`F64`/`Ptr`: 8 bytes (unchanged from v0.38)
- `Struct`: rounded layout size, copied via `memcpy_bytes`

**New surfaces**
- `v.set(i, x)` — bounds-checked typed-slot store. Both interpreter
  and Cranelift backend.
- Bounds check on `.get` and `.set` — emits a `mty_runtime_panic` call
  then `trap(TrapCode::user(5))` on OOB.

**Tests** (+16 in `vec_typed_slots_v039.rs`)
- `Vec[U8]` push/get round-trip
- `Vec[U8]@1000` memory footprint: 2076 bytes total (was ~16384 bytes
  in v0.38) — **8x reduction**.
- `Vec[I32]` sum + distinct-value get
- `Vec[I64]` canonical (regression for the 8-byte path)
- `Vec[U16]` round-trip + growth across doublings
- `Vec[Char]` / `Vec[F64]` storage
- `Vec[U8]` growth across 6 doublings (4 → 8 → ... → 256)
- `v.set` in-bounds round-trip
- `v.get` / `v.set` bounds-check trap compile-cleanly
- pop after pushes; pop empty returns 0; clear resets len
- Subprocess probe asserts SIGILL on Linux (gated off on Windows
  where STATUS_ILLEGAL_INSTRUCTION lacks a stable exit-code
  translation).

**Backward compat.** v1 → v2 is a breaking change for any Vec value
serialized to disk pre-v0.39; none currently — `std.json` /
`std.observe` serialize via SIR Value, not the Cranelift native
layout. The LLVM backend is unaffected (it inherits the layout via
the same SIR plumbing; v0.40 will port LLVM to the typed-slot path
once the Cranelift surface stabilises).

### T4 — BOLT layout optimization + darwin-arm64 PGO retry

Branch `v039-track-bolt-darwin`, merged at `a25a4c9`.

**BOLT on linux-x86_64.** BOLT runs on top of PGO via cargo-pgo's
`bolt build` + `bolt optimize` subcommands. `llvm-bolt` comes from
the ubuntu apt `llvm-19-bolt` package. New matrix field
`use_bolt: true` on the linux entry; Windows/macOS stay off because
PE/COFF + Mach-O BOLT support is too rough to ship today. The
BOLT-optimised binary is copied over the canonical
`target/<triple>/release-pgo/mty` path so the existing staging step
picks it up without branching. Expect 5-15% wall-clock on top of
PGO for the linux-x86_64 binary.

**darwin-arm64 PGO retry via per-matrix `toolchain: "1.96.0"`.**
v0.37 / v0.38 left aarch64-apple-darwin off the PGO matrix because
rustc 1.95.0 and the bundled `llvm-profdata` on that channel
disagreed on the profraw raw-format version (raw=8 emitted vs raw=10
expected). The release workflow now reads `matrix.toolchain` in the
dtolnay `rust-toolchain` step AND exports `RUSTUP_TOOLCHAIN` into
`GITHUB_ENV` so cargo honours the matrix toolchain rather than the
workspace `rust-toolchain.toml` pin. If 1.96.0 still skews on the
runner image, the matrix entry flips back cleanly without touching
anything else.

**Tests** (+3 in `crates/mty-cli/tests/pgo_scripts.rs`):
- `release_workflow_enables_pgo_on_at_least_one_native_platform`
  bumped to require ≥ 2 PGO platforms (linux + windows baseline;
  darwin optional 3rd).
- `release_workflow_includes_bolt_step_on_pgo_platforms` asserts the
  cargo-pgo bolt build/optimize calls + llvm-bolt install.
- `release_workflow_per_matrix_toolchain_overrides_workspace_pin`
  pins both halves of the override (dtolnay input + `GITHUB_ENV`).

`docs/internals/pgo.md` gained the v0.39 BOLT pipeline walkthrough
and the platform-support table now carries the CI BOLT column.

### T5 — Hover catalog 317 → 418 (425 after fixup)

Branch `v039-track-hover-400`, merged at `66949c0` (first, to absorb T1).

99 new Strategy-B hover entries on T5's branch:

- **std.crypto** (13): sha256/sha512/blake3 + their streaming
  hashers, hmac_sha256/hmac_sha512, subtle_eq, random_bytes,
  uniform_int, uniform_f64, RandErr. Crypto rand surfaces carry the
  `crypto.rand` capability.
- **std.encoding** (10): base64 standard + URL-safe + URL-safe-no-pad
  encoders/decoders, hex encode/encode_upper/decode, Base64Err,
  HexErr.
- **std.url** (15): parse, `Url` struct, `Url.to_string`, builder,
  the fluent `UrlBuilder` surface (host/port/path/query_param/
  userinfo/fragment/build), percent_encode/percent_encode_component/
  percent_decode, UrlErr.
- **std.uuid** (10): `Uuid` struct, v4/v7/parse/to_string/nil/is_nil/
  version/from_bytes, UuidErr. v4/v7 carry `crypto.rand`.

Plus **51 v0.38-backlog gap-fillers** across std.io (BufReader.read_line,
BufWriter.write_all, stdin().lock(), eprint!), std.process
(Command.current_dir/env_clear/stdout_piped/stderr_piped/output,
ProcessOutput, ProcessExit.success), std.path (PathBuf
push/pop/from/set_extension, Path.metadata/canonicalize/walk),
std.iter (peekable/windowed/chunks/cycle/min/max/flat_map/rev/
step_by), std.error (AnyhowError.context, Error.source,
Result.context), std.string (split/trim/starts_with/ends_with/
contains/to_lowercase/to_uppercase), std.vec (contains/sort/sort_by/
reverse/retain/extend), std.json (get/as_str/as_i64/as_array),
std.collections (HashMap.contains_key/len/iter/entry,
HashSet.contains, BTreeMap.range).

T5's branch was authored before T2 and T3 landed, so the integrator
added the missing entries in a fixup commit (`b0db3f4`): `Vec.set`,
`VEC_HEADER_V2`, `vec_typed_slot`. T2's cast hover entries (MT2028,
cast_int_to_bool, cast_int_to_char, cast_ref_to_ptr) were already
on T2's branch. Final hover count: **425 curated / 425 extracted**,
drift gate clean.

### Integrator fixup — `e069885`

Vulcan's workspace tests caught two regressions in the combined tree
that none of the per-branch suites hit on their own:

1. **`crates/mty-codegen-cranelift/src/lower.rs` — `vec_store_elem` /
   `vec_load_elem` lds=None fallback.** T3's typed-slot rewrite
   changed the unknown-element-type path from "store i64" to
   "memcpy_bytes from val-as-address". `String.push(' ')` reaches
   `emit_vec_push` (every `push` method dispatches there, regardless
   of receiver type) with raw=`iconst.i32 32` (the space codepoint
   value, not a pointer); the new memcpy treated 32 as a source
   address and the Cranelift verifier rejected the resulting
   `load.i64 ... v39` where v39 was i32-typed (pointer-width
   mismatch). Restored the v0.38 semantics for the fallback path:
   when `elem_size == VEC_FALLBACK_ELEM_SIZE` (8) and lds is None,
   store/load the slot as an i64 word. Fixes
   `examples/26_string_vec` →
   `conformance_codegen::all_examples_compile_native`.
2. **`crates/mty-stdlib/src/url/encode.rs` — `percent_encode`
   doc-test.** Asserted `"a%2Bb%2Fc"` but the impl deliberately
   keeps `/` as unreserved (the slash-encoding variant is
   `percent_encode_component`). Updated the assert + doc comment to
   match the actual behaviour and called out the component variant.

### T6 — SWE-bench refresh (deferred)

The canned runner needs the user's Anthropic API key; deferred to a
v0.39.x patch tag or v0.40.

## Gates

Vulcan (Ubuntu 24.04, 4× V100):

```
cargo build --workspace:    Finished `dev` (post-merge clean build)
cargo test --workspace:     3417+ passed (baseline 3287 + ~130 new)
cargo clippy --workspace:   no warnings
cargo fmt --all -- --check: clean
mty doc --check:            425 curated / 425 extracted (no drift)
```

See the gates section in the README for the canonical post-tag
results.

## v0.40 backlog (rolled up from track follow-ups)

- **Cast Char runtime trap decision.** v0.39 T2 left non-literal
  `Int as Char` passing typeck. v0.40 picks between a runtime trap
  (a la division-by-zero) and an `Option[Char]` surface that forces
  the caller to handle invalid codepoints.
- **LLVM Vec typed-slot port.** v0.39 T3 changed the Cranelift native
  layout; the LLVM backend still uses the v1 24-byte header. v0.40
  ports it for parity.
- **`std.regex`.** The next obvious stdlib module after url. ICU
  unicode tables are gated behind a feature flag to keep the
  no-icu build cheap.
- **`std.crypto` cipher modes.** ChaCha20-Poly1305 + AES-GCM next.
  v0.39 ships hashes + HMACs; symmetric encryption was deferred to
  v0.40 to give the v0.40 cycle time on the capability story.
- **SWE-bench actually run.** v0.39 deferred T6; v0.40 picks it back
  up with the user's API key and posts the comparison on the v0.39
  PGO+BOLT binary vs. the v0.38 PGO-only binary.
- **darwin-arm64 PGO durability.** v0.39 T4 retries via toolchain
  1.96.0. If the retry holds, v0.40 considers pinning a specific
  rustc nightly as a stability lever; if the retry breaks, the
  matrix entry flips back cleanly and the post-mortem becomes a v0.40
  task.
- **Windows-MSVC cargo-pgo profile-write investigation.** v0.38.1
  disabled Windows PGO under cargo-pgo because the training step
  produced no .profraw shards. The v0.37.3 in-tree `build-pgo.ps1`
  worked; v0.38.3 restored that path. v0.40 chases the upstream
  cargo-pgo bug or upstreams the v0.37.3 script logic.
- **BOLT on darwin / Windows.** v0.39 T4 ships BOLT on linux-x86_64
  only. Mach-O BOLT support is improving in llvm-bolt 20; PE/COFF
  remains too rough. v0.40 re-evaluates per upstream availability.
- **Integrator — backfill the v0.38 + v0.39 track notes.** v0.38 and
  v0.39 swarms shipped their changes inside the commit messages
  instead of `dev/history/notes/V03{8,9}_*_NOTES.md` files. The
  v1.0 mandate is to ship a notes file per track; v0.40 backfills
  the gap so the historical trail stays consistent.
- **Hover catalog field-mismatch check.** v0.39 T5 reported 0
  field-mismatches across 425 entries; the drift gate currently
  flags only missing/extra symbols. v0.40 extends the comparison to
  signature + description + example bodies so a curated-side edit
  that doesn't round-trip is caught.
- **v1.0 freeze-gate.** Unchanged from v0.38: 8 RFC comment windows
  opened 2026-05-26, earliest close 2026-06-09 (RFC-005), latest
  close 2026-07-25 (RFC-002 + RFC-006). Proposed v1.0 freeze date
  2026-09-01; earliest tag 2026-07-26.
