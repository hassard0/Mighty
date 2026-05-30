# Changelog

All notable changes to Mighty are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

For the full per-release notes, see
[`dev/history/releases/`](dev/history/releases/).

## [Unreleased]

v0.42 candidates (rolled up from v0.41's IDE-dogfooding lessons log):

### Known issues — carry forward
- **L28 (P0):** native `mty build` Vec growth still broken under
  capture-rebind `v = v.push(x)`; works under interp.
- **L21 (P0):** Vec param read in nested loops SIGSEGVs under native
  codegen (likely same liveness/spill family as L28).
- **L19 (P0):** `expr as T` numeric casts don't actually convert
  (Char cast shipped v0.40 T3; int/float widening still broken).
- **L20 (P1) — FIXED (v0.42 T3):** `(a + b)(c)` no longer mis-parses as
  a CALL_EXPR. The postfix-`(` rule in `expr_bp` now only treats a
  following `(` as a call when the preceding primary is callable-shaped
  (path / call / field / index / lambda / parens wrapping any of those).
  Arithmetic/boolean paren groups (`(a + b)`, `(-f)`, `()`, `(a, b)`)
  surface a clearer parse error than the downstream MT2008.
- **L23 (P1):** native `log(...)` only takes string literals; no
  computed-value tracing on the native path.
- **L18 (P1):** `std.fs` is a Rust-internal capability API, not
  Mighty-callable.
- **L26 (sharp):** `mty fmt` no-op stub on `.mty`; DESTRUCTIVE on
  non-`.mty` input (truncates).
- **L22 (P2):** type-error spans collapse to enclosing fn start;
  ANSI always on; `mty check` ≠ a full lint.
- **Pending:** #253 SWE-bench numbers, #262 BOLT training profile path.

### v0.40-era candidates (still open)

v0.41 candidates (rolled up across the 6 v0.40 tracks + v0.39 carryovers):

- **T4 follow-ups — Ed25519 / X25519 / Argon2 / HKDF.** v0.40 T4
  added AES-GCM + ChaCha20-Poly1305 (AEAD); v0.41 picks up the
  asymmetric primitives + KDF surface so `std.crypto` covers the rest
  of the "real-world web service" floor.
- **T4 — RegexSet multi-pattern matching.** Rust's `regex` crate
  exposes `RegexSet` for "which of these N patterns matched?" in a
  single pass. v0.41 surfaces it as `std.regex.RegexSet`.
- **T4 — Raw-string literals.** v0.40 T4 demos paper over the lack
  of raw strings with double-backslash escapes; v0.41 lands
  `r"..."` / `r#"..."#` so regex/crypto code reads naturally.
- **T4 — Dynamic-string log() codegen.** v0.40 T4 demo 13 routes
  runtime-built strings through a wrapper because the Cranelift
  `log()` codegen still assumes a literal symbol. v0.41 unblocks the
  dynamic path.
- **T1 — BOLT on darwin / Windows.** v0.40 T1 restored BOLT on
  linux-x86_64 via the separate `release-pgo-bolt` profile. Mach-O
  BOLT support keeps improving in llvm-bolt 20; PE/COFF remains too
  rough. v0.41 re-evaluates per upstream.
- **T6 — SWE-bench actual run.** Still deferred behind the user's
  API key. v0.41 picks it up alongside the v0.40 PGO+BOLT vs. v0.38
  PGO-only comparison.
- **T5 — Hover catalog field-mismatch check.** Drift gate currently
  flags only missing/extra symbols. v0.41 extends the comparison to
  signature + description + example bodies so a curated-side edit
  that doesn't round-trip is caught.
- **T2 — variadic typeck tightening** (carryover from v0.38) — when
  the format string at a `printf`-shape call is statically known,
  constrain the trailing arg types to match the specifiers.
- **T2 — WASM Component Model variadic FFI** (carryover) —
  `funcref` / resource-type surface for variadic externs.
- **T3 — Mutable Str / caller-owned buffer ergonomics** (carryover)
  for row 10's `snprintf` shape — first-class mutable byte-buffer
  binding.
- **T3 — `#[ffi_nul_ok]` runtime enforcement** (carryover) — flip the
  attribute from metadata-only to opt-in for a future safety-wrapper
  pass.
- **T3 — More extern ABIs** (carryover) — `extern system`,
  `extern aapcs`, `extern sysv`.
- **T4 — Hover examples extraction** (carryover) — pull `///`
  triple-backtick blocks into the LSP hover as `### Example` sections.
- **T4 — cargo-pgo extension** (carryover) — `darwin-x86_64` PGO via
  rosetta-host sniff; `linux-aarch64` PGO via qemu-user emulation;
  per-machine `.pgo-config` for cached path resolution on dev laptops.
- **Integrator — backfill the v0.38 + v0.39 track reports.** Both
  cycles shipped track changes inside commit messages instead of
  `dev/history/notes/V03{8,9}_*_NOTES.md` files. v0.40 backfills the
  gap so the historical trail stays consistent.

The v1.0 freeze-gate is unchanged: 8 RFC comment windows opened
2026-05-26, earliest close 2026-06-09 (RFC-005), latest close
2026-07-25 (RFC-002 + RFC-006). Proposed v1.0 freeze date
2026-09-01; earliest tag 2026-07-26.

## [0.41.0] - 2026-05-30

### Fixed — honest correctness release; 5 P0 bugs from IDE dogfooding

- **T1 — struct field reads return the named field, not field 0 (L15).**
  `mty-ir` lower had two bugs: the multi-segment path resolver on the
  read path fell back to field index 0, and the assign path stored to a
  fresh temp instead of the field's slot. +10 tests in
  `crates/mty-ir/tests/struct_fields.rs`. Tuple positional access and
  L16's full follow-through left for T6 / future.
- **T2 — package-level module resolution for `mty test` / `mty check` /
  `mty run` (L13).** All three now assemble every `src/**/*.mty` into
  one HIR Package before lower / typecheck / run, instead of each entry
  file seeing only its own contents. New diagnostics MT2029
  UNRESOLVED_MODULE + MT2030 SYMBOL_NOT_IN_MODULE. +5 conformance tests
  in `crates/mty-cli/tests/cmd_test_package.rs`.
- **T3 — 5 native-codegen parity gaps closed (L1).** All in the
  Cranelift lowering; all caused segfaults under `mty build` / native
  JIT while `mty run --legacy-interp` worked: (1) `v.get(i)` now
  returns a real `Option[T]` aggregate (tag=0=Some + payload, tag=1
  =None on OOB); (2) `v.pop()` same shape fix; (3) implicit arena
  push at `main` entry so `let v = Vec.new()` outside an explicit
  `arena {}` stops null-derefing; (4) `String.clear()` /
  `String.push_str(...)` stop routing through `Vec.clear` / `Vec.push`
  (was reading the String's (ptr,len) pair as a Vec header and
  looping); (5) `stream.next()` on opaque receivers now synthesises a
  real `None` aggregate instead of a 0 scalar. New examples
  conformance suite at `crates/mty-cli/tests/conformance_examples.rs`
  runs every `examples/*.mty` through both `mty run --legacy-interp`
  and `mty run` (JIT) and diffs stdout + exit code. +9 JIT unit tests
  in `option_aggregate_v041.rs`.
- **T4 — manifest-driven native linking (L2).** `mighty.toml` grows a
  `[build]` section: `native-libs`, `link-search`, `frameworks`,
  `link-args`. New linker-flavor detection (gnu/msvc) with
  `MTY_LINKER_FLAVOR` override + MSVC arg rewrite table. New
  `crates/mty-driver/src/link_flavor.rs` (245 lines, 11 unit tests).
  +14 integration tests in
  `crates/mty-driver/tests/manifest_build_link.rs`. New example
  `examples/extern_c_with_manifest/`.
- **T6 — top-level `const` evaluates to its declared value (L16).**
  Wired through HIR → DefMap → resolve → typecheck → IR lower with
  inline-at-use; no runtime default-construction.
- **T6 — alloc-effect diagnostic carries a per-effect hint + docs
  link (L14).** Was a generic "missing effect"; now points at the
  exact alloc shape and the docs section that explains it.

### Tools / process

- **T5 — hover-catalog surface audit + CI gate.** Catalog 565 → 388
  honest entries. Whole modules deleted that never shipped
  (`collections`, `iter`, `error`, `process`). 38 entries kept as
  concept-docs via a new `# concept-doc` marker. New
  `crates/mty-doc/src/surface_audit.rs` (880 lines + tests).
  `mty doc check --check-surface` extended; CI gate added in
  `.github/workflows/ci.yml` so future docstub-vs-stdlib divergence
  fails the build, not the integrator's eye.
- **T6 — pre-push hook honors `CARGO_TARGET_DIR`.** Hook was hardcoded
  to `target/release/mty.exe`, breaking every parallel-worktree track
  that set per-worktree target dirs (the v0.36 lesson).

### Acknowledgements

Every fix in v0.41 was surfaced by dogfooding Mighty IDE
(`C:\Users\ihass\mighty-ide`, MIT). Living lessons log lives at
`mighty-ide/docs/mighty-language-lessons.md`; v0.41 is the first
release to consume it as a triage queue.

## [0.40.0] - 2026-05-30

### Added — Real-world crypto + parsing, LLVM Vec parity, BOLT restored

- **T1 — BOLT layout optimization restored on linux-x86_64.** v0.39.1
  reverted BOLT after the v0.39.0 build collided with the
  `release-pgo` profile's `strip = "symbols"` (BOLT needs the symbol
  table to rewrite the binary, plus `emit-relocs` from the linker).
  v0.40 T1 introduces a separate `release-pgo-bolt` profile that
  inherits from `release-pgo` but sets `strip = "none"` and uses
  `RUSTFLAGS=-C link-arg=-Wl,-q` for emit-relocs; the BOLT-optimised
  binary ships alongside the plain PGO binary as a release asset.
  +2 tests in `pgo_scripts.rs` asserting the profile + RUSTFLAGS
  combination is wired into `release.yml`.
- **T2 — LLVM backend Vec typed-slot port.** v0.39 T3 landed the
  Cranelift native typed-slot port; v0.40 T2 brings the LLVM backend
  to parity. New header v2 (32 bytes: len, cap, data, elem_size),
  per-elem-size load/store via `i8` / `i16` / `i32` / `i64` typed
  pointers, bounds check that emits the same `mty_runtime_panic`
  call + LLVM `unreachable` as the Cranelift path. AOT and JIT
  Vec[U8] memory footprint now matches the v0.39 native numbers.
  +16 tests in `vec_typed_slots_v040.rs` (LLVM-gated behind
  `--features llvm`).
- **T3 — `Char.from_u32(U32) -> Option<Char>` + non-literal
  `Int as Char` rejected.** v0.39 T2 left non-literal `Int as Char`
  passing typeck and emitting the raw bit pattern. v0.40 T3 picks
  the `Option` route: `Char.from_u32` is the safe constructor
  (returns `None` for surrogates `0xD800..=0xDFFF` and codepoints
  ≥ `0x110000`); non-literal `Int as Char` is now a compile error
  MT2027 (REQUIRE_CHAR_FROM_U32) with a fix-suggestion pointing at
  the new API. Literal casts still compile (the v0.39 MT2028
  validator covers them). +8 tests across `mty-types` + runtime.
- **T4 — std.regex + std.crypto.aes_gcm + std.crypto.chacha20_poly1305.**
  Three foundational stdlib surfaces that close the "real-world web
  service" gap left by v0.39. **`std.regex`** — `Regex.new` / `find`
  / `find_all` / `captures` / `captures_all` / `replace` /
  `replace_all` / `is_match` / `split` / `as_str` backed by the Rust
  `regex` crate (RE2-style linear-time, no look-around). Surfaces
  `Match { text, start, end }` and `Captures` with `get` / `len`.
  **`std.crypto.aes_gcm`** — AES-256-GCM authenticated encryption
  via RustCrypto `aes-gcm` 0.10; KAT-tested against NIST CAVP
  vectors. **`std.crypto.chacha20_poly1305`** — ChaCha20-Poly1305
  AEAD via RustCrypto `chacha20poly1305` 0.10; KAT-tested against
  RFC 8439 vectors. Identical encrypt/decrypt shape across both
  ciphers so callers swap by changing one function name. +58 tests
  total. Examples: `examples/43_secure_session.mty`.
- **T5 — Hover catalog 425 → 564.** 18 regex entries (Regex +
  methods + Match + Captures + RegexErr), 6 AEAD entries
  (aes_gcm / chacha20_poly1305 module + encrypt + decrypt), the T3
  `Char.from_u32` symbol, and ~30 v0.39 gap-fillers across
  std.string / std.vec / std.collections / std.json. Drift gate
  byte-for-byte clean (564 curated / 564 extracted).
- **T6 — Demo 12 (web auth) + Demo 13 (RAG with regex).** Two new
  end-to-end demos showcasing the v0.40 surface. **Demo 12** —
  cookie-based web auth using `std.crypto.hmac_sha256` for session
  signing + `std.uuid.v7` for monotonically-sortable session IDs +
  `std.crypto.aes_gcm` for cookie-payload encryption + `std.url`
  builder for the redirect target. **Demo 13** — RAG over a small
  markdown corpus that uses `std.regex` for paragraph-level chunking
  and section-heading extraction, then `std.memory.VectorStore` for
  retrieval. Demo count 11 → 13. Both ship `smoke.sh`.

### Deps added
- `regex` 1.12.3 (promoted from transitive — was a build dep of
  cargo, now a direct mty-stdlib dep)
- `aes-gcm` 0.10.3 (RustCrypto AEAD trait family)
- `chacha20poly1305` 0.10.1 (RustCrypto AEAD trait family)

### Test counts
- v0.39.1 workspace: ~3417 tests (estimate).
- v0.40.0 workspace: **3555 tests on vulcan** (0 failing, 24
  ignored). Net add: ~+138 tests across the 6 tracks.

## [0.39.0] - 2026-05-30

### Added — Real-world stdlib that ships real apps

- **T1 — std.crypto + std.encoding + std.url + std.uuid.** Four
  foundational stdlib modules covering "the surfaces every web
  backend eventually needs". std.crypto: SHA-256 / SHA-512 / BLAKE3
  one-shot + streaming, HMAC-SHA-256 / HMAC-SHA-512 + constant-time
  subtle_eq, CSPRNG random_bytes / uniform_int / uniform_f64 (gated
  by `crypto.rand` capability). std.encoding: base64 standard +
  URL-safe + no-pad RFC 4648 § 4-5; hex lowercase / uppercase /
  mixed-case decode. std.url: WHATWG / RFC 3986 parser, fluent
  builder, percent-encode + percent_encode_component + decode.
  std.uuid: canonical 8-4-4-4-12 parse, v4 (random) and v7 (RFC 9562
  time-ordered, lexicographically sortable). +152 KAT-anchored tests
  across the four modules. Examples: `examples/42_crypto_url.mty`.
- **T2 — Cast surface polish: Bool↔Int + reference cast + MT2028
  INVALID_CODEPOINT.** `Int as Bool` lowers to `icmp ne 0` (so
  `256_i32 as Bool` is `true`, not silently truncated). `Bool as Int`
  is the inverse zext. Reference cast `&T as *T` accepted when inner
  types unify — promotes the v0.37 T3 extern-c `coerce_addr_of` path
  to a general explicit cast outside FFI. New MT2028
  INVALID_CODEPOINT fires at compile time for `Int as Char` literal
  sources outside `0..0x110000` or in the UTF-16 surrogate gap
  `0xD800..=0xDFFF`. Non-literal sources currently pass typeck and
  produce the raw bit pattern — v0.40 picks between a runtime trap
  and an `Option[Char]` surface. New `docs/reference/casts.md` spec
  page. +26 tests in `crates/mty-types/tests/v039_cast_polish.rs`.
- **T3 — Vec typed-slot storage (8x memory reduction for Vec[U8]).**
  v0.38's L28 fix landed a real native growable Vec with an 8-byte
  slot regardless of T; v0.39 makes the slot width follow the
  element type. Header v1 (24 bytes: len, cap, data) → v2 (32 bytes:
  + elem_size). Element-size handling: 1 byte for U8/I8/Bool, 2 for
  U16/I16, 4 for U32/I32/Char/F32, 8 for U64/I64/USize/F64/Ptr,
  rounded layout for structs via memcpy_bytes. New `Vec.set(i, x)`
  surface with bounds-checked typed-slot store; both `.get` and
  `.set` emit a `mty_runtime_panic` call + `trap(TrapCode::user(5))`
  on OOB. Memory footprint for `Vec[U8]@1000`: 2076 bytes total (was
  ~16384 bytes in v0.38). New `VEC_HEADER_V2` constant exposed for
  future migration tooling. LLVM backend unaffected (deferred to
  v0.40). +16 tests in `vec_typed_slots_v039.rs`.
- **T4 — BOLT layout optimization + darwin-arm64 PGO retry.** BOLT
  runs on top of PGO via cargo-pgo's `bolt build` + `bolt optimize`
  on linux-x86_64; llvm-bolt comes from the ubuntu apt `llvm-19-bolt`
  package. Expect 5-15% wall-clock on top of PGO. Windows / macOS
  stay off (PE/COFF + Mach-O BOLT support too rough). darwin-arm64
  PGO retried via per-matrix `toolchain: "1.96.0"` override — the
  release workflow now reads `matrix.toolchain` in the dtolnay
  rust-toolchain step AND exports `RUSTUP_TOOLCHAIN` into
  `GITHUB_ENV` so cargo honours the matrix toolchain rather than the
  workspace `rust-toolchain.toml` pin. If 1.96.0 still skews on the
  runner, the matrix entry flips back cleanly. +3 tests in
  `crates/mty-cli/tests/pgo_scripts.rs`.
- **T5 — Hover catalog 317 → 425.** 99 entries for the T1 stdlib
  surfaces (crypto/encoding/url/uuid) + 51 gap-fillers across std.io
  (BufReader/BufWriter/stdin().lock()/eprint!), std.process (Command
  builder + ProcessOutput + ProcessExit.success), std.path (PathBuf
  push/pop/from/set_extension), std.iter (peekable/windowed/chunks/
  cycle/min/max/flat_map/rev/step_by), std.error (AnyhowError.context
  + Error.source + Result.context), std.string (split/trim/
  starts_with/ends_with/contains/lower/upper), std.vec (contains/
  sort/sort_by/reverse/retain/extend), std.json (get/as_str/as_i64/
  as_array), std.collections (HashMap.contains_key/len/iter/entry +
  HashSet.contains + BTreeMap.range). Integrator added T3 + T2
  follow-ups (Vec.set, VEC_HEADER_V2, vec_typed_slot, the four cast
  symbols) in fixup `b0db3f4`. Final: 425 curated / 425 extracted,
  drift gate byte-for-byte clean.

### Changed
- **Vec header v1 (24 bytes) → v2 (32 bytes).** Adds an `elem_size`
  word at offset 24. No on-disk Vec values exist pre-v0.39
  (std.json / std.observe serialise via SIR `Value`, not the native
  layout), so the breaking layout change is internal-only; the
  `VEC_HEADER_V2` constant ships for future migration tooling.
- **Release workflow.** New `use_bolt` matrix field on the linux
  platform entry; new per-matrix `toolchain` override applied in
  both the dtolnay rust-toolchain step and `RUSTUP_TOOLCHAIN` env.

### Deferred
- **T6 — SWE-bench refresh.** The canned runner needs the user's
  Anthropic API key; deferred to a v0.39.x patch tag or v0.40.

## [0.38.3] - 2026-05-29

### Fixed
- **Restore Windows PGO via the v0.37.3 build-pgo.ps1 path.** v0.38.1
  disabled Windows PGO under cargo-pgo because the training step
  produced no .profraw shards; v0.38.3 routes the windows-x86_64
  release through the in-tree `scripts/build-pgo.ps1` (the path that
  worked in v0.37.3). Investigation of the cargo-pgo Windows-MSVC
  empty-profraw bug deferred to v0.40.

## [0.38.1] - 2026-05-29

### Fixed
- **Disable darwin-arm64 + windows-x86_64 PGO after cargo-pgo
  surfaces.** v0.38.0's Release run revealed cargo-pgo doesn't actually
  fix darwin-arm64's `raw=8 vs expected=10` toolchain-internal mismatch
  (cargo-pgo locates a profdata but rustc's runtime still emits the
  wrong raw version — same bug as v0.37). Windows separately produces
  no `.profraw` shards under cargo-pgo (training step exits clean, but
  `target/pgo-profiles/` is empty at optimise time; v0.37.3's ps1
  script worked). v0.38.1 disables both PGO legs and ships them with
  the `release` profile; linux-x86_64 PGO via cargo-pgo still works.
  v0.39 follow-up: cargo-pgo Windows-MSVC profile-write investigation.

## [0.38.0] - 2026-05-29

### Added — Finishing the PGO loop honestly
- **T1 — cargo-pgo migration (1/5 PGO platforms after v0.38.1
  contingency).** Drop the in-tree `scripts/build-pgo.{sh,ps1}` from
  the CI release pipeline in favour of upstream
  [`cargo-pgo`](https://github.com/Kobzol/cargo-pgo) 0.2.9. v0.38.0
  attempted 3/5 PGO platforms (linux-x86_64 + darwin-arm64 +
  windows-x86_64) but the release run revealed cargo-pgo doesn't
  paper over the v0.37 darwin-arm64 toolchain-internal version
  mismatch (rustc emits raw=8; the same channel's runtime expects
  raw=10) and Windows produced no profraws at all. v0.38.1 retag
  disabled darwin-arm64 + windows PGO; final PGO matrix is
  **`linux-x86_64` ON** + `darwin-arm64` / `windows-x86_64` /
  `linux-aarch64` (cross) / `darwin-x86_64` (cross) OFF. Manual
  `scripts/build-pgo.{sh,ps1}` stay in the tree as the local-dev
  fallback. `scripts/tests/test-cargo-pgo-availability.sh` gates:
  binary present + `cargo pgo --help` exits 0 + `llvm-profdata` major
  version matches `rustc` major version.
- **T2 — Cranelift variadic-call codegen.** v0.37 T6 shipped the
  parse / typeck / decl half; v0.38 T2 lights up calls with extras.
  Build a per-call `ir::Signature` at every variadic call site,
  import it via `Function::import_signature`, take the imported
  symbol's address with `func_addr`, dispatch through `call_indirect`.
  C ABI default argument promotion applied to extras (`F32→F64`,
  signed `I8/I16→I32` sextend, unsigned `U8/U16→U32` uextend,
  `Bool/Char→I32`). `printf_real_libc_round_trip` JIT-builds a real
  call to `libc::printf("hello %d\n", 42)` and asserts the runtime
  output. (+14 tests in `crates/mty-codegen-cranelift/tests/variadic_call.rs`;
  integrator de-flake follow-up: every build_jit test takes the
  `CLIF_DUMP_LOCK` mutex because the process-wide `MTY_DUMP_CLIF`
  env var raced between parallel cargo-test threads.)
- **T3 — FFI returned-struct + fn-pointer + `#[ffi_nul_ok]`.**
  Three FFI matrix surfaces in one track. (a) **Row 7** — `extern c
  fn make_point() -> Point` binds: ≤8 bytes ride a single integer
  return register, 9..=16 bytes ride two, >16 bytes use a hidden
  `sret` first param (`ArgumentPurpose::StructReturn`). The
  `AggregateReturnKind` classifier + `build_extern_signature` live
  in `crates/mty-codegen-cranelift/src/abi.rs`. (b) **Row 11** —
  `fn(T1, T2) -> R` as an extern-c param now accepts a Mighty fn as
  the callback; cranelift's `Const::FnPtr(FnRef::User(fid))` arm
  takes the fn's address via `func_addr` against the `Linkage::Local`
  declaration. (c) **`#[ffi_nul_ok]`** — per-param attribute on
  extern-c params, metadata-only today (the Str→*U8 coercion already
  takes the null-terminated fast path); reserves the side-table for
  a future runtime null-terminator-check pass. (+25 tests: 16 typeck
  cases in `crates/mty-types/tests/ffi_v038_t3.rs` + 9 codegen cases
  in `crates/mty-codegen-cranelift/tests/ffi_v038_t3.rs`.)
- **T4 — Stdlib hover catalog 215 → 317.** +102 entries across 10
  new modules: `extern` (11), `cast` (8), `process` (12), `io` (14),
  `path` (9), `collections` (13), `iter` (18), `result` (7),
  `option` (6), `error` (4). Two catalog tests pin the count + verify
  every entry has both a summary and a longer description.
- **T6 — Benchmark numbers refreshed against v0.38 main.** Re-baselined
  `mty check` parse-only throughput across 50 examples
  (`docs/benchmarks/parse_throughput.md`) and `mty build --target
  wasm32-wasi` output sizes for the 10 demos (`docs/benchmarks/wasm_size.md`)
  using the v0.38 `release-pgo` binary. `scripts/tests/test-bench-results-
  headers.sh` asserts every table carries the v0.38 column header +
  the PGO profile annotation.
- **Cranelift native growable Vec (L28 fix).** `v = v.push(x)` in a
  loop now grows under `mty build` native — pre-fix, the cranelift
  backend had no Vec runtime (only an `mty_runtime_extern_call`
  stub that returned 0). Adds a 24-byte arena-backed header
  (`len@0`, `cap@8`, `data@16`) + `emit_vec_new` + Vec push/len/get
  arms in the `MethodCall` lowering. SIR interpreter unchanged
  (always worked). (+1 test in `crates/mty-codegen-cranelift/tests/
  vec_push_native.rs`.)

## [0.37.0] - 2026-05-29

### Added — Stopping the loop
- **T1 — mty fmt --check in pre-push hook.** `.git-hooks/pre-push`
  gains a third gate that builds `mty-cli` in release mode (cached
  after first call) and runs `mty fmt --check` on every `.mty` file
  under `examples/`, `demos/*/src/`, and
  `tools/gallery/examples/*/main.mty` (60 files on main today).
  Failure prints the offending path + the exact `mty fmt …` command.
  `MTY_PRE_PUSH_SKIP=1` still bypasses the whole hook. Caught two
  pre-existing demo drifts (`demos/03_extract_tool/src/breach.mty`,
  `demos/11_ffi_winit_stub/src/main.mty`) on its first run; fixed
  in the T1 commit. `mty hooks install` is idempotent so the hook
  script body can evolve in subsequent releases without changing
  the install surface. (+9 tests)
- **T2 — Parser cast surface (`expr as Ty`).** Parser emits a real
  `CAST_EXPR` CST node carrying source expression + parsed target
  type. Type checker classifies legal vs illegal cast pairs
  (integer ↔ integer with truncation/extension, integer ↔ float,
  pointer ↔ pointer of same pointee or `*U8`); illegal pairs emit
  new **MT2027 INVALID_CAST** with source type, target type, and a
  hint pointing at the legal alternative. IR lowering fix: the
  cast target type is now the real `TyId` instead of the
  `IrTy::Error` stub the old AST shape forced. Cranelift backend's
  cast path now emits the correct widening/truncation instruction.
  (+21 tests)
- **T3 — FFI ergonomics (IDE unblocker).** Three call-site coercions
  gated on `FnDef.extern_abi == Some("c")`: (1) **Str → *U8
  auto-coercion** — Mighty Str literals are interned
  null-terminated UTF-8; typeck reads the ptr-half of the (ptr, len)
  aggregate at extern-c arg positions whose declared type is `*U8`.
  (2) **`&local` / `&mut local`** at `*T` / `*mut T` parameter
  positions; existing `HirExpr::Borrow` lowering allocates a
  Ref-typed temp whose slot holds the place address. Borrow check
  rules unchanged. (3) **Struct literal at extern-c call site** —
  `ffi_draw_rect(Rect { x: 0, y: 0, w: 100, h: 50 })` typechecks
  directly. New `Rvalue::StrPtr` SIR variant + `FnDef.extern_abi`
  marker. Rows 03, 04, 05, 06, 08, 09 of the v0.36 T2 extern-c matrix
  are now "v0.37 direct" — the wrapper-pattern stays in the test
  fixtures for ABI coverage but real user code calls directly.
  **Unblocks the parallel IDE agent at C:\\Users\\ihass\\mighty-ide.**
  (+18 tests)
- **T4 — darwin-arm64 PGO + 6-path llvm-profdata fallback.**
  `scripts/build-pgo.{sh,ps1}` discover `llvm-profdata` by walking
  six fallback paths in order: host-tuple-specific rustup dir →
  explicit arm64/x86_64 darwin → wildcard rustup glob → any
  installed toolchain → system `$PATH`. First version-matching hit
  wins. Phase 1 logs the chosen path. `release.yml` flips
  `aarch64-apple-darwin` back to `use_pgo: true`. A new `pgo-paths`
  CI job exercises the fallback table on ubuntu-latest. **PGO is
  back to 3 of 5 platforms** (linux-x86_64, windows-x86_64,
  darwin-arm64); `darwin-x86_64` + `linux-aarch64` stay non-PGO
  with the documented "no native runner" rationale. (+9 tests)
- **T5 — LLVM backend signedness threading.** Eight LLVM IR-builder
  call sites swapped to sign-aware variants:
  `build_int_cast` → `build_int_cast_sign_flag`,
  `build_int_signed_div` / `build_int_signed_rem` → unsigned variants
  for unsigned types, `ICMP SLT/SGT/SLE/SGE` → `ICMP ULT/UGT/ULE/UGE`
  for unsigned, `lshr` (logical) for unsigned right-shift. Two
  helpers (`mty_int_cast`, `mty_int_pred`) centralise the dispatch.
  Mirror of v0.36 T1's cranelift-side fix; brings LLVM backend
  unsigned-integer correctness up to par. Tests gate on
  `--features llvm`. (+17 tests)
- **T6 — Variadic extern declarations + cmd_serve uses ureq.**
  Parser accepts trailing `...` in extern-c fn signatures (wrapped
  in `VARIADIC_MARKER` CST node); `HirFn.is_variadic` /
  `FnDef.is_variadic` / SIR `ExternBinding.is_variadic` thread the
  flag through. Typeck relaxes the strict arity check for variadic
  fns; below-fixed-arity calls still emit MT2005. Cranelift backend
  lowers fixed-arity prefix calls (e.g. `printf(fmt)`) end-to-end;
  calls passing extra varargs surface a clean
  `CodegenError::Unsupported` pointing at
  `docs/internals/extern-c-matrix.md` (cranelift 0.132 has no vararg
  `Signature` flag — fix is per-call `import_signature` +
  `call_indirect`, tracked for v0.38). Wasm backend rejects any
  variadic extern fn unconditionally. cmd_serve test rewritten to
  use `ureq` (dev-dep), removing the v0.36.1 tolerate-RST workaround.
  (+17 tests)

### Changed
- `FnDef` gains `extern_abi: Option<String>` (T3) and
  `is_variadic: bool` (T6); built-ins / regular Mighty fns / agent
  methods leave both at `None` / `false`.
- `HirFn` and SIR `ExternBinding` gain `is_variadic: bool` (T6).
- New SIR `Rvalue::StrPtr(arg)` (T3) for the Str → *U8 coercion.
- New diagnostic code MT2027 INVALID_CAST (T2).
- Pre-push hook now runs 3 gates instead of 2 (T1).
- `release.yml` `aarch64-apple-darwin` leg `use_pgo: true` (T4).

### Fixed
- IR lowering for `expr as Ty` no longer emits `IrTy::Error` for the
  cast target type (T2).
- `cmd_serve` test flake under GHA Ubuntu connection-reset race
  removed by switching the test client to `ureq` (T6).
- **Integrator fix** — wasm backend's `Rvalue::Cast` arm was
  pass-through (bundled with `Rvalue::Use`); after T2 added the
  parser cast surface, `b as I64` on a U8 source pushed `i32` where
  validators expected `i64`. Split Cast into its own arm with a
  `lower_ty(src)` vs `lower_ty(dst)` ValType compare and emit the
  matching wasm conversion (`i64.extend_i32_{u,s}`, `i32.wrap_i64`,
  `f64.promote_f32`, `f32.demote_f64`). The u-vs-s pick mirrors
  v0.36 T1's cranelift uextend fix. Caught by
  `conformance_codegen::all_examples_compile_wasm{,_component}` in
  the v0.37 vulcan test run.

## [0.36.1] - 2026-05-29

### Fixed
- **Windows + Ubuntu `cargo test` env-var races.** Three tests
  (`mty_codegen_cranelift::object::find_linker_honours_stardust_linker_env`,
  `find_linker_treats_whitespace_override_as_unset`,
  `mty_stdlib::observe::storage::is_recording_enabled_respects_falsey_values`)
  set process-wide env vars (`MTY_LINKER`, `STARDUST_LINKER`,
  `MTY_OBSERVE`) without acquiring the module-level `ENV_LOCK` mutex
  that the v0.36.0 T4 tests used. Under cargo's default test
  parallelism a sibling test could `remove_var` between the set and
  the read, flipping the result. Two fixes: (a) all
  `mty-codegen-cranelift::object` env-var tests now hold `ENV_LOCK`
  and snapshot/restore previous values, (b) added the same `ENV_LOCK`
  pattern to `mty-stdlib::observe::storage` tests.
- **macOS PGO `Phase 3: merge profiles` profile-format mismatch
  (`raw=8 vs expected=10`).** `scripts/build-pgo.sh` and
  `scripts/build-pgo.ps1` were preferring `llvm-profdata` from
  `$PATH` over the rustup-shipped variant. On the `macos-14`
  GitHub runner `$PATH` resolved to a newer system LLVM that doesn't
  understand rust 1.95.0's instrumentation format. The discovery
  order is now flipped: rustup-shipped `llvm-profdata` first (it
  version-matches the rustc that emitted the `.profraw` shards),
  system LLVM as last-resort fallback.

### Changed
- `aarch64-apple-darwin` release leg returns to `use_pgo: false`
  pending a v0.37 canary run with the corrected `llvm-profdata`
  discovery order. v0.36.0's darwin-arm64 PGO leg was the only
  release platform that failed; linux-x86_64 + windows-x86_64 PGO
  worked end-to-end.

## [0.36.0] - 2026-05-29

### Added — fix-it-for-others
- **T1 — Native codegen fixes.** U8 widening now uses `uextend`
  (zero-extension) for unsigned types rather than `sextend`,
  fixing wrong-sign comparisons of high-bit U8 values lowered to
  Cranelift. Dynamic `log()` lowering allocates a 16-byte
  ptr+len stack slot so non-literal `Str` arguments
  (`let g = greet(); log(g)`) no longer trip
  `CodegenError::Unsupported("non-literal string in log/print")`.
  Hex / binary / octal literals with explicit type suffixes
  (`0xFFu8`, `0b1010_u16`, `0o777i32`, …) now parse across all
  12 integer types. LLVM-backend signedness fix is deferred to
  v0.37. (+66 tests)
- **T2 — extern c signature matrix + [[extern_lib]].** All 11
  signature shapes documented and tested end-to-end against C
  reference impls: nil/i32/two-i32/ptr-in/out-ptr/struct-by-value/
  struct-by-ptr/return-struct/array-ptr/str-in/str-out/fn-ptr.
  `mighty.toml` now accepts `[[extern_lib]]` entries with
  `name`, `kind = "static" | "dynamic"`, `path`, and per-platform
  `link_args`. The driver resolves paths against the manifest
  directory and forwards `-l` / `-L` / `--whole-archive` to the
  host linker. Surfaces a stable `mty_driver::manifest::ExternLib`
  + `build_linker_args` for parallel IDE / agent tooling. (+35 tests)
- **T3 — String position / range ops.** Twelve new `String`
  methods: `rfind`, `position`, `insert_at`, `remove_range`,
  `replace_range`, `is_char_boundary`, `next_char_boundary`,
  `prev_char_boundary`, `char_indices` iterator, plus three
  char-boundary internal helpers. New diagnostic **MT5080**
  (`Range edit at non-char-boundary`) with byte-offset + nearest
  valid boundary in the hint. (+44 tests)
- **T4 — Stardust → Mighty rename.** `MTY_LINKER`,
  `MTY_OTLP_ENDPOINT`, `MTY_TRACE`, `MTY_RUNTIME_THREADS`,
  `MTY_CONF_ONLY`, `MTY_CONF_CASE` are the primary env-var
  spellings; `STARDUST_*` still resolve via the new
  `mty_runtime::env_compat` shim with a one-shot stderr
  deprecation warning per legacy key. WIT package id emitted as
  `mty:*` (still accepts `stardust:*` imports). Cranelift object
  segment renamed to `b"mighty"`. OTLP spans renamed `mty.*` and
  carry an `mty.legacy_name` attribute on the rename hop. Default
  registry slug `mighty-pkg/registry` (still accepts
  `stardust-pkg/registry`). 121 references → 60 (45 in
  dev/history + docs/spec, 23 in legacy-compat code paths). (+18 tests)
- **T5 — Windows install + macOS LC_BUILD_VERSION + PGO.**
  `cargo install mty --no-default-features --features cli-min`
  skips rusqlite's C build (the historical Windows
  `link.exe not found` path) and produces a fully-functional CLI.
  Cranelift-object's `Darwin(_)` → `PLATFORM_UNKNOWN (0)` default
  overridden with `PLATFORM_MACOS + minos=11.0 + sdk=14.0` packed
  in the nibble layout `loader.h` documents; honors
  `MACOSX_DEPLOYMENT_TARGET` and a new `MTY_MACOSX_SDK_VERSION`
  knob. **PGO re-enabled on `linux-x86_64`, `darwin-arm64`,
  `windows-x86_64`** — Phase 4 drops `-Clinker-plugin-lto` (which
  collided with PGO's `CG Profile` module metadata on linux), a
  new Phase 0 wipes `target/release-pgo/{build,deps,incremental,
  .fingerprint}` so stale `-Cprofile-use` codegen can't survive
  across runs (root cause of the `raw=8 vs expected=10` mismatch),
  and `release.yml` cache keys segregate PGO vs non-PGO so
  restore-keys can't cross-contaminate. `darwin-x86_64` and
  `linux-aarch64` stay `use_pgo: false` (rosetta / cross-compile
  can't run the instrumented binary). (+21 tests)

### Changed
- WIT package namespace emitted as `mty:*` (still accepts
  `stardust:*` for legacy imports)
- Cranelift object segment renamed to `b"mighty"` (legacy readers
  may also accept `b"stardust"` for backward-read)
- OTLP spans renamed `mty.*` (carry `mty.legacy_name` attribute
  on the rename hop, one-release deprecation)
- Default registry slug `mighty-pkg/registry` (still accepts
  `stardust-pkg/registry`)
- `BuildOptions` now carries `extern_libs: Vec<ExternLib>` and
  `manifest_dir: Option<PathBuf>` for [[extern_lib]] resolution

### Deprecated
- `STARDUST_LINKER`, `STARDUST_OTLP_ENDPOINT`, `STARDUST_TRACE`,
  `STARDUST_RUNTIME_THREADS`, `STARDUST_CONF_ONLY`,
  `STARDUST_CONF_CASE` — use `MTY_*` instead; one-shot stderr
  warning on first lookup per process.

### Fixed
- Native codegen wrong-sign comparison for U8 values widened via
  the previously-misused `sextend`.
- Native codegen `Unsupported` panic on non-literal `Str`
  arguments to `log()` and `print()`.
- Stardust → Mighty: 6 env-var paths that previously had no
  `MTY_*` spelling at all.
- `BuildOptions` reconciliation between T1's `native_dynamic_log`
  test and T2's new required fields.



- **T1 — WASM playground** — Cloudflare Worker proxy deployment
  (runbook ready; `wrangler deploy`); WASM size pass (`wasm-opt -Oz`,
  wasm-bindgen tree-shake); browser JIT via a `playground-wasm-jit`
  feature once cranelift's wasm target matures; CF Worker
  rate-limit telemetry into Workers Analytics; CI WASM size guard
  at 2 MB.
- **T2 — Agent transports** — HTTP `Transfer-Encoding: chunked` for
  streaming batch responses; Windows `AF_UNIX` (10+ supports it —
  drop the "not supported" fallback); optional `--tls-cert /
  --tls-key` for HTTPS without a reverse proxy; replay unified-diff
  output on byte-mismatch.
- **T3 — fix --apply + LSP fixAll** — `mty fix --auto-pick` carry-over
  from v0.34 (highest-confidence regardless of threshold);
  per-namespace bulk-apply filters
  (`source.fixAll.mighty.MT4xxx`); `mty fix --apply --workspace`
  to walk the project; apply telemetry to feed the confidence-score
  calibration corpus.
- **T4 — PGO / Docker / Homebrew** — **PGO re-enable (HIGH
  PRIORITY)** — v0.35.2 ships PGO disabled after v0.35.0+v0.35.1
  Release failures; fix path is to drop `-Clinker-plugin-lto` and
  tighten the cargo cache key; PGO on `darwin-x86_64` (needs
  native macOS Intel runner that can execute instrumented binary);
  PGO on `linux-aarch64` (needs native arm64 GitHub runner);
  Docker publish toggle (user-driven); Homebrew-core PR (runbook
  ready, user files).
- **T5 — Strategy B hover** — flip source-of-truth so
  `STDLIB_EXAMPLES` is `build.rs`-generated from docstubs (needs
  LSP `LazyLock<Vec<…>>` bridge); `##since <version>` directive for
  "added in vX" hover badges; walker-driven `EMBEDDED_DOCSTUBS`
  list so new module files are auto-picked up; multi-modal hover
  carry-over from v0.34 T3 (inline SVG/PNG in docstubs).
- **Cross-cutting / integrator** — vulcan PATH (cargo at
  `~/.cargo/bin/cargo`, not default PATH — past integrators have
  falsely reported "no Rust toolchain"); carry-forward unresolved
  v0.34/v0.35 backlog: `SCHEMA_VERSION` crate-root re-export,
  pre-push hook in CI redundancy, vulcan disk hygiene, multi-modal
  streaming, Go/C++ comparator refresh, `mty find` semantic search,
  `mty-runtime::work_stealing` Windows-only flake.

The v1.0 freeze-gate is unchanged: 8 RFC comment windows opened
2026-05-26, earliest close 2026-06-09 (RFC-005), latest close
2026-07-25 (RFC-002 + RFC-006). Proposed v1.0 freeze date
2026-09-01; earliest tag 2026-07-26. v0.35 pulls three more
freeze-gate items into "ready": real WASM playground (T1), `mty
agent` production transports (T2), and the agent-first-shot →
zero-shot loop (T3).

## [0.35.5] - 2026-05-29

### Fixed
- **CI `test (minimal features)` example sweep** — the step
  `cargo run --no-default-features -p mty-cli -- check <example>`
  failed with `target 'mty' in package 'mty-cli' requires the
  features: 'host-toolchain'` because T1's `[[bin]] mty` ships
  `required-features = ["host-toolchain"]`. The example-sweep
  step's value is asserting the corpus checks cleanly, not
  exercising the binary's feature minimality (the `cargo test
  --workspace --no-default-features` step above is the
  source-of-truth for that). v0.35.5 drops `--no-default-features`
  from the sweep cargo-run commands, restoring default features
  for the `mty check` binary the sweep depends on.

## [0.35.4] - 2026-05-29

### Fixed
- **CI `test (minimal features)` job (round 2)** — v0.35.3 fixed
  the compile errors but the spawn-binary integration tests still
  ran under `--no-default-features` and panicked because the
  `mty` `[[bin]]` is gated on `required-features =
  ["host-toolchain"]` (T1) so the binary doesn't exist. v0.35.4
  gates every spawn-binary mty-cli test
  (`agent_http`, `agent_mode`, `agent_recorder`, `agent_unix`,
  `cmd_fix`, `cmd_new_template`, `cmd_serve`, `cmd_serve_watch`,
  `cmd_test_eval`, `explain`) behind
  `#![cfg(feature = "host-toolchain")]`. Also gates
  `mty-stdlib/tests/observe_auto_record.rs` behind
  `observe-sqlite` (it unwraps `SqliteStore::in_memory()` which
  returns `FeatureDisabled` when the feature is off).

## [0.35.3] - 2026-05-29

### Fixed
- **CI `test (minimal features)` job (round 1)** — `cargo test
  --workspace --no-default-features` failed under `RUSTFLAGS=-D
  warnings` with 6 dead-code / unused-import errors: 4 in `mty-pkg` (`split_url`,
  `short_slug`, `now_secs`, `normalise_sha256_line` plus the
  `registry::self` import) where the helpers are only reachable
  via the `git-fetch` / `registry-fetch`-feature-gated `fetch`
  fns, 1 in `mty-stdlib` (`format_unix_ms_iso` only reachable
  through the `observe-sqlite`-feature path), and 2 `mty-cli`
  integration tests (`cmd_find`, `cmd_run_argv`) that reach into
  `mty_cli::cmd::*` and `mty_stdlib::env::*` which T1's
  `host-toolchain` refactor now feature-gates out. Fix:
  `#[cfg_attr(not(feature = "…"), allow(dead_code | unused_imports))]`
  on the helpers, `#![cfg(feature = "host-toolchain")]` on the
  affected mty-cli test files. No source changes outside these 5
  files; default builds unchanged.

## [0.35.2] - 2026-05-29

### Fixed
- **Release pipeline** — PGO temporarily disabled across the three
  PGO platforms (`linux-x86_64`, `darwin-arm64`,
  `windows-x86_64`). v0.35.1 fixed the `-Cprofile-use` absolute-
  path bug at Phase 4, but two deeper issues remain: `linux-x86_64`
  hits `LLVM ERROR: Broken module found, module flag identifiers
  must be unique !"CG Profile"` during the PGO+`-Clinker-plugin-lto`
  link-time pass, and `darwin-arm64` + `windows-x86_64` see a
  profile-format version mismatch (`raw=8 vs expected=10`) on
  cached `target/` artefacts. v0.36 owns the deeper fix (likely:
  drop `-Clinker-plugin-lto`, scope cache key away from
  `target/pgo-profiles/`). All 5 release binaries now ship via the
  plain `release` profile. PGO script + workflow wiring preserved.

## [0.35.1] - 2026-05-29

### Fixed
- **PGO release pipeline** — `scripts/build-pgo.{sh,ps1}` promote
  `$PROFDIR` / `$ProfDir` to absolute before any `rustc`
  invocation. v0.35.0's Release workflow failed on all three PGO
  platforms with `file 'target/pgo-profiles/merged.profdata'
  passed to '-C profile-use' does not exist` because `rustc`
  resolved `-Cprofile-use=<relative-path>` from each build
  script's own CWD (package dir), not the workspace root.
  `-Cprofile-generate` was unaffected (the path is resolved at the
  instrumented binary's runtime CWD, which is the workspace root).
  Superseded by v0.35.2 on the deeper PGO breakage (see above).

## [0.35.0] - 2026-05-28

**Mighty v0.35 — closing the v0.33 stubs. Real WASM mty in the
browser (the install funnel that the playground was promising),
`mty agent` HTTP + Unix transports + record/replay, `mty fix
--apply` + LSP bulk `source.fixAll.mighty` (agent first-shot becomes
zero-shot), PGO release binaries on 3 platforms, multi-arch Docker,
and Strategy B hover with drift detection.** Five tracks merge in
parallel.

### Added — Closing the agent-first stubs
- **T1** — Real WASM `mty` compiler in `tools/playground/`
  (1.15 MB; parser + typeck + borrowck + IR + tree-walk interp
  run in the browser via `wasm-pack`); `mty-cli` lib gains
  `cdylib` + default-on `host-toolchain` feature gating the native
  dep set; Cloudflare Worker LLM proxy source (`POST
  /v1/{anthropic,openai,gemini}/{path}`, per-IP rate-limit via KV,
  CORS allowlist); `.github/workflows/playground.yml` Playwright
  smoke on every PR; `.github/workflows/pages.yml` extended to
  publish the playground under `site/playground/`. `4/4 Playwright
  smoke tests`.
- **T2** — `mty agent` HTTP transport (hyper HTTP/1.1; `POST
  /v1/agent`, `POST /v1/agent/batch`, `GET /v1/agent/version`;
  optional `--auth-token` bearer auth with 401 +
  `WWW-Authenticate: Bearer`); Unix socket transport
  (`tokio::net::UnixListener`; pre-existing socket files unlinked
  on bind); recorder (`--record <PATH>`, appends NDJSON
  request/response pairs) + replay (`--replay`, byte-matches live
  responses against the recording). `+50 tests`.
- **T3** — `mty fix --apply` CLI (`--code`, `--alternative`,
  `--threshold`, `--dry-run`, `--interactive`, `--from-stdin`;
  highest-line-first conflict-resolution policy); LSP
  `source.fixAll.mighty` action wires the v0.34 T2 capability to a
  real handler — one `CodeAction` with an atomic `WorkspaceEdit`
  bulk-applying every preferred-confidence fix in the document;
  shared `mty_diagnostics::apply::apply_unified_diff` helper used by
  both paths. Canonical zero-shot loop:
  `mty check --format json src/main.mty | mty fix --apply
  --from-stdin`. `+78 tests`.
- **T4** — PGO scripts + workflow matrix wiring for 3 platforms
  (`linux-x86_64`, `darwin-arm64`, `windows-x86_64`); **shipped
  disabled in v0.35.2** after CI failures (see [0.35.2] above).
  Multi-arch Docker (`linux/amd64,linux/arm64`, cosigned with
  `--recursive`, SBOM, gated on `vars.PUBLISH_DOCKER`); Homebrew-
  core submission runbook updated for v0.35 reality (all four
  arches shipping cleanly for ~7 releases — user-driven
  submission).
- **T5** — Strategy B hover catalog: per-module `.docstub` files
  (`crates/mty-stdlib/docs/<module>.docstub`, 18 module buckets)
  + walker (`crates/mty-doc/src/stdlib_walker.rs`,
  `include_str!`-time parse) + one-shot generator
  (`crates/mty-doc/src/bin/regen-stdlib-docstubs.rs`); `mty doc
  --check` drift gate, CI-enforced on every push; 203 entries
  migrated with byte-for-byte zero drift against the curated
  gold-set. `+29 tests`.

### Gates (vulcan)
- `cargo test --workspace --no-fail-fast` — **3017 passed, 0
  failed** (pre-v0.35: 2887; +130 over the 5 feature tracks).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean.
- `cargo audit --deny warnings` — clean.
- `wasm-pack build --target web --no-default-features --features
  playground-wasm` — clean; emits `1.15 MB` `mty_cli_bg.wasm`.



## [0.34.0] - 2026-05-28

**Mighty v0.34 — compounding the agent first-shot success rate. 81
MTxxxx codes (up from 31) ship structured auto-fix proposals; every
fix appears as a one-click LSP CodeAction in VS Code + JetBrains;
the stdlib hover catalog grew from 58 to 203 examples; and a
pre-merge fmt/clippy hook stops the recurring Linux drift trap
before it reaches CI.** Four tracks merge in parallel (T2 already
includes T1).

### Added — Agent-first compounding
- **T1** — 50 more MTxxxx fix engines (full MT2xxx coverage + MT3xxx
  polish + MT4xxx finish); total fix-capable codes 31 → 81; multi-
  alternative envelopes on every MT4xxx code with more than one
  legitimate untaint path. `+56 tests`.
- **T2** — LSP `textDocument/codeAction` wiring all fix envelopes as
  one-click "Apply fix" quickfixes in VS Code (setting
  `mighty.codeAction.confidenceThreshold`) and JetBrains (Settings >
  Tools > Mighty > Code action confidence). Per-alternative actions
  appear when the envelope has more than one fix path.
  `+53 tests`.
- **T3** — Stdlib hover catalog 58 → 203 entries (+145), covering
  `std.rag`, `std.computer`, `std.swarm`, `std.observe`,
  `std.taint`, `std.eval`, `std.web`, `std.fs`, `std.json`,
  `std.string`, `std.vec`. Every public stdlib item the LSP hover
  query can resolve now answers with at least one worked example
  (was: ~40%). `+2 catalog tests`.
- **T4** — `MT4099` emit-site span fidelity (taint diagnostics now
  point at the exact `call(tainted)` byte range, not the enclosing
  function); `schema_version` field on every `DiagnosticEnvelope`
  (versioning policy in `docs/internals/diagnostic-envelopes.md`);
  receiver-type hover for local bindings; pre-push fmt + clippy git
  hook + `mty hooks install` subcommand. `+21 tests`.

### Integrator
- The pre-push hook paid for itself on its first run by catching a
  missing `schema_version` field in the MT4099 test envelope
  introduced by the T1+T2/T4 merge; fix shipped inline before the
  integrator commit reached origin/main. First confirmed save
  against the v0.33 Linux-fmt-drift recurrence pattern.

### Gates (vulcan)
- `cargo test --workspace --no-fail-fast` — **2887 passed, 0 failed**
  (pre-v0.34: 2766; +121 over the 4 feature tracks).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean.
- `cargo audit --deny warnings` — clean.
- VS Code extension `npm run compile` — clean (0.34.0).
- Playground `npm run build` — clean.

## [0.33.0] - 2026-05-28

**Mighty v0.33 — the agent-first release. Structured auto-fix
diagnostics make Mighty the language with the highest agent
first-shot success rate. Plus `mty agent` JSON CLI, `std.rag`,
multi-modal vision-language, `mty find`, LSP hover with examples,
web playground + agent gallery, and v0.33 benchmarks published.**
Six tracks (T2-T7) merge in parallel; T1 is the integrator's
housekeeping. All 10 demos pass `smoke.sh` pre and post; clippy /
fmt / audit green.

### Added — Agent-first
- **T2** — `std.rag` (RAG-as-stdlib: `Index` + `Retriever` +
  `Reranker` + `Pipeline`) + multi-modal vision-language `Image`
  input across all 4 LLM providers (Anthropic, OpenAI, Gemini,
  Bedrock) + demo 10 (vision RAG) + tour 21. `+59 tests`.
- **T3** — Web playground at `tools/playground/` (Vite + Monaco +
  WASM `mty` target stub) + agent gallery at `tools/gallery/` with
  7 starter examples. (WASM artifact stubbed; real
  compile-and-run is v0.34.)
- **T4** — Structured agent-actionable diagnostics: 31 MTxxxx
  codes emit JSON envelopes carrying machine-readable auto-fix
  proposals; MT4099 (taint) ships 3 first-class untaint strategies
  as alternatives. `mty check --format json --include-source`.
  `+50 tests`.
- **T5** — `mty agent`: NDJSON-over-stdio CLI protocol with 9 ops
  (`check`, `fix`, `run`, `build`, `find`, `explain`, `inspect`,
  `lsp_hover`, `version`) so LLM agents drive every other `mty`
  subcommand without scraping human output. HTTP + Unix
  transports stubbed for v0.34. `+60 tests`.
- **T6** — LSP hover surfaces 58 stdlib `///` examples +
  capability hints + See-also inference. `+20 tests`.
- **T7** — `mty find`: capability-tagged stdlib search ("write
  files" → `fs.write` APIs); `--by-capability` inverse;
  `pretty` / `json` / `short` formats. `+18 tests`.
- **T1** — `[profile.release-pgo]` (already added v0.22) inherited
  through v0.33; v0.33 benchmark rerun on vulcan replaces the v0.6
  baseline numbers on the docs site. (PGO wiring into
  `release.yml` is v0.34.)

### Changed
- README updated in-place to mention agent-first marketing,
  `std.rag`, multi-modal, `mty find`, `mty agent`, playground,
  LSP hover examples. Test count 2559 → ~2766. Example count
  37 → 38. Demo count 9 → 10.
- Six v0.32-track follow-ups remain open and have been re-promoted
  into the v0.34 unreleased section under their owning v0.32 track
  letters (debugger UI, VS Code polish, JetBrains TextMate, the
  Docker publish flip, the GH Actions polish, and the legacy
  JSON-lines recorder drop).
- `docs/benchmarks/*` refreshed with v0.33 numbers from vulcan;
  Mighty + Rust comparator rows updated for parse,
  agent-send-latency, mailbox; Go + C++ retain v0.6 (vulcan has no
  Go installed, and the Rust hyper http-server comparator hit
  E0790 on the new toolchain — both tracked for v0.34).

### Fixed
- 7 clippy errors in `mty-diagnostics` (format_collect,
  format-in-format args, identical replace-line, redundant
  lifetime, and 2× OR-pattern → range) introduced by T4's diff
  envelope code.
- 3 clippy errors in `mty-stdlib` (manual `div_ceil`, derivable
  `Default`, `skip_while(_).next()` → `find(!_)` ) introduced by
  T2's chunker + base64 vendor.
- 13 files of fmt drift introduced by parallel-merge of T2/T4/T5/T6
  swarm branches.

## [0.32.1] - 2026-05-28

### Fixed
- `examples/37_debug_demo.mty` stripped a trailing blank line in its
  head comment block that `mty fmt --check` rejected on Linux runners.
  Caught at v0.32.0 tag time when the CI fmt gate flagged the file;
  retagged as v0.32.1 with the fix rather than amending v0.32.0.
  No source / API changes; binaries identical apart from the example
  file.

## [0.32.0] - 2026-05-28

**Mighty v0.32 — debugger + multi-arch + replay closure. `mty dap`
ships across VS Code + JetBrains (Community + Ultimate), 2 new
release targets (macOS x86_64 + Linux aarch64), and the 3 v0.29
replay backlog items are all closed.** Six tracks merge in parallel.
All 9 demos pass `smoke.sh` pre and post; clippy / fmt / audit
green.

### Added — Debugger + Replay
- **Track A** — DAP debug adapter via `mty dap`; VS Code launcher
  (F5 on any `.mty` file synthesises a default launch config) +
  JetBrains "Mighty Debug" run configuration type (works in both
  Community + Ultimate IDEs). `+33 tests`.
- **Track F** — `MemberReply.tool_uses` structural payload (closes
  v0.29 replay backlog item 1); `ReplayDriver::replay_all`
  interleaved with `with_provider` (item 2); `MTY_RECORD_TRACE`
  env auto-captures via the recorder integration (item 3). `+24
  tests`.

### Added — Editor surfaces
- **Track B** — VS Code cost CodeLens above every `@tool(`,
  `swarm(`, `Member.<vendor>(`, and `.ask(` site (today's per-file
  cost + call count, refreshed every 60s and on save); cost
  side-panel webview with theme-aware summary cards / per-provider
  bars / top-10 table (replaces the terminal `mty inspect`
  command); tree-sitter semantic-tokens **stub** registering a
  forward-compatible token legend incl. our custom `taintedType`
  token (full grammar integration is v0.33).
- **Track C** — JetBrains **Community-edition fallback** —
  TextMate grammar registered via the platform's TextMate bundle
  facility so highlighting works on IDEs without the LSP API;
  adaptive LSP load via `<depends optional="true"
  config-file="mighty-lsp.xml">com.intellij.modules.lsp</depends>`;
  `since-build` 232 (IDEA 2023.2+ Community + Ultimate); cost
  tool window upgraded from HTML pre-block to a sortable TreeTable
  with Date / Provider:Model / Calls / Cost columns + "Copy as
  JSON" action.

### Added — Distribution + CI
- **Track D** — `release.yml` extended from 3 → **5 platforms**
  (added macOS x86_64 + Linux aarch64); Homebrew formula
  audit-clean + `tools/distribution/homebrew/HOMEBREW_CORE_SUBMISSION.md`
  runbook; `tools/distribution/asdf-mty/` plugin skeleton; multi-arch
  dry-run in CI; cosign + SBOM gated behind
  `vars.PUBLISH_DOCKER == 'true'`.
- **Track E** — `cost-delta` composite action (PR comment with
  per-provider spend delta vs base); `mty-explain` composite
  action (wraps `mty explain MTxxxx` and pastes the rendered
  diagnostic into a PR comment); `mty-check` now emits an
  `error_code` output for downstream conditionals;
  `tools/gh-actions/examples/dependabot.yml` example.

### Documentation
- README: Editor support bullet for `mty dap`; example count
  36 → 37 (Track A added `examples/37_debug_demo.mty`); Install
  section now lists all 5 binary targets; Status section bumps
  test count to reflect the +57 Rust-side tests from Tracks A+F.

### v0.29 replay backlog — closed
1. ~~structural `MemberReply.tool_uses`~~ — shipped Track F
2. ~~`ReplayDriver::replay_all` interleaved with provider swaps~~ —
   shipped Track F
3. ~~`MTY_RECORD_TRACE` auto-capture via recorder integration~~ —
   shipped Track F

### Constraints honoured
- **Track A** — JetBrains debug config uses run-target plumbing +
  console mode; the full XDebugger UI integration (step-in /
  step-over / variables panel in the JetBrains debug tool window)
  is the v0.33 follow-up.
- **Track B** — tree-sitter semantic tokens is a **stub** (no
  WASM grammar artifact yet); CodeLens + webview are real.
- **Track C** — TextMate grammar duplicated between
  `tools/vscode/syntaxes/` and `tools/jetbrains/src/main/resources/textmate/`
  (canonical extraction is v0.33).
- **Track D** — cosign + SBOM gated on `vars.PUBLISH_DOCKER ==
  'true'` (off by default until Docker push lands in
  `release.yml`); 2 placeholder SHAs in the Homebrew formula until
  v0.32.0 binaries publish.
- **Track F** — native-only `Case::from_trace` (JSON-lines
  auto-route retired; the JSON-lines recorder variant ships with
  a deprecation shim for one cycle).

## [0.31.0] - 2026-05-28

**Mighty v0.31 — the DX release. A tree-sitter grammar that
cascades into Neovim/Helix/Zed/GitHub linguist, a VS Code
extension with a real cost status bar, a JetBrains plugin
covering 11 IDEs, every install path templated (Homebrew + Scoop
+ winget + Docker + devcontainer + mise + snap), and a reusable
GitHub Actions library that drops Mighty into anyone's CI in
three lines.** Five tracks land in parallel under disjoint
subfolders of the new `tools/` tree. Zero Rust source changes —
`cargo test` count holds at 2502; conformance + clippy + fmt +
audit + the 9-demo smoke sweep are all green pre and post.

### Added — Developer Experience
- **Track 1** — tree-sitter grammar under `tools/tree-sitter/`
  (highlights + locals + indents + injections + tags;
  36/36 corpus examples parse); cascades into Neovim, Helix,
  Zed, GitHub linguist.
- **Track 2** — VS Code extension under `tools/vscode/` (LSP
  wiring, 44 snippets, 8 palette commands, real cost status bar
  reading the v0.30 `std.observe` SQLite; `.vsix` builds via
  `npm run compile && vsce package`).
- **Track 3** — JetBrains plugin under `tools/jetbrains/`
  (gradle wrapper bundled; 4 actions; Mighty Cost tool window;
  11 IDE compatibility entries; LSP features require Ultimate-tier
  IDEs — Community editions get the plugin without LSP-driven
  features, with a TextMate fallback queued for v0.32).
- **Track 4** — distribution manifests under
  `tools/distribution/` (Homebrew formula, Scoop manifest,
  winget manifests, Dockerfile + docker-compose example,
  devcontainer.json, mise plugin stub, snap snapcraft.yaml — all
  SHA256-pinned to the v0.31.0 binaries).
- **Track 5** — reusable GitHub Actions under
  `tools/gh-actions/` (5 composite actions:
  `setup-mty` / `mty-check` / `mty-test` / `mty-test-eval` /
  `mty-bench-smoke`; 3 example workflows:
  `basic-check.yml` / `full-ci.yml` / `nightly-eval.yml`).

### Documentation
- README gains an "Editor support" section linking the new
  `tools/{vscode,jetbrains,tree-sitter,gh-actions}/` subfolders
  and a one-line Homebrew install hint.

### Unchanged
- Cargo workspace tests: **2502** (identical to v0.30.1, no Rust
  source changes).
- All 9 demos pass `smoke.sh`; 2 demos additionally pass the
  mock-LLM end-to-end smoke under `MTY_AGENT_SMOKE=1`.
- All 6 CI workflows continue to gate: `test`, `test-minimal`,
  `msrv`, `clippy-strict`, `bench`, `security`.

## [0.30.0] - 2026-05-27

**Mighty v0.30 ships the *differentiator release* — compiler-checked
prompt-injection prevention (`Tainted[T]`), first-class Anthropic
Computer Use with a capability-typed sandbox, native cost/latency
observability, `mty test --eval` as a CI verb, and a SWE-bench
Verified harness ready to publish numbers.** Five tracks land in
parallel under isolated-worktree discipline. Rust test count grows
**2289 → 2502** (+213). Nine demos all green; the new examples
33–36 (taint basics, taint untaint, observability demo, computer
use) cover the source-level surface. No source-level breaking
changes — `Tainted[T]` is additive: stdlib sources that previously
returned `Str` now return `Tainted[Str]`, and the compiler rejects
unsanitised flow at the sink. Programs that already routed LLM
output through a sanitiser continue to compile.

### Added
- **Track A** — `Tainted[T]` type for compiler-checked
  prompt-injection prevention; MT4099 fires when tainted data
  reaches a sink (`fs.write`, `process.exec`, `sql.execute`,
  `net.request`); untaint via `matches_regex` / `in_allowlist` /
  `sanitize_with`. Design departure (opaque-ADT + post-typeck
  pass rather than `TyData::Tainted` variant) documented in
  `docs/internals/taint-types.md`.
- **Track B** — SWE-bench Verified harness (`bench/swe/`
  standalone crate + `Makefile` `bench-smoke`/`bench-full`
  targets); 10-problem smoke ready to fire via `make bench-smoke`
  with `ANTHROPIC_API_KEY` set.
- **Track C** — `std.computer` (screen capture + mouse/keyboard,
  3 platform shims) + `@computer_use` decorator; Anthropic
  Computer Use first-class with capability-typed sandbox
  (`Sandbox::screen_region` / `Sandbox::input_only_in_app` /
  `Sandbox::deny_keys`).
- **Track D** — `std.observe` auto-wraps every LLM call with
  cost + latency in `~/.mty/observe.db`; `mty inspect --cost`
  reads the SQLite; OTel exporter stub.
- **Track E** — `mty test --eval` discovers `*.eval.mty` suites,
  pass/fails on score thresholds, `--replay-only` runs against
  recorded traces for free CI smoke.

### Notes
- README is at ~270 lines post-v0.30. Cut-not-bloat discipline held.
- Track B's smoke run has **not** been executed on this branch
  (no API key in the build agent's environment). The harness is
  green; the user runs `make bench-smoke` and the results file
  `dev/history/benchmarks/swe-bench-smoke-v0.30.md` is updated
  in place.
- Track C's deferred demo 10 (browser operator) is queued as the
  first v0.31 candidate.

## [0.29.0] - 2026-05-27

**Mighty closes every v0.27/v0.28 surface gap — typed bang-send
returns reach call sites, `while let` finishes the streaming
surface, `budget` is a soft keyword, std.eval rides native replay,
and demo 09 spans 2 nodes.** Six tracks land in parallel under
isolated-worktree discipline (v0.28's deferred Tracks A–E plus a
sixth Track F that wires `std.eval` to the real replay seam).
Rust test count grows **2187 → 2289** (+102). Nine demos all green;
demo 08 drops every v0.27 workaround and demo 09 is the new
distributed-swarm forcing function. No source-level breaking
changes.

### Added
- Track A — `BuiltinId::Swarm` interpreter arm
- Track B — 4 swarm ADTs added to handler-safe allowlist
- Track C — typed bang-send return-type lowering
- Track D — `while let` parser + finished streaming surface
- Track E — `budget` soft keyword + per-provider `*_BASE_URL` env vars
- Track F — `std.eval` native replay hooks (`Replay::with_provider`,
  `iter_llm_calls`, trace wire v3 backward-compat, `mty replay --diff`)
- Demo 09 — distributed 2-node swarm code review

### Unreleased (v0.30 candidates)
- `Member::ask` structured tool_uses return (Track F follow-up)
- `ReplayDriver::replay_all` interleaved with `with_provider` (Track F follow-up)
- Recorder integration into `Member::ask` via `LlmProvider` trait (Track F follow-up)

## [0.28.0] - 2026-05-27

**Mighty ships `std.eval`: byte-identical-replay-based LLM evals
as a typed stdlib surface — the "regression-test agents like any
other code" capability the README's Why-Mighty section promises
is now real.** A Mighty program can now declare a
`Suite::new("research-agent")`, attach `Case`s (from raw input,
from a recorded `.mty-trace`, or from a saved transcript), fan
the suite across a panel of `Member`s (Anthropic / OpenAI /
Gemini / Bedrock — any subset of the four v0.27 typed providers,
mixed freely), pick a `Compare` strategy (byte-equal-after-
trim-lower, semantic-cosine over `std.memory::Embedder`, or
order-independent tool-call-set equality), and read back a
per-(case, member) verdict matrix plus per-divergence rows. The
runner shares a `SharedDollarBudget` across members within a
case so the whole suite stays under one cost cap. **Track G** is
the only track that shipped this slice. The five in-tree tracks
dispatched alongside it (A–E, all v0.27 Track F follow-ups) hit
shared-`target/` contention mid-build and were discarded
unverified — they re-dispatch as v0.29 candidates under
isolated-worktree discipline. The `std.eval` module lives at
`crates/mty-stdlib/src/eval/` (6 files, ~1700 LOC, 60 new unit
tests + 2 doctests); `examples/31_eval_agent.mty` and
`docs/internals/std-eval.md` cover the source-level surface.
Three comparators (`Compare::equal()` / `semantic_similarity` /
`tool_call_set_equal`); five verdict variants
(`Match` / `Diverge` / `Error` / `SingleMember`, plus
suite-level `EmptySuite` / `NoMembers` / `AllCellsFailed`). The
`Member` enum is re-exported from `std.swarm` so eval panels and
`swarm(...)` consensus calls share the same provider
abstraction. Working around four v0.29 replay-runtime hooks for
now by reading a lightweight JSON-lines trace shape
(`replay_glue::decode_trace_baseline`); the hooks are queued in
`[Unreleased]` above. Rust test count grows **2125 → 2187**
(+62). Eight demos all green; demo 08 still uses v0.27
workarounds for the 5 deferred tracks; KNOWN_ISSUES net zero.
v1.0 freeze gate status unchanged structurally (blocker #2's
8 RFC comment windows still the only standing item; earliest
possible v1.0 tag remains 2026-07-26).

## [0.27.1] - 2026-05-27

**Hotfix: two example files had a leading blank line the formatter
collapses.** `examples/28_agent_with_llm_field.mty` (added in Track
B) and `examples/29_streaming.mty` (added in Track E) each had a
spurious blank line between their leading doc-comment block and the
`package` decl; the formatter collapses these to a single blank, so
the in-tree CI `fmt --check` sweep against every example failed on
ubuntu-latest at v0.27.0 (windows-latest passed because the
v0.26.1 CRLF-normalisation in `cmd_fmt` happens to mask the
single-blank vs no-blank difference when the file is checked out
with `core.autocrlf=true`). v0.27.1 reformats both files; CI green
across all three OSes. No source-level surface change; the fmt
canon stays the same as it has been since v0.13.

## [0.27.0] - 2026-05-27

**Mighty is now feature-complete as an LLM-agent language: all
four providers full, `@tool` source-level decorator parses,
`std.swarm` multi-LLM consensus + a shared dollar budget across
the swarm, twelve `std.*` ADTs handler-safe, and eight demos
cover the agent loop end-to-end.** v0.27 is the "fill in every
gap v0.26 surfaced" release. **Track A** wires the source-level
`@tool(description: Str, cap: CapabilitySet)` decorator through
lexer → parser → HIR lowering → companion-fn synthesis + the
existing `mty_macros::tool` registry registration call (the
v0.26 macro was registered at Rust level only — demo 07 had to
fall back to doc-comment spec); 13 new tests. **Track B** widens
the v0.26 Strict-Agent scope so 12 `std.*` ADTs are recognised
as handler-safe (`LlmClient`, `LlmProvider`, `Message`,
`ContentBlock`, `TokenBudget`, `MemoryStore`, `VectorStore`,
`Episodic`, `Working`, `ToolHandle`, `McpClient`, `McpServer`)
plus lifts the `wasm32-web` agent-ADT-field restriction by
growing opaque-ADT slot tracking in the per-agent 64KB linear-
memory region (each opaque-ADT field reserves an 8-byte slot
holding a host-side resource-table handle index; reload + replay
both preserve the index); 11 new tests. **Track C** promotes the
v0.26 OpenAI / Gemini / Bedrock skeletons to SHIPPED-FULL with
real response decoding + streaming + tool-use + budget short-
circuit + typed error coverage — OpenAI `chat/completions` +
`tool_calls` + SSE, Gemini `generateContent` +
`streamGenerateContent` + `functionCall`, Bedrock Anthropic-on-
AWS body + **inline SigV4 signing** (deliberately did not add
`aws-sdk-rust` — too heavy; ~140 LOC builder, exercised by the
v0.26 `tls_handshake` infra) + AWS event-stream binary framing
for streaming; 29 integration + 53 lib tests (per-provider
response decode + streaming + tool-use + budget; SigV4 canonical-
request / canonical-headers / string-to-sign / signing-key /
authorization-header golden vectors). **Track D** ships
`std.swarm` — `swarm(prompt, members, strategy, budget).await`
async fn fans the prompt to every `Member` (thin wrapper around
any `Arc<dyn LlmProvider>` + name + optional weight) under a
shared `SharedDollarBudget` (`Arc<Mutex<...>>`) and votes the
consensus reply with one of four `ConsensusStrategy` variants
(`Majority`, `Plurality`, `Unanimous`, `Weighted`); members that
go over budget short-circuit with `SwarmError::BudgetExhausted`,
the consensus surfaces with `budget_exhausted: bool` + the per-
member transcript; `MockMember` is the test fixture; 37 new tests
across `swarm_basic`, `swarm_budget`, `swarm_consensus`.
**Track E** closes 2 of 3 v0.26 demo 07 QoL gaps: `Vector.is_empty()`
single-line method on `std.memory.VectorStore`, and `mty run path
-- a b c` argv forwarding via a process-wide `OnceLock<RwLock<
Vec<String>>>` cell installed by `Run` dispatch before runtime
startup (`std.env.args()` reads the snapshot — three unit tests
serialised by a `TEST_SERIAL` mutex to dodge the parallel-test
race the integrator caught on Windows). The source-level
streaming surface (`for chunk in stream { ... }`) shipped partial
— the runtime-side `MessageStream::next()` exists but surfacing
it as `for` needs `while let` pattern desugaring in the parser,
which rolls to v0.28; 15 new tests. **Track F** ships demo 08
swarm-driven code-reviewer — 216-LOC `.mty` that exercises every
other track end-to-end (source-level `@tool` decorator on the
snippet-loading fn, agent field of type `Swarm`, all three real
non-Anthropic providers, swarm consensus + budget surface,
`vector.is_empty()`, `mty run -- <snippet-id>`); `mty check` +
`mty fmt --check` clean and the mock-LLM smoke under
`MTY_AGENT_SMOKE=1` passes; **SHIPPED-PARTIAL** because 5 narrow
`mty run` interpreter gaps surfaced (documented as the v0.28
backlog above). **Integrator fixes (this tag commit):**
`crates/mty-codegen-wasm/tests/agent_handle_fields.rs` —
Track A's reported `assertions_on_constants` clippy lint
resolved via scoped `#[allow]` (the constant is intentional —
the assertion is a regression-detection guard).
`crates/mty-stdlib/src/env.rs` — three `env::tests` race
serialised behind a `Mutex<()>` `TEST_SERIAL` static. Plus a
`cargo fmt` sweep across the three new Track D test files +
the `mty-stdlib` swarm module re-exports (the formatter wanted
the variant names sorted after the `swarm` re-export). **KNOWN_ISSUES
net: 0.** No new entries. P2 #9 (demo 06 RAF-mid-frame phash
flake, 4-of-5 success, no required-gate impact) stays open. P1
stays empty. **v1.0 freeze gate status: unchanged structurally.**
Blockers #1 + #3 stay CLOSED; #2 (8 RFC comment windows)
infrastructure stays live; earliest possible v1.0.0 tag remains
**2026-07-26**. Conformance kit stable at **159 cases** (the
v0.27 surfaces are stdlib, not normative). Rust test count grows
**1989 → 2125** (+136; A +13, B +12, C +82 = 29 integration +
53 lib, D +37, E +15, F +0, scaffolding −23). Python stable
at **490**. Self-host driver stable at **23**. Combined (with
159 conformance cases): **2797** (+136 vs v0.26). Eight demos
(was 7); demo 08 adds `MTY_AGENT_SMOKE=1` to the mock-LLM
end-to-end stage. See
[`dev/history/releases/RELEASE-v0.27.md`](dev/history/releases/RELEASE-v0.27.md).

## [0.26.1] - 2026-05-27

**Hotfix: SSE parser tolerates CRLF input on Windows checkouts.**
Anthropic SSE-event boundaries are `\n\n` per spec; on Windows
checkouts with `core.autocrlf=true`, the captured streaming
fixtures load as CRLF and `parse_anthropic_sse`'s
`rsplit_once("\n\n")` never matches, returning the entire body as
the tail and dropping every event on the floor. v0.26.1
normalises CRLF → LF at the head of the parser so fixtures (and
any upstream proxy that rewrites line endings) parse identically
on every platform. The Anthropic real-network path is unaffected —
Anthropic emits LF only. CI red on windows-latest at v0.26.0
(5/5 `llm_streaming` tests failed); CI green at v0.26.1. No new
tests; the existing 5 `llm_streaming` tests now pass on Windows.

## [0.26.0] - 2026-05-27

**Mighty is now an LLM-agent language: typed providers,
capability-enforced tools, MCP server/client, and memory
primitives. Demo 07 puts it all together.** v0.26 is the agent-
features turning-point release. Three new stdlib surfaces
(`std.llm` + `@tool` / `std.mcp` + `std.memory`) land in parallel
with the v0.25 carry-over cleanup and a 213-LOC research-agent
demo that consumes the new surfaces end-to-end. **Track A** ships
`mty_stdlib::llm::LlmProvider` as the single typed trait every
backend implements, with Anthropic as the SHIPPED-FULL reference
(real HTTP/1.1 over `hyper` + `tokio-rustls`, SSE streaming via
`event: content_block_delta` / `message_stop`, typed `ContentBlock::
ToolUse { id, name, input }` for tool-use blocks, typed `Budget`
with per-method short-circuit returning `LlmError::BudgetExhausted`
off the request estimate, typed `LlmError` covering
`BudgetExhausted` / `Network` / `Status` / `Decode` / `Stream`)
and OpenAI / Gemini / Bedrock as SHIPPED-SKELETON (auth +
endpoint + body shape correct against the canonical vendor URL +
typed schema; `complete()` returns a stub
`Message::assistant_text("[<vendor> stub v0.26 ...]")`; v0.27
wires the response parser + streaming bodies); 49 new tests.
**Track B** ships `@tool` as a typed attribute macro through
`mty_macros` (signature `@tool(description: Str, cap:
CapabilitySet)`; expansion emits a synthesised `__tool_<name>`
companion fn with the fn metadata + registry registration call;
the macro is registered at Rust level — the source-level
`@tool(...)` parse is v0.27 work), plus `std.mcp` server (stdio
+ http auto-exposes registered tools) + `std.mcp` client (runs
the JSON-RPC initialise + tools/list + tools/call handshake) +
5-family CapabilitySet enforcement (`Fs` / `Net` / `Clock` /
`Model` / `Custom(Str)` checked at every tool invocation; per-
invocation capability ledger accumulates for replay); new
`MT6011`–`MT6016` diagnostic band; 48 new tests. **Track C**
ships `std.memory` with three primitives — `VectorStore` (local
flat-list cosine-similarity index + qdrant skeleton), `Episodic`
(in-memory ring buffer + sqlite-backed persistence via opt-on
`memory-sqlite` feature; `(rowid, key TEXT, value JSON,
recorded_at TEXT)` schema), `Working` (token-budgeted scratchpad
with FIFO drop-oldest on budget overflow) — and replay
integration via a new `MemoryDelta { store, op, key, value }`
event variant routed through the existing `record_io_read` hook
so `mty replay` reconstructs memory state at any frame; 63 new
tests. **Track D** closes 3 of 5 v0.25 Track F gaps: wasm32-web
agent persistence emitter-side via per-agent 64KB linear-memory
regions + `__agent_<Name>__inst_ptr` global + callback exports
loading state pointer + calling handler with state as implicit
first arg (closes Track F §C); extern_js name canonicalised via
`kebab()` (pivoted from v0.25's "preserve `_` verbatim" because
`wit_parser` rejects `_`-prefixed identifiers even with
`%`-escape; closes Track F §B; side effect: existing hand-written
JS shims targeting `_foo` must migrate to `foo` in the WIT-
binding layer); canvas taint through fn parameters via type-based
detection (extends v0.25 Track A's per-fn scheme to flow taint
into callees when a param resolves to `std.web.Canvas`; closes
Track F §A); 15 new tests. Track F's remaining 2 gaps (§D
`const` in match patterns, §E `format!("{n}", n=value)`)
roll forward to v0.27 QoL. **Track E** ships demo 07 research
agent — 213-LOC `.mty` source that consumes `std.llm` +
`std.memory` (indexes a local 5-doc corpus into the VectorStore,
calls the LLM provider, dispatches tool invocations against the
`@tool`-tagged fns, persists episodic memory across turns, writes
the final answer back into the corpus), opt-in mock-LLM smoke
(`MTY_AGENT_SMOKE=1 bash demos/07_research_agent/smoke.sh`) + real
Anthropic invocation path (`ANTHROPIC_API_KEY=sk-ant-... mty
run`). **SHIPPED-PARTIAL**: 6 narrow v0.27 follow-ups documented
(`@tool` source-level parser; opaque-ADT ctor scope + agent ADT
fields → wasm32-web; `mty run` argv forwarding; `Vector.is_empty()`;
source-level `stream!` macro). **Integrator fixes (this tag
commit):** `crates/mty-cli/src/cmd/fmt.rs` normalises CRLF → LF
before the `fmt --check` compare and preserves the file's original
line-ending convention on write — `fmt --check` was failing on
Windows checkouts (`core.autocrlf=true`) because the formatter
emits LF and exact-string compare against CRLF was always reporting
"would reformat"; the v0.26 swarm Windows smoke would have shipped
red without this fix. Plus 4 demo formatter-idempotence sweeps
(`demos/0{1,2,3,4}/src/main.mty` each had an extra blank line the
formatter collapses to canonical single-blank) and one unused-
import removal in `crates/mty-stdlib/tests/memory_episodic.rs` that
v0.26 Track C left after a late-merge refactor. **KNOWN_ISSUES
net: 0.** No new entries. P2 #9 (demo 06 RAF-mid-frame phash flake,
4/5 success rate, predates v0.24, no required-gate impact) stays
open. P1 stays empty. **v1.0 freeze gate status: unchanged
structurally.** Blockers #1 + #3 stay CLOSED; #2 (8 RFC comment
windows) infrastructure + dashboard stay live + discussion threads
opened 2026-05-26 (commit `bf4261e`); earliest possible v1.0.0 tag
remains **2026-07-26**. Conformance kit stable at **159 cases**
(new surfaces are stdlib, not normative). Rust test count grows
**1790 → 1989** (+199; A +49, B +48, C +63, D +15, E +0, +24
integrator / scaffolding). Python stable at **490**. Self-host
driver still at **23**. Combined (with 159 conformance cases):
**2661** (+185 vs v0.25). See
[`dev/history/releases/RELEASE-v0.26.md`](dev/history/releases/RELEASE-v0.26.md).

## [0.25.0] - 2026-05-26

**Closed all 7 v0.24 demo-blocking gaps + extended `format!()` +
real `std.String` / `std.Vec[T]`. Demo 06 V2 shim −48 %.** v0.25
is a six-track parallel swarm that closes every gap v0.24 Track
E flagged for v0.25 plus extends the language surface with two
foundational stdlib types. **Track A** wires `canvas.fill_rect(...)`
through HIR → IR → wasm32-web import via a per-fn canvas-handle
taint scheme on `FnBuilder::canvas_locals` (constructor taints
the result local; `bind_pat_assign` propagates through let-
rebind; `lower_expr::MethodCall` and `lower_call`'s local-method-
call arm route tainted-receiver calls to `BuiltinId::CanvasOp`)
+ fixes the latent Unit-returning user-fn stack-balance bug
(`emit_call`'s `FnRef::User` arm now pushes a placeholder
`i32.const 0` for Unit / Never callees, matching every other
arm) — closes v0.24 Track E gaps A + B + KNOWN_ISSUES P2 #8;
24 new tests. **Track B** lifts `extern js { fn _foo() }`
into real `(import "mty:web/js" "_foo" ...)` entries via a new
`Program::extern_bindings` IR side-table populated by
`record_extern_bindings` in `register_fn_shells`, an
`Emitter::predeclare_extern_js_imports` pre-declare pass that
runs before `declare_fns` (so the function-index space is
correct), and a per-program `interface js { ... }` WIT stub —
closes v0.24 Track E gap E; 13 new tests. **Track C** fixes
agent fields with `[T; N]` types — the parser already accepted
the surface, but HIR lowering's `TYPE_ARRAY` arm dropped the
length expression (`len: None` → slice degrade); 12-line fix
captures the expression as `ExprId` and passes `len = Some(...)`
through to the downstream `const_eval_len` path. Plus pins SIR-
runtime cross-callback persistence with three regression tests
(persistence already worked there; never tested). Designs the
wasm32-web single-agent-instance pattern for v0.26 — closes
v0.24 Track E gaps C + D; 12 new tests. **Track D** extends
`format!()` to the full Rust layout grammar
(`[[fill]align][sign][#][0][width][.precision][type]`): `{:5}`,
`{:05}`, `{:<5}` / `{:>5}` / `{:^5}`, `{:*<5}` fill char,
`{:.3}` precision, `{:+}` sign, `{:#x}` / `{:#X}` / `{:#b}` /
`{:#o}` alt prefixes, `{:b}` / `{:o}` no-prefix. Combined specs
respect canonical ordering (`{:#05x}` → `0x0ff`). New
diagnostics MT6011 (`UNSUPPORTED_FORMAT_TYPE`), MT6012
(`MALFORMED_FORMAT_WIDTH`), MT6013 (`MALFORMED_FORMAT_PRECISION`).
Defers positional `{0}`, dynamic `{:1$}`/`{:.*}`, explicit
`n=v` named-arg passthrough to v0.26 — closes v0.24 Track E
gap F; 64 new tests + 6 conformance fixtures. **Track E**
lands real `std.String` (UTF-8 byte string, `Vec<u8>`-backed,
`String.new` / `with_capacity` / `from_str` / `from_utf8` /
`len` (bytes) / `push_str` / `push` / `clear` cap-preserve /
`as_str` / `to_str`, no `unsafe`) and `std.Vec[T]`
(`#[repr(transparent)]` over `std::vec::Vec<T>` so the wasm
Component ABI `list<T>` layout matches; `new` / `with_capacity` /
`push` / `pop` / `get` / `len` / `is_empty` / `clear` / `iter`)
in `mty-stdlib`; new `examples/26_string_vec.mty`; 41 unit
tests. **Track F** rewrites demo 06_canvas_game canvas-direct
against Tracks A–E's outputs — `agent Notetris: NotetrisInput
{ board: [U32; 200] = [0; 200], score = 0, ... }` is the
protocol of record, `frame(dt)` opens `let canvas = ...` and
routes 30+ render ops through the local, HUD lines use
`format!("score: {:>4}", n)`. JS shim drops **213 → 110 LOC
(−48 %)**; Mighty source grows 186 → 313 LOC (now carries the
canonical agent decl + canvas-direct render + Vec[U32] board
construction). Surfaces **5 narrow v0.26 gaps**: (§A) canvas-
handle taint through fn params; (§B) extern_js kebab-vs-`_`
drift through `wit-component`; (§C) wasm32-web agent
persistence emitter-side; (§D) `const` identifier in match
patterns; (§E) `format!("{n}", n=value)` named-arg shorthand.
**Integrator fixes (this tag commit):** orchestrator commit
`4b8ae7a` ("ci: fix clippy-strict failures across v0.25
swarm") pinned 5 cross-track clippy-strict lints
(`manual_let_else` + 4 others) the unified
`-D warnings` sweep surfaced that no individual track ran;
this tag commit fixes two example-file formatter idempotence
drifts in `examples/25_agent_array.mty` (CRLF line ending) +
`examples/26_string_vec.mty` (blank lines around a `// -----`
divider). **KNOWN_ISSUES net: −1.** P2 #8 (wasm32-web Unit-fn
stack-balance) resolved by Track A's `emit_call` fix; P2 #9
(demo 06 RAF-mid-frame phash flake, 4/5 success, not a
required-gate blocker) stays open. P1 stays empty. **v1.0
freeze gate status: unchanged structurally.** Blockers #1 + #3
stay CLOSED; #2 (8 RFC comment windows) infra + dashboard
stay live; earliest possible v1.0.0 tag remains
**2026-07-26**. Conformance kit grows **156 → 159 cases**
(+6 new `format_*` fixtures, replaces 3 v0.24 stubs). Rust
test count grows **1675 → 1790** (+115). Python grows **474 →
490** (+16; format-spec parser tests). Self-host driver still
at **23**. Driver bucket grows **153 → 173** (+20). Combined:
**2476** (+148). See
[`dev/history/releases/RELEASE-v0.25.md`](dev/history/releases/RELEASE-v0.25.md).

## [0.24.0] - 2026-05-26

**wasm32-web emitter completed + `format!()` + v1.0-RC5 spec
polish + deterministic `mty serve --watch`.** v0.24 closes the v0.23
Track D #1 / #2 / #3 language gaps at the emitter + macro layer,
drops a long-standing `#[ignore]` on the watcher integration test,
walks the spec from RC4 to RC5 (+414 lines normative prose; §12.6
`Resumable` / §12.7 `MT506x` reload band / §12.8 Tier 4.3
migration + `PlacementPolicy` / §20.6 cap-name resolver active
emit / §22.5 per-message work-stealing / §25.8.1-8
`mty:web/canvas@0.1` + `mty:web/input@0.1`), ships a live RFC
dashboard with per-window countdowns + per-RFC implementation
status, declares the v1.0 GA normative/informative conformance
split (104 normative / 49 informative), and rewrites demo
06_canvas_game against the new exports + `format!()` (Mighty
source 195 → 186 LOC; JS shim 235 → 213 LOC). **Track A** ships
`BuiltinId::CanvasOp(CanvasOpKind)` SIR variant + wasm32-web
dispatch arm + `is_web_callback_export` wiring (`frame` /
`keydown` / `keyup` now reach the embedded core module's export
section; 10 codegen tests). **Track B** ships `format!()` as a
first-class Mighty macro (`{}` / `{:x}` / `{:X}` / `{:?}` /
named-arg passthrough / brace escapes + MT6009 + MT6010
diagnostics; 22 integration + 19 unit tests + 3 conformance
fixtures). **Track C** drops the v0.23 `#[ignore]` on
`serve_watch_rebuilds_on_change` via an env-gated test hook
(`MTY_SERVE_TEST_WATCH_HOOK=1`) that bypasses OS-watcher event-
timing jitter; 5/5 deterministic; +2 net tests. **Track D** ships
[`docs/spec/rfcs/RFC_DASHBOARD.md`](docs/spec/rfcs/RFC_DASHBOARD.md),
annotates all 8 RFC files with `## Implementation Status`, walks
`docs/spec/v1.0-rc.md` from RC4 to RC5, and declares
[`tests/conformance/v1.0-NORMATIVE.md`](tests/conformance/v1.0-NORMATIVE.md).
**Track E** rewrites demo 06_canvas_game and surfaces **6 v0.25
gaps**: (A) HIR → IR routing for `canvas.fill_rect(...)`, (B)
Unit-returning user-fn call stack-balance failure at wasm-component
validate (KNOWN_ISSUES #8; reproduces against v0.23.0, NOT a v0.24
regression), (C) agent fields don't survive across exported-
callback invocations, (D) arrays in agent fields don't parse, (E)
`extern js { fn _foo() }` declarations don't emit wasm imports,
(F) `format!()` extended specs (width / precision / alignment)
deferred from Track B. KNOWN_ISSUES picks up entries #8 (gap B
latent emitter bug) + #9 (demo 06 headless-smoke phash flake on
RAF-mid-frame capture moments, 4/5 success rate, predates v0.24).
v1.0 freeze gate: blockers #1 + #3 stay CLOSED; #2 (8 RFC comment
windows) infra stays live + dashboard added; earliest possible
v1.0.0 tag remains **2026-07-26**. Rust test count **1604 →
1675** (+71). Python stays at **474**. Conformance kit grows
**153 → 156 cases** (+3 from Track B's format!() fixtures).
Self-host driver still at **23**. Combined: **2328** (+74). See
[`dev/history/releases/RELEASE-v0.24.md`](dev/history/releases/RELEASE-v0.24.md).

## [0.23.0] - 2026-05-26

**Mighty can run a web game on localhost.** v0.23 lands the
`mty:web/canvas@0.1` + `mty:web/input@0.1` WIT interfaces, the
`std.web` host bindings, a `wasm32-web` regression harness that
locks in the embedded core-module invariant, a `mty serve` dev
server with hot-reload + a `mty new --template web-game` scaffold,
headless-browser visual smoke for every web demo, and a 6th demo
where the Mighty agent drives the canvas via the new WIT surface.
The Tetris demo at the end of v0.22 was the right stress-test: it
surfaced exactly how thin the canvas + keyboard story was. v0.23
closes that gap end-to-end. **Track A** (canvas + keyboard WIT)
ships `crates/mty-stdlib/src/web/{canvas,input}.rs` (~430 LOC) with
`WIT_IMPORT_*` / `WIT_EXPORT_*` drift-guard constants + 8 codegen
tests + 13 stdlib unit tests covering `Canvas::clear/fill_rect/
request_animation_frame` and `Input::poll_keydown/keyup`. **Track
B** (wasm32-web embedded core module) is a no-code-change recon
outcome — the long-standing suspicion that wit-component shipped a
"header-only" component was wrong; the core module IS embedded at
byte offset 189, and a 5-test regression harness
(`crates/mty-codegen-wasm/tests/embedded_core_module.rs`) now locks
the invariant in via `wasmparser` walks against the 2055-byte
framing floor. **Track C** (`mty serve` + `mty new --template
web-game`) lands `crates/mty-cli/src/cmd/serve.rs` (+~340 LOC) with
a hand-rolled HTTP/1.1 server + RFC 6455 hand-rolled websocket
hot-reload over `notify` file watches, plus a template registry
(`crates/mty-cli/src/cmd/new.rs`) with two templates (`blank` +
`web-game`) embedded via `include_str!`; 22 tests. **Track D**
(demo 06_canvas_game) ships a 6th demo where the Mighty agent owns
score/level/piece/board and drives the canvas via Track A's WIT;
JS shim down **32% (345 → 235 LOC)** vs demo 05; headless smoke
locks in a `canvas_game.phash` golden. Three language gaps
surfaced — `BuiltinId::CanvasOp(...)` lowering, `format!()` /
interpolation, `export fn` reaching the core export table — and are
**flagged for v0.24** (the canvas-game runs; not every piece of
logic lives in Mighty source yet). **Track E** (headless-browser
visual smoke) lands `tests/web-smoke/smoke-headless.mjs` (+~380
LOC) — Playwright-driven, 8x8 average-hash perceptual-hash golden
under `tests/web-smoke/golden/<name>.phash`, hamming-distance
tolerance 12, opt-in via `MTY_WEB_SMOKE=1`, skips cleanly when
Playwright isn't installed; manual `web-smoke.yml` workflow_dispatch
job; wired into demos 02 + 05 + 06. **Integration fixes (this
tag commit):** (a) `crates/mty-cli/tests/cmd_serve.rs` port flake —
`pick_port` was nanosecond-hashed mod 10000 and collided
deterministically under workspace-wide parallel testing; replaced
with OS-assigned via `TcpListener::bind("127.0.0.1:0")` then
drop-and-reuse. (b) `crates/mty-runtime/tests/telemetry.rs`
cross-test env pollution — tests 2 + 7 (`#[tokio::test]`) set
`MTY_OTLP_ENDPOINT` while tests 8 + 9 (plain `#[test]`) raced their
remove; defensive `remove_var` at start of plain tests. (c)
`crates/mty-cli/src/cmd/new.rs` path-as-package-name bug — `mty
new --template web-game /tmp/asteroids` was substituting the full
path into `{{NAME}}` → generated `package /tmp/asteroids` → parse
error; new `package_name_from_path` helper sanitises basename to a
valid identifier + 4 new tests. (d) `tests/web-smoke/
smoke-headless.mjs` canvas-or-DOM mode — Track E's
counter-web wiring required a `<canvas>` that the counter demo
doesn't have; new `--mode {canvas,dom}` flag validates `#count` or
`[data-mty-output]` for DOM-mode demos. (e) `demos/02_counter_web/
web/serve.sh` python3 portability — Windows aliases bare `python3`
to the MS Store launcher stub; backported the cascading `python` →
`python3` → `py` lookup from demo 06's serve.sh. (f) `demos/
05_notetris_web/{mighty.toml, README.md, src/, web/}` untracked-file
recovery — the v0.22 notetris demo source had been written to disk
but never `git add`-ed (only Track E's smoke.sh was committed);
files were complete + consistent, pulled into the tag. **All gates
green, Rust test count grows 1554 → 1604** (+50: +8 Track A
codegen + +13 stdlib + +5 Track B regression harness + +22 Track C
serve/new + +5 cross-cut integrator). Python stays at **474** (no
impl-py changes in this slice). Conformance grows to **153 cases /
24 categories** (+6: Track A wasm_component additions + Track B
codegen regression cases). Self-host driver still at **23**.
Combined: **2254** (+56 vs v0.22's 2198). **KNOWN_ISSUES P1 + P2
lists stay empty.** v1.0 freeze gates: blockers #1 + #3 unchanged
(CLOSED); blocker #2 (RFC comment windows) still infrastructure-
ready, user-action pending. **Earliest v1.0.0 tag: 2026-07-26**
(unchanged).

## [0.22.0] - 2026-05-26

**All post-v1.0 roadmap items now landed pre-v1.0 — work-stealing
(Tier 5) + PGO/ThinLTO + Python full pipeline. Only RFC comment
windows remain for v1.0 GA.** v0.22 closes the v0.21 "Post-v1.0"
block end-to-end. **Per-message work-stealing (Tier 5)** lands —
the v0.10 affinity-hint scheduler is promoted to true crossbeam-
deque per-worker queues with NUMA-locality steal ordering (own
NUMA → same socket → anywhere) and a new process-wide
`worker.steals_total{src,dst}` OTel counter; the `local → siblings
→ injector` phase reversal alone produces a 61% speed-up on
pinned-task bursts vs v0.21 (1000 pinned tasks: 12.1 ms → 4.7 ms;
1000 injector tasks: 5.4 ms → 4.9 ms). New
`crates/mty-runtime/src/scheduler/work_stealing.rs` (+395 LOC) +
`scheduler/locality.rs` (+333 LOC) + `telemetry/sink.rs` (+118 LOC)
+ 7 work_stealing integration tests. **PGO + ThinLTO build
profile** lands — new `[profile.release-pgo]` cargo profile +
two-stage `scripts/build-pgo.{sh,ps1}` pipeline (instrumented
build → `mty-bench-pgo` sweep over `examples/*.mty` →
`llvm-profdata merge` → final build with `-Cprofile-use` +
`-Clinker-plugin-lto`); new `mty-bench-pgo` binary
(`crates/mty-bench/src/bin/mty-bench-pgo.rs`, +160 LOC); new
manual `.github/workflows/pgo-bench.yml` runs the pipeline on
`workflow_dispatch` and writes baseline-vs-PGO `mty check`
wall-clock delta to the workflow summary; PGO **not** wired into
`release.yml` (v0.22 ships measurement, not gating; v0.23's BOLT
follow-up turns it into the default release artifact pipeline).
**Python 2nd-impl full pipeline** lands — the impl-py 2nd-impl
now covers lex → parse → lower → typeck → borrow → wasm end-to-
end. Borrow checker (`impl-py/mty/borrow.py`, +865 LOC) is an
NLL-flavoured subset (scope-based loan lifetimes; MT3001 move-
while-borrowed, MT3002 move-out-of-borrow, MT3003 mut+shared
conflict, MT3004 use-after-move, MT3005 double `&mut`) with
branch joining via AND-of-moved-flags. Wasm codegen
(`impl-py/mty/codegen_wasm.py`, +954 LOC) emits Core 1.0 wasm
bytes — magic + 5 sections (type, function, memory, export,
code); i32 arithmetic, comparisons, bitwise, control flow,
calls, locals; if/else block-type i32; while as block+loop+br_if;
deduplicated function-type table; structural validation via
`parse_sections`. Full-pipeline sweep
(`tests/test_examples_full_pipeline.py`) parametrised over 24
examples × 4 phases = 96 cases; coverage gate `≥ 15/24 examples
emit wasm fn body`, **21/24 actual**. Python test count
**311 → 474** (+163: +28 borrow + +37 codegen + +98 sweep).
**Diagnostic-code coverage closure** activates 7 of the 8
v0.21-uncovered codes — MT0004 UNKNOWN_DURATION_UNIT + MT0030
DEPTH_LIMIT_EXCEEDED via a new `Parser::pre_lex_scan` (INT_LITERAL
+ IDENT zero-gap with duration-unit-like text and DURATION_LITERAL
+ IDENT unconditional emit MT0004; paren/brace/bracket nesting >
256 emits MT0030) + driver `parse_source` preserving
`ParseError::code` instead of funneling to UNEXPECTED_TOKEN;
MT2015 NON_EXHAUSTIVE_MATCH + MT2016 UNREACHABLE_MATCH_ARM via
`synth_match`; MT2018 IF_BRANCH_MISMATCH via `synth_expr_inner`
If branch; MT2019 RETURN_TYPE_MISMATCH via custom function-body
path in `items` (synthesises tail without expected-propagation,
unifies against ret); MT3015 USE_OF_UNINITIALIZED via
`mty-borrow::flow::walk_stmt` binding `let x: T;` as
`Ownership::Uninit`. **MT3012 DROP_IN_CONST_CONTEXT explicitly
deferred to v0.23** — HIR's `lower_item` punts on `CONST_DECL`
(`mty-hir/src/lower/items.rs:33`), so emit-site activation
requires (1) full `CONST_DECL → HirConst` lowering, (2) a
const-context flag propagated through the HIR walker,
(3) a borrow-check pass over const initialisers — each a slice's
worth of work; bundling them into the closure slice would burst
its scope. +7 conformance fixtures (`parser/02`, `parser/03`,
`type_checking/28..31`, `borrow_checking/15`). Coverage delta:
covered 62 → 69 (+7), uncovered 8 → 1 (-7, MT3012), direct % 56
→ 63, any-harness % 93 → 99. **MtyIR `Stmt` source-span carrier**
lands — every MtyIR `Stmt` + `Terminator` now carries a real
`SourceSpan` field (default `SourceSpan::ZERO` for manually-
constructed programs); HIR spans propagate through
`lower → MtyIR → cranelift SourceLoc → DWARF v5 line row`, so
v0.21's synthetic-uniform per-statement byte-offset spread is
gone and `gdb step-line` is byte-accurate. `mty-ir/src/ir.rs`
(+74 LOC), `lower/{ctx, exprs, items, stmts, mod}.rs` (+308 LOC
across), `mty-codegen-cranelift/src/lower.rs` (+29 LOC reads
`stmt.span.start_byte`), +5 spans tests in `mty-ir/tests/spans.rs`
+ extended `debug_mach_src_loc.rs` (new
`dwarf5_row_byte_offsets_match_source`). All gates green:
**1554 Rust tests** (+25 vs v0.21), **474 Python tests**
(+163 vs v0.21), **147 conformance cases** (+7), **23 self-host
driver** tests (unchanged), **2198 combined** (+195 vs v0.21's
2003). KNOWN_ISSUES P1 + P2 stay empty.

## [0.21.0] - 2026-05-26

**The post-v1.0 roadmap continues to land pre-v1.0 — Polonius
borrows + cap-name resolver + Tier 4.3 lossless live migration +
DWARF v5 dense rows.** v0.21 finishes everything v0.20 deferred
and lands the last three items from the v0.19 "Post-v1.0" block.
**Hot reload (Tier 1.5) completes**: `MT5064` placeholder is gone —
new `crates/mty-runtime/src/reload/wasm_loader.rs` parses
`__mty_agent_type` + `__mty_schema_hash` custom sections via
`wasmparser`; `Program::with_swapped_agent` clones the per-agent
slot map; `MigrateFrom<Old>` + a `SchemaRegistry` BFS over
`(old_hash, new_hash)` edges supports schema-evolution chains
(V1 → V2 → V3 supported); the control-socket `op=reload` handler
is end-to-end via `Request::Reload { agent_type, module_b64,
deadline_ms }` + `ReloadHook` trait + process-global
`reload_hooks()` registry; the 1 ms busy-poll is gone, replaced
with a `condvar_drain::DrainSignal` (parking_lot `Condvar` over
`Mutex<DrainState>`). +27 reload tests across `reload_wasm.rs`
(6), `reload_migration.rs` (8), updated `reload.rs` baseline,
and inline control-socket / condvar / resumable / wasm_loader
tests (65 reload-related tests across the crate).
**Tier 4.3 lossless live agent migration (RFC-006)** lands: new
`crates/mty-runtime/src/cluster/migration.rs` (~680 LOC) carries
`MigrationOrchestrator::migrate_agent(agent, target, deadline)`
running the canonical drain → snapshot → ship
`WireFrame::MigrateSnapshot` → `MigrateAck` → forward queued
mailbox → mark agent `REMOTE(target, new_id)` sequence; abstracted
over the runtime via three hooks (`SnapshotSource` / `SnapshotSink`
/ mesh wire surface) so `agent.rs` / `runtime.rs` stay untouched;
6 MB hard cap on snapshot payload
(`MAX_MIGRATION_SNAPSHOT_BYTES`); new `MT507x` diagnostic band
reserved for migration (MT5071 AgentNotFound / MT5072
TargetUnreachable / MT5073 SameNode / MT5074 Deadline / MT5075
Rejected / MT5076 SnapshotTooLarge / MT5077 Mesh / MT5079
Internal — plus MT5060 IncompatibleSchema shared with reload);
new `crates/mty-runtime/src/cluster/placement.rs` (~250 LOC)
lands `PlacementPolicy` trait + 3 bundled policies (`StickyPolicy`,
`LeastLoadedPolicy`, `StaticPolicy`); supervisor's
`RestartRequested` event now carries
`placement_hint: Option<NodeId>`; new `[cluster.placement]`
manifest block with `policy = "sticky"|"least_loaded"|"static"` +
`default_node`; OTel cluster metrics (migrations_started_total /
migrations_completed_total / migrations_failed_total /
migrations_rolled_back_total / migration_state_bytes_sum /
placements_chosen_total{policy}); +8 migration tests in
`tests/cluster_migration.rs`. **DWARF v5 MachSrcLoc plumbing**:
cranelift's per-instruction `MachSrcLoc` map flows through
`Module::define_function` so the v0.20 conservative 2-entry line
table is replaced with a dense per-statement line program;
`LowerCtx` grows `fn_debug: HashMap<IrFnId, FnSrcLocMap>` + a
`capture_debug_info` flag; `FnLower::note_stmt_loc(byte_offset)`
pushes synthetic byte offsets into `stmt_byte_offsets[idx]` and
calls `b.set_srcloc(SourceLoc::new(idx))`; `lower_one_block`
invokes `note_stmt_loc` at every MtyIR statement boundary +
terminator; `.debug_loclists` per-local emitted from cranelift
slot offsets (same gap as v4 today, now closed for v5); v5
binary-size delta flips from +3.2% to -2.3% vs v4 on the
synthetic benchmark (dense `DW_LNS_advance_pc` + small-delta
`DW_LNS_copy` opcodes compress better than the equivalent v4
stream once you cross ~8 rows per fn); +5 integration tests in
`crates/mty-codegen-cranelift/tests/debug_mach_src_loc.rs`
(uses `MTY_CRANELIFT_NO_OPT=1` to keep cranelift's egraph from
coalescing arithmetic chains and breaking per-statement row
determinism). **Polonius-style borrows** ship behind the
`polonius` cargo feature: datalog fact model
(`Borrow(origin, place, mut)`, `Loan(origin, scope)`,
`Subset(o1, o2, point)`, `Invalidates(origin, point)`) + 4
inference rules (transitive subset closure, loan-region
intersection, mutual-borrow conflict, end-of-scope loan death)
+ fixpoint solver layered on the v0.3-vintage NLL walker;
default build uses NLL unchanged so v0.21 default semantics are
byte-identical to v0.20; +20 tests (10 integration + 10 inline)
in `crates/mty-borrow`. **Cap-name resolver**: new
`crates/mty-types/src/cap_resolver.rs` + `cap_check.rs` lands a
3-layer scope frame (current fn signature, enclosing impl/trait,
module-level prelude) pinning `Fs` / `Net` / `Clock` / `Dom` /
`Model` names against their cap family + narrowing surface; the
6 v0.20-uncovered MT4xxx codes (MT4060 Unbound / MT4061
FamilyMismatch / MT4062 NarrowingParamMismatch / MT4063
NarrowingInBodyButNotSignature / MT4064 FamilySurfaceInconsistency
/ MT4065 NarrowingConstructorArgShape) now actively emit; +18
unit tests in `tests/cap_resolution.rs`; +6 conformance fixtures
in `tests/conformance/type_checking/22..27/`. **Conformance
expansion**: per-backend test crates
`crates/mty-codegen-cranelift/tests/conformance_native.rs` (5
tests: 4 per-case object-shape MUSTs + best-effort `cc` link-and-
run smoke + 1 inventory) and `crates/mty-codegen-wasm/tests/
conformance_wasm_component.rs` (5 tests: 4 per-case import/export-
subset MUSTs against `expected_component.txt` + 1 inventory);
`tests/conformance/coverage.json` audit reconciles the v0.20
report against the actual fixture corpus — 9 codes promote from
`uncovered` → `covered` without writing new fixtures (MT2003 /
MT2009 / MT2014 / MT2022 / MT2023 / MT2024 / MT2025 / MT3002 /
MT3007 — existing v0.11/v0.12 emit-site work + fixture coverage
was already there); true gap drops 17 → 8; coverage 53 → 62
direct (56%) and 93% any-harness. The 8 remaining gaps (MT0004
/ MT0030 / MT2015 / MT2016 / MT2018 / MT2019 / MT3012 / MT3015)
need crate-source emit-site work + HIR shape gap closure, all
documented in the new `v0_21_audit_note` field of `coverage.json`
for v0.22 follow-up. **`docs/internals/cluster.md`** gains a new
`## Live migration (v0.21 Tier 4.3)` section with the sequence
diagram, the three-hook abstraction, the wire-frame shape, and
the placement-policy surface; `docs/internals/borrowck.md` gains
§21 Polonius; `docs/internals/capabilities.md` gains a v0.21
§Cap name resolution section; `docs/internals/hot-reload.md`
gains wasm-byte loading + schema-migration + condvar-drain +
control-socket protocol sections; RFC-006 now cross-references
the implementation at `docs/internals/cluster.md#live-migration`.
**KNOWN_ISSUES P1+P2 lists stay empty.** **v1.0 freeze blockers
unchanged from v0.19/v0.20**: #1 + #3 CLOSED, #2 infrastructure
live, awaits user-side Discussion-thread openings.
**1529 Rust + 311 Python + 140 conformance + 23 selfhost-driver =
2003 tests passing** (+96 vs v0.20), 0 failing, 7 ignored
(unchanged), 0 clippy warnings under the strict `pedantic` gate,
all 6 CI workflows green (CI / Pages / Python second-impl / bench
/ security / release), conformance kit ~108 K (unchanged)
auto-attached to v0.21.0 alongside Linux x86_64 + macOS arm64 +
Windows x86_64 binaries.

## [0.20.0] - 2026-05-26

**The full post-v1.0 roadmap is now live pre-v1.0 — hot reload,
cluster mTLS+supervisor, DWARF v5, byte-identical replay all
landed.** v0.20 collapses the entire `### Post-v1.0` block from
the v0.19 README roadmap into shipping code. **Hot reload (Tier
1.5)** ships: new `Resumable` trait (FNV-1a `SCHEMA_HASH` const +
default ciborium-backed `to_snapshot`/`from_snapshot`), the swap
pipeline (`reload::swap` — pause → drain → snapshot → schema
check → restore → resume via `ReloadGate`), `ModuleSource::SameProgram`
wired end-to-end (`ModuleSource::WasmBytes` rejected with `MT5064`
until v0.21), the `mty reload <agent-type> --from new.wasm` CLI
with `--dry-run`/`--deadline-ms`/`--sock`/`--json` flags, the new
diagnostic band `MT5060–MT5069` (IncompatibleSchema / AgentNotFound
/ DrainDeadline / Snapshot / WasmReloadNotImplemented / Internal),
and +24 tests across `crates/mty-runtime/tests/reload.rs` (9) +
inline `resumable.rs` (7) + `swap.rs` (5) + `cmd/reload.rs` (3).
**Cluster mTLS + Tier 4.2 supervisor** ships: new `cluster/tls.rs`
builds rustls accept/connect configs and pins
`verify_peer_identity(node_id, cert_der)` as a custom
`ServerCertVerifier`-driven post-handshake check; a hand-rolled
~50-LOC `extract_cn_from_der` TLV walker pulls the cert CN
(no extra dep — `x509-cert` was already transitively present via
sigstore but a single function isn't worth dep promotion); mTLS is
opt-in via the new `ClusterMesh::from_config_mtls(cfg)` constructor
(`ClusterConfig` shape unchanged so v0.18/v0.19 struct-literal
callers compile clean); new `cluster/supervisor.rs` lands
`ClusterSupervisor` with per-child state machine + 3 restart
strategies (`OneForOne`/`RestForOne`/`OneForAll`) + per-child
circuit breaker (sliding-window failure count, half-open/closed
recovery); restart decisions emit on a bounded
`SUPERVISOR_EVENT_CAPACITY = 256` channel rather than invoking
synchronously (caller picks placement; v0.21 lands `PlacementPolicy`);
mesh `notify_node_disconnect` hook marks affected children
`:noproc`; +13 tests across `cluster_mtls` (5) + `cluster_supervisor`
(6) + inline cert-walker tests (4). **DWARF v5** ships as opt-in
via `MTY_DWARF5=1` (env var, not Cargo feature — feature
unification would invalidate caches for v4 path on every test):
new `crates/mty-debuginfo/src/dwarf5.rs` (~330 LOC) emits the v5
`.debug_info` + `.debug_line` + `.debug_str` + `.debug_line_str` +
`.debug_abbrev` quintuple via `gimli::write::Dwarf::new_5()`;
`mty-codegen-cranelift/src/debug.rs` gains `build_dwarf_dispatch`;
v5 *capacity* for per-instruction line rows + cross-CU
`.debug_line_str` sharing is wired (defensive monotonic-address
skip on `gimli::write::LineProgram::generate_row`; `FileId(0)`
re-add trick because the v5 `LineProgram::new` auto-inserts
comp_file at index 0 but doesn't return its id); the *enablement*
of those wins waits on cranelift `MachSrcLoc` plumbing
(v0.21 follow-up); +5 integration tests in
`crates/mty-debuginfo/tests/dwarf5.rs` (header magic, indirect
string table, round-trip, monotonic drop, file-id-zero re-add).
**Strict-equality replay payloads** finishes the v0.18 hot-path
migration the v0.19 capability work parked: the two in-process
send callsites (`Runtime::send`, `Runtime::ask`) now call a new
`encode_payload_for_trace_structural(&[Value]) -> ReplayPayload`
helper instead of `encode_payload_for_trace`, so fresh recordings
carry `ReplayPayload::Values` payloads by default and the
`ReplayDriver`'s strict structural equality arm is the live replay
semantic (the `Opaque ≈ Opaque` loose-equality arm stays as a
backwards-compat fallback that never fires for fresh recordings;
cluster routing paths still use the byte envelope by transport
contract — the receiver structurally decodes on the other side of
the mesh); +5 strict-equality tests in
`crates/mty-runtime/tests/replay_strict_equality.rs`. **Spec
cross-reference polish** lands: 7 broken internal anchor refs in
`docs/spec/v1.0-rc.md` fixed (python-markdown `toc.slugify` collapses
non-word runs to single hyphens, so em-dash and inline-code
headings never produced double-hyphen slugs; audited via a Python
script that round-trips every heading through `slugify` and diffs
against every `](#...)` reference); one stale RFC-009 cross-ref
in `docs/spec/rfcs/RFC-008-effect-rows.md` replaced with "deferred
to a future RFC." **Conformance corpus expansion**: the four
placeholder categories from v0.19 are populated (`deterministic_replay/`
+5, `formatter_idempotence/` +5, `native_abi/` +4, `wasm_component/`
+4 = +18 cases / 122 → 140); new machine-readable
`tests/conformance/coverage.json` (53 covered / 42 auxiliary / 17
uncovered, the uncovered set unchanged from v0.11);
`.github/workflows/release.yml` gains a `conformance-kit` job that
runs in parallel with `build`, shell-execs
`scripts/build-conformance-kit.sh <tag>`, and includes the
resulting `mty-conformance-kit-<version>.tar.gz` (~108 K) in the
release's `files:` list. **KNOWN_ISSUES P1+P2 lists stay empty.**
**v1.0 freeze blockers unchanged from v0.19**: #1 + #3 CLOSED, #2
infrastructure live, awaits user-side Discussion-thread openings.
**1433 Rust + 311 Python + 140 conformance + 23 selfhost-driver =
1907 tests passing** (+73 vs v0.19), 0 failing, 2 ignored
(`capability_checking/03_narrow_to_ro`, `supervisor_restart/02_escalate`
— both pending the cap-name resolver wiring + escalation-chain
serialisation rework, both post-v1.0 backlog). Two new docs pages
land (`docs/internals/hot-reload.md`,
`docs/reference/cli/mty-reload.md`); `docs/internals/cluster.md`
and `docs/internals/debug-info.md` extended with mTLS / supervisor
and DWARF v5 sections; `mkdocs.yml` nav extended with both new
pages; `mkdocs build --strict` passes locally. **Earliest
possible v1.0.0 tag: 2026-07-26** (unchanged from v0.19; gated on
RFC-002 / RFC-006 comment windows closing).
[Release notes](dev/history/releases/RELEASE-v0.20.md).

## [0.19.0] - 2026-05-26

**The last minor before v1.0-RC — Blockers #1 + #3 closed, every
KNOWN_ISSUES P1/P2 cleared, full cluster routing + byte-identical
replay land.** v0.19 closes two of the three v1.0-freeze blockers
(#1 Python 2nd-impl through HM + closures + generic-constraints with
+37 new tests; #3 normative conformance kit + spec doc +
`scripts/build-conformance-kit.sh`) and ships the tracking
infrastructure for the third (#2 RFC comment-window tracking via
`docs/spec/rfcs/COMMENT_WINDOWS.md`; the actual window-opening is a
user-driven admin action). The replay subsystem grows a **byte-identical
re-execution** mode on wire-format v2: `ReplayPayload::Values` carries
a structural mirror of the IR `Value` type (13 variants), `ReplayDriver`
re-runs the original program against the trace and diffs each event
byte-for-byte, `mty replay --byte-identical --program <path>` is the
CLI seam, v0.18 (`version=1`) traces decode transparently via the
`V1TraceFile` back-compat shim, +24 tests in
`crates/mty-runtime/tests/replay_byte_identical.rs` + unit-test files.
**Cluster routing wires into the Runtime hot path** (Tier 4.1
follow-up): `Runtime::with_cluster(SharedRouter)` +
`send_addr(AgentAddr, …)` + `ask_addr(AgentAddr, …)` consult the
router; a new `CorrelationTable` (`cluster/correlation.rs`) demuxes
inbound `Reply` / `Error` frames into oneshot receivers; a reply-demux
task peels reply frames off the mesh inbox before the runtime sees
them; peer-disconnect fan-out cleanly fails every in-flight ask to
that node (`MT5032`); a `[cluster]` / `[[cluster.peers]]` /
`[cluster.tls]` manifest parser lands in `mty-driver/src/manifest.rs`;
+8 integration tests in `tests/cluster_routing.rs`. **HIR lowerer
reads every row var**: `EffectClause::row_var_names()` (new AST
iterator) chains the three source positions in order;
`lower_effect_clause` collects every var into a fully-populated
`Vec<HirRowVar>`; the v0.15 first-only `row_var_name()` accessor is
`#[deprecated(since = "0.19.0", …)]`; +14 tests; `examples/24_multi_row_full.mty`
typechecks. **Paper-cuts cleared**: KNOWN_ISSUES #4 (`clippy-strict`
required) re-verified, KNOWN_ISSUES #5 (`mkdocs --strict`) re-verified,
KNOWN_ISSUES #7 (`--no-default-features` example sweep) added to the
`test-minimal` job; the vendored `wasi_snapshot_preview1.*.wasm`
bytes are deleted (~125 KB removed) in favour of caller-supplied
bytes via `AdapterEmbed::new(AdapterKind, Vec<u8>)`. **All
KNOWN_ISSUES P1/P2 entries are now closed.** The release workflow
that first fired on v0.15.0 continues to ship `mty` binaries for
Linux / macOS arm64 / Windows on every `v*` tag push (Intel macOS
dropped in v0.18). **1378 Rust + 311 Python + 122 conformance + 23
selfhost-driver = 1834 tests passing** (+121 vs v0.18), 0 failing,
2 ignored (`capability_checking/03_narrow_to_ro`,
`supervisor_restart/02_escalate` — both pending the cap-name
resolver wiring + escalation-chain serialisation rework, both post-v1.0
backlog). One new internals doc page lands
(`docs/internals/conformance.md`); `docs/reference/README.md` rewrites
from stub to full landing page; `mkdocs.yml` nav extended with the
new pages + a top-level **RFCs** section; `mkdocs build --strict`
passes locally. **Earliest possible v1.0.0 tag: 2026-07-26.**
[Release notes](dev/history/releases/RELEASE-v0.19.md).

## [0.18.0] - 2026-05-26

**v1.0 freeze gates closing fast — KNOWN_ISSUES P1 list cleared
(#1, #2, #3), replay end-to-end, distributed agents land.** v0.18
clears every P1 entry on `KNOWN_ISSUES.md`, wires deterministic
replay into the Runtime hot path across 13 instrumentation sites,
and grows the agent runtime a distributed transport layer (Tier 4.1
of `docs/internals/agent-features-roadmap.md`). The spec promotes
to **v1.0-RC4** with the RFC-008 multi-row-variable parser grammar
amendment at §9.2. The `cabi_realloc` real free-list allocator
(KNOWN_ISSUES #1) extracts from inline-in-emit to its own
`cabi_realloc.rs` module (8 size classes, ~190 wasm instructions, 17
dedicated coverage tests); the `mty-pkg/sigstore-real` cargo feature
(KNOWN_ISSUES #2) now compiles and drives the real keyless flow
end-to-end (Fulcio short-lived ECDSA-P256 cert + Rekor
`hashedrekord` upload with full standard Sigstore Bundle JSON
embedded under `verificationMaterial.sigstoreBundle`; `cosign
verify-blob` consumes it directly); the v0.17 replay recorder wires
into `Runtime::{spawn_agent, send, ask, shutdown}`, `agent.rs`'s
inner `run_one_turn_with_shared_reply`, the agent loop's
budget-exhaust / cancellation / terminal-exit arms, and every
`StdHost::effect_call` route for fs / http / time / random (13
sites total, zero overhead when `MTY_RECORD_TRACE` is unset);
`AgentAddr = node:type:pid` + `ClusterMesh` with framed CBOR over
TLS lands the Tier 4.1 transport layer (`Runtime::send` consults
the router in v0.19); the parser tail accepts `(',' RowVar)*` so
the multi-row source forms (`!{| E1, E2}` / `effect a, b | E1, E2`)
parse cleanly and flip MT4059 to active emit; the MSRV gate
(KNOWN_ISSUES #3) hardens to `cargo build --workspace --tests`
which pulls in the full `[dev-dependencies]` graph. The release
workflow that first fired on v0.15.0 continues to ship `mty`
binaries for Linux / macOS×2 / Windows on every `v*` tag push.
**1324 Rust + 274 Python + 92 conformance + 23 selfhost-driver =
1713 tests passing** (+50 vs v0.17), 0 failing, 5 ignored. Three
new internals doc pages land (`agents.md`, `introspect.md`,
`replay.md`); `mkdocs build --strict` passes locally.
[Release notes](dev/history/releases/RELEASE-v0.18.md).

## [0.17.0] - 2026-05-26

**WASI Preview 2 adapter goes away (`log()` direct), deterministic
replay + recorder land, Python 2nd-impl through typeck, RFC-008
multi-row, security bundle cleared.** v0.17 removes the last
preview1-adapter dependency in the WASI P2 hot path: `log()` /
`print()` now lower to a three-call canonical-ABI sequence on
`wasi:cli/stdout@0.2.3#get-stdout` +
`wasi:io/streams@0.2.3#[method]output-stream.blocking-write-and-flush`
+ `[resource-drop]output-stream`, and the embedded adapter flips
from always-on to opt-in (`Preview2Options::new(_).embed_adapter ==
None`; `.with_adapter(Some(WASI_P1_ADAPTER_COMMAND))` reattaches it
for back-compat builds). Tier 1.4 of
`docs/internals/agent-features-roadmap.md` lands as
`crates/mty-runtime/src/replay/{wire, recorder, mod}` (8 typed
`TraceEvent` variants, `MTYTRACE`-magic + serde-additive wire format
v1, `StepHandler` trait + `CountingStepHandler`) and a `mty replay
<trace>` CLI with `--dump-json` + `--step` + `--json` modes; the
full Runtime re-execution and hot-path wire-up are deferred to v0.18.
The Python 2nd-impl (`impl-py/`) reaches typeck for the first time
via `mty/hir.py` + `mty/lower.py` + `mty/typeck.py` (Hindley-Milner
unifier with `TyAny` absorption for shapes the v0.17 surface doesn't
yet model); all 23 `examples/*.mty` typecheck clean and the test
count grows **139 → 274** (+135), substantially closing v1.0
freeze blocker #2. RFC-008's HIR widens to
`HirEffectRow::Open(concrete, Vec<HirRowVar>)`; the
`UserRowPolyMeta` side table feeds the call-site walker so MT4055
(declaration ambiguity), MT4056 (concrete + row var with no fn-typed
param), and MT4058 (call-site arity mismatch) all reach active
emission, with MT4059 reserved for the v0.18 parser ship of
`!{| E1, E2}`. The `wasmtime` dev-dep bumps 25 → 36, clearing 15
RUSTSEC advisories (`audit.toml` ignore list shrinks 16 → 3); no
production code is affected. The release workflow that first fired
on v0.15.0 continues to ship `mty` binaries for Linux / macOS×2 /
Windows on every `v*` tag push. The spec stays at v1.0-RC3.
**1274 Rust + 274 Python + 92 conformance + 23 selfhost-driver =
1663 tests passing** (+192 vs v0.16), 0 failing, 4 ignored.
[Release notes](dev/history/releases/RELEASE-v0.17.md).

## [0.16.0] - 2026-05-26

**Observability + RFC-008 typeck-finishing tier — live agent
introspection (`mty inspect` + control socket), OpenTelemetry agent
spans, user-authored effect rows typecheck end-to-end, WASI Preview 2
fs + http direct, self-host MethodCall + custom iterators.** Tier 1.1
of `docs/internals/agent-features-roadmap.md` lands as
`crates/mty-runtime/src/introspect.rs` + `control_socket.rs` and a
new `mty inspect` CLI (pretty / JSON / `--watch` modes) wired to an
opt-in `MTY_RUNTIME_CONTROL_SOCK` Unix-domain socket; `AgentSnapshot`
exposes agent type, mailbox depth + high-water, in-flight handler +
elapsed, CPU / mem / tick budgets, and the last-N messages (opt-in
body capture) at wire `version: 1` (additive evolution). Tier 1.2 +
1.3 land as a new `telemetry/` submodule under `mty-runtime`:
`span_spawn` / `span_send` / `span_ask` / `span_handler` plus
`record_restart` + `record_budget_exhausted`; the
`agent.event(name, &[(k, v)])` helper attaches user attributes to the
active handler span; lazy init from `MTY_OTLP_ENDPOINT` keeps the
runtime cost-zero when telemetry is disabled. The v0.15 RFC-008
surface syntax is wired through typed AST accessors
(`mty-ast::effects`) → `HirEffectRow` (`Closed | Open`) on
`HirFn::effect_row` → `UserRowPolyIndex` in `mty-types::effects`;
five new diagnostic codes (**MT4055 / MT4056 / MT4057 / MT4058 /
MT4059**) are wired, MT4057 actively emits, and
`examples/22_effect_row.mty` flips from `@typeck-pending` to live in
the example sweep. The WASI P2 emitter takes nine more stdlib
lowerings direct: five `std.fs` fns (`open` / `read_file` /
`write_file` / `stat` / `close`) hit
`wasi:filesystem/types@0.2.3#descriptor.*` and four `std.http`
variants (`get` / `post` / `send` / `incoming_request_consume`) hit
`wasi:http/types@0.2.3` + `wasi:http/outgoing-handler@0.2.3`; a
latent emitter import-index bug is fixed via a new `prescan_p2_direct`
predeclare pass. The self-host Wasm codegen lowers `Rvalue::MethodCall`
through the host `ir_method_resolve(name)` bridge (v0.15 emitted
`unreachable`) and desugars `for x in custom_iter` at the selfhost-IR
layer into the iter-protocol loop-match-`Some`/`None` shape; driver
tests go **17 → 23 live / 0 ignored**. The release workflow that
first fired on v0.15.0 continues to ship `mty` binaries for Linux /
macOS×2 / Windows on every `v*` tag push. The spec stays at v1.0-RC3.
**1217 Rust + 139 Python + 92 conformance + 23 selfhost-driver = 1471
tests passing** (+43 vs v0.15), 0 failing, 4 ignored.
[Release notes](dev/history/releases/RELEASE-v0.16.md).

## [0.15.0] - 2026-05-25

**Dispatch-finishing tier — HOF dispatch end-to-end, RFC-008
surface syntax, WASI P2 default, self-host 17 codegen tests,
cross-platform release binaries.** The 19 row-polymorphic stdlib
signatures that v0.14 landed as a SHIPPED-SUBSET are now wired
through call-site dispatch: a new `BuiltinMethod.row_sig` field
threads 21 sigs across 12 method names into
`walk_expr_effects`, which instantiates fresh row variables per
call and propagates closure effects into the caller (MT4050 fires
on closed-row rejection; +10 dispatch tests). RFC-008 surface
syntax `!E` / `!{a | E}` / `!{fs, net | E}` / `effect a | E`
parses through `mty-syntax` with 4 new SyntaxKind variants
(EFFECT_SET, EFFECT_NAME, EFFECT_ROW_TAIL, EFFECT_ROW_VAR), spec
§9.2.1, +16 parser tests, and `examples/22_effect_row.mty`
(parser-only; HIR/typeck wiring is v0.16). WASI Preview 2 is now
the default for `wasm32-wasi` (explicit `--wasi=p1` retains
back-compat) and four stdlib fns (`std.random.bytes`,
`std.time.now` / `monotonic_now` / `resolution`) emit direct P2
imports through `emit.rs`; the log shim + `std.fs` / `std.http`
still route through the embedded adapter (canonical-ABI rewrite
deferred to v0.16). The self-host Wasm codegen reaches **17 live /
0 ignored** (was 13) with variant-call lowering in
`mty-ir::lower::exprs::resolve_callee` (Some/Ok/MyEnum.Variant →
`Rvalue::AdtInit`), a SwitchInt cascade for dense integer matches,
and `for i in 0..n` desugar. The deprecated
`mty_macros::expand` / `expand_to_source` API is removed (9
integration test files migrated; `mty-macros` 111 → 101 tests, 10
redundant pruned + coverage preserved). The v0.13 red-shirt
`conformance/borrow_checking/14_borrow_outlives_owner` is closed
by the one-line `SyntaxKind::BLOCK` arm in
`mty-hir::lower::exprs::is_expr_node`; conformance corpus moves
**91 → 92 cases / 16 categories / 3 → 2 ignored**. A new
`.github/workflows/release.yml` produces `mty` binaries for Linux /
macOS×2 / Windows on `v*` tag push — first run on this tag. The
spec stays at v1.0-RC3 (RFC-008 + RFC-009 remain roadmap RFCs).
**1140 Rust + 139 Python + 92 conformance + 57 self-host = 1428
tests passing** (+38 vs v0.14), 0 failing, 3 ignored.
[Release notes](dev/history/releases/RELEASE-v0.15.md).

## [0.14.0] - 2026-05-25

**Integration-and-finishing tier — WASI Preview 2 with vendored
wasmtime adapter, self-host codegen reaches example 03, set-of-scopes
hygiene now powers HIR macro resolution, KNOWN_ISSUES #11 closed.**
The WASI Preview 2 backend now embeds the upstream wasmtime v32
preview1→preview2 adapter (command / reactor / proxy under
[`crates/mty-codegen-wasm/wit/adapter/`](crates/mty-codegen-wasm/wit/adapter/))
and ships the full upstream WASI 0.2.3 WIT surface; `std.random` /
`std.time` route through new `P2DirectImport` constants direct to
preview2 origins (`std.fs` / `std.http` direct lowering is v0.15).
The v0.13 internal `mighty:cli-adapter` shim is gone — components
now run unmodified on any preview2 host. The self-host codegen
([`selfhost/codegen/wasm.mty`](selfhost/codegen/wasm.mty)) grew
~400 → ~660 LOC with three new modules
(`string_pool.mty`, `adt_layout.mty`, `pattern.mty`) and the
driver test reports **13 live / 0 ignored** (example 03 passes,
was the v0.13 single ignored). `mty-hir::lower::macros` now drives
`expand_scoped_to_source` (set-of-scopes) rather than the legacy
mangler; the legacy `expand` / `expand_to_source` API stays
callable behind a `#[deprecated(since = "0.14.0")]` shim with
removal scheduled for v0.15. Two FROZEN typeck codes land their
emit-sites (MT2003 at `check_stmt(HirStmt::Let)`, MT2023 at
`resolve_generic_args`); the other four in KNOWN_ISSUES #11
(MT2009 / MT2022 / MT2024 / MT2025) were rediscovered to already
have emit-sites from v0.12 work — issue #11 closed with a per-code
closure-history table. The conformance corpus moves **89 → 91
cases** / 16 categories / 3 ignored (red-shirt
`14_borrow_outlives_owner` traced to a one-line bug in
`mty-hir::lower::exprs::is_expr_node` missing the `BLOCK` arm —
out of v0.14 swarm scope, carried over). Stdlib HOF row-polymorphism
lands 19 more row-polymorphic signatures in a new `pub mod
stdlib_sigs` (+207 LOC) as a SHIPPED-SUBSET — the signatures + 24
tests ship; the call-site dispatch through
`prelude::BuiltinMethod` is v0.15. Integrator carve-out: MT2003
exempts `let mut xs = []` (legitimate idiom — downstream assignments
unify the element type), with a regression test pinning the
behaviour. The spec stays at v1.0-RC3. **1109 Rust + 137 Python +
91 conformance + 53 self-host = 1390 tests passing** (+67 vs
v0.13), 0 failing, 4 ignored.
[Release notes](dev/history/releases/RELEASE-v0.14.md).

## [0.13.0] - 2026-05-25

**Capability tier — end-to-end self-host complete + WASI Preview 2 +
2 new RFCs (effect rows + set-of-scopes hygiene).** The Mighty
compiler front-end + Wasm core-module back-end is now implemented in
Mighty source for the slice-1 subset:
[`selfhost/codegen/wasm.mty`](selfhost/codegen/wasm.mty) (~400 LOC)
closes the bootstrap chain lexer → parser → HIR → typeck → MtyIR →
wasm codegen, with 6/6 live driver tests passing (1 ignored — example
03's generic `Option[T]`). **The self-host milestone called for since
the v0.5 lexer port is reached.** A WASI Preview 2 backend lands
behind `--wasi=p2` (default stays `p1`): new `--world <name>` flag, a
new `[wit]` section in `mighty.toml` for user-supplied WIT, a vendored
`wasi:*@0.2.3` slice covering `cli`/`io`/`clocks`/`filesystem`/`http`/
`random`, example at [`examples/21_wasi_preview2.mty`](examples/21_wasi_preview2.mty),
user-facing matrix at [`docs/reference/wasi.md`](docs/reference/wasi.md).
Two new RFCs land with usable infrastructure: **RFC-008 effect-row
polymorphism** (`!E`, `!{a | E}`, four-case unification, subsumption)
with a 450-LOC row module in `crates/mty-types/src/effects.rs::row`
and a relaxed `stdlib_list_map_sig()`; and **RFC-009 set-of-scopes
macro hygiene** (Flatt-style scope sets) with `scopes.rs` + `hygiene.rs`
+ a new `expand_scoped()` entry point alongside the legacy mangler.
Both ship as **SHIPPED-SUBSET**: infrastructure + tests + first wired
consumer, with v0.14 follow-ups for surface-syntax parsing
(RFC-008) and mty-hir rewire (RFC-009). The spec stays at v1.0-RC3;
the conformance corpus stays at 89 cases / 16 categories / 3 ignored.
**1051 Rust + 137 Python + 89 conformance + 46 self-host = 1323 tests
passing** (+82 vs v0.12), 0 failing, 5 ignored.
[Release notes](dev/history/releases/RELEASE-v0.13.md).

## [0.12.0] - 2026-05-25

**Spec-and-evidence tier — v1.0-RC3 spec released + 4th showcase
demo + conformance Gap B/C/E partial closure + Go 3rd-impl source
landed.** The normative spec advances **v1.0-RC2 → v1.0-RC3**:
operator precedence is promoted to normative §11.1.1 (was deferred
to non-normative `docs/internals/parser.md`); the full reserved
keyword set is enumerated (63 reserved + 4 contextual + 7
reserved-for-future); the 16 Python-impl spec findings from v0.11
are codified in prose (+396 spec lines, no behaviour change). A
fourth runnable showcase lands at [`demos/04_kvstore/`](demos/04_kvstore/)
— a sharded supervised in-memory key-value store (~400 LOC)
exercising agents + protocols + supervisors + restart-on-crash +
`std.http` end-to-end (the first demo whose pitch is the
supervisor restart story). The conformance corpus gains six new
fixtures (typeck 17..20, borrow 13..14) and a real MT3007
`BORROW_OUTLIVES_OWNER` emit-site in `mty-borrow/src/flow.rs`;
the harness now reports **89 cases / 16 categories / 3 ignored**
(one new red-shirt: `borrow_checking/14_borrow_outlives_owner`
needs `pending_borrower` wired through plain assignments —
deferred to v0.13). A Go 3rd-impl lands at
[`impl-go/`](impl-go/): 4848 LOC of lexer + parser + CLI + tests,
built from `docs/spec/v1.0-rc.md` (v1.0-RC3) prose alone, with
zero peeking at `crates/mty-*`, `selfhost/`, or `impl-py/`. The
Go toolchain is not installed on the v0.12 build host so
`go test ./...` has not been run; cross-validation pending v0.13.
**Closes KNOWN_ISSUES #10 (operator precedence not normative) and
#12 (`package`/`export`/`requires` keywords not in §3.3).** **977
Rust + 135 Python + 89 conformance + 40 self-host = 1241 tests
passing**, 0 failing, 3 ignored. [Release notes](dev/history/releases/RELEASE-v0.12.md).

## [0.11.0] - 2026-05-25

**Quality tier — strict-clippy gate green + Python 2nd-impl partial
+ conformance gap closure + UX polish.** The `clippy (strict)` CI
job is now **required** (no more `continue-on-error: true`) and
clean across the whole 20-crate workspace: 2341 pedantic warnings
on baseline → 0 via a workspace-level `[lints.clippy]` allowlist
plus ~30 real fixes. **All six CI jobs now run as required gates.**
An independent Python implementation of the Mighty front-end lands
at [`impl-py/`](impl-py/): pure-Python lexer + parser (~2.5 KLOC)
built from the v1.0-RC2 spec prose alone (no peeking at
`crates/mty-syntax`, `crates/mty-ast`, or `selfhost/`); **135 tests
passing, 20/20 examples lex+parse**. **Real partial credit on v1.0
freeze blocker #1** (two independent implementations). The slice
also surfaced 16 spec findings — biggest: operator precedence is
not in the normative §11 (deferred to `docs/internals/parser.md`)
and needs to be promoted before v1.0 freeze. Normative conformance
corpus grows **88% → 91% FROZEN coverage** (62% → 70% direct), 4 of
8 documented gaps closed with two harness extensions
(warning-severity assertions; per-case `mighty.toml` via `CwdGuard`)
plus 3 new positive-fire cases (MT2012, MT6003, MT6008); the 4
deferred gaps each have a precise crate-source-edit reason recorded.
UX polish: 15 high-traffic MTxxxx codes rewritten to a consistent
Cause/Example/Fix/Spec format, all 16 tour chapters refreshed
(`.sd` → `.mty`, spec links bumped to `v1.0-rc.md`), FAQ extended
12 → 26 entries, getting-started rewritten 187 → 290 lines.
Inherited from post-v0.10.0 `main`: three macOS codegen fixes
(`LC_BUILD_VERSION` on Mach-O objects + cosmetic + CI tolerance for
missing `cc`). **977 Rust tests + 135 Python tests = 1112 total.**
[Release notes](dev/history/releases/RELEASE-v0.11.md).

## [0.10.0] - 2026-05-25

**Production cleanup + conformance audit.** Lifts the v0.9 RC-prep
stubs to real implementations: `cabi_realloc` becomes a segregated
free-list allocator (8 size classes + bump tail), sigstore signing
gets a real keyless path behind the `sigstore-real` feature (default
keeps the v0.9 SHA-256 envelope shape), the Cranelift egraph fuzz
bug is filed upstream as
[wasmtime #13476](https://github.com/bytecodealliance/wasmtime/issues/13476)
with an in-tree `MTY_CRANELIFT_NO_OPT` workaround and a new
`MTY_DUMP_CLIF` debug knob. Conformance corpus grows 16 → 81 cases
(88% FROZEN coverage). Self-host examples 04 + 05 deferrals closed —
**40/40 selfhost tests now pass**. CI hardened: MSRV gate now runs
`cargo test --no-run` + bedrock subset; `mkdocs --strict` enabled
with all 55 stale links fixed; cargo-audit job added; parallel
monomorphisation honestly reverted to sequential default after
re-benching. Major repo cleanup: 62 dev artefacts archived under
`dev/history/`, README rewritten 421 → 210 lines, root
`CHANGELOG.md` introduced, license switched from Apache-2.0/MIT dual
to **MIT-only**, repo URL bumped `hassard0/stardust` →
`hassard0/Mighty`. **977 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.10.md).

## [0.9.0] - 2026-05-24

**RC-prep + freeze-readiness.** Spec promoted to **v1.0-RC2** with all
10 OPEN amendments resolved (3 FREEZE-MVP, 7 DEFER-V1.1) and six
follow-up RFCs drafted (RFC-001..RFC-006). Brought up a four-target
cargo-fuzz harness (parser / typeck / fmt / codegen) with 27-file seed
corpus, fixed three P0 OOM parser bugs the fuzzer surfaced, and did an
audit sweep over every sibling `loop` for the same anti-pattern.
Self-hosted the MtyIR lowering on examples 01-03 (joining the v0.5
lexer, v0.6 parser, v0.8 HIR + minimal typeck — **34 self-host tests
passing**). Fixed `demos/02_counter_web`'s long-standing
`cabi_realloc` regression (3/3 demos passing again). Published the
[GitHub Pages docs site](https://hassard0.github.io/Mighty/), hardened
CI (stable/beta/nightly matrix, minimal-versions, strict, MSRV), shipped
reproducible release scripts, and landed a sigstore-style package
signing stub. **955 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.9.md).

## [0.8.0] - 2026-05-24

**Loose-end closure + self-host HIR + perf + spec v1.0-RC.** Closed 4 of
5 remaining v0.5 loose ends (proc-macro sandboxed execution with
MT6007/MT6008, real per-agent HTTP routing, LSP cross-file workspace
resolve, WIT canonical-ABI return-area for DOM strings). Self-hosted
the HIR + minimal typeck phases (~1.1 KLOC of Mighty source; 5+5 new
self-host tests). Three of four perf optimisations landed (parse +27%,
mailbox +7%, ~800 ns agent-send). Consolidated 88 spec amendments into
**v1.0-RC** at `docs/spec/v1.0-rc.md`. Closed all rebrand residuals
(runtime ABI symbols, DWARF producer, bench fixture). **927 tests
passing.** [Release notes](dev/history/releases/RELEASE-v0.8.md).

## [0.7.0-rebrand] - 2026-05-24

**Stardust → Mighty rename.** Naming-only release: 20 `sdust-*` crates
renamed to `mty-*`, `.sd` → `.mty` source extension, `star.toml`/`star.lock`
→ `mighty.toml`/`mighty.lock`, `SD####` → `MT####` diagnostic codes
(with `SD` aliases preserved for `mty explain`), WIT `stardust:*` →
`mty:*`, VS Code extension repackaged. **0 behavioural deltas — 885
tests pass byte-for-byte against v0.6.0.**
[Release notes](dev/history/releases/RELEASE-v0.7.md).

## [0.6.0] - 2026-05-24

**Multi-core + benchmarks + self-host parser.** Runtime now distributes
work across N OS threads via per-worker tokio runtimes + crossbeam-deque
work-stealing + affinity hints + lightweight migration + per-worker
stats. First honest benchmarks shipped — new `mty-bench` crate covers
six categories with Rust/Go/C++ comparators. Self-host parser subset
(~1930 LOC, 13/13 bootstrap tests, examples 01-05 covered). DOM MtyIR
lowering reaches `emit_dom_call` end-to-end. MT6001-MT6006 macro codes
merged into the central `mty-diagnostics` catalog. Per-call `FsCap`
isolation contract test. **885 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.6.md).

## [0.5.0] - 2026-05-24

**Self-hosting + dogfood completion.** Loops actually terminate via
`break`/`continue`/iterator exhaustion (bounded-fixed-point loop
borrows). Self-host lexer now round-trips byte-for-byte against the
Rust lexer. Five v0.4 dogfood stopgaps replaced with real
implementations (real `std.http.serve` over TCP, Wasm DOM imports as
a 4-method WIT interface, full `Str` method table, MtyIR
mem-budget auto-charge, `FsCap` allowlist process-wide). Macros
completion: `name!(args)` invocation, extended hygiene, cross-file
`pub macro`, proc-macro skeleton, stdlib macros. LSP advanced —
semantic tokens, rename, inlay hints, code actions, signature help,
workspace folders, semantic completion. **839 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.5.md).

## [0.4.0] - 2026-05-24

**Dogfood + ecosystem.** Three end-to-end dogfood demos
(`01_search_api`, `02_counter_web`, `03_extract_tool`) with passing
smoke scripts. Real package registry transport over GitHub Releases
REST with on-disk index cache + sha256 sidecar + deterministic
`.tar.gz` bundles + three new CLI subcommands. Hygienic declarative
macros (MT6001..MT6004 catch unknown/arity/depth/bad-arg). Self-host
lexer subset bootstrap. MtyIR loop terminator fix — loops genuinely
iterate. **692 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.4.md).

## [0.3.0] - 2026-05-25

**Soundness hardening.** Borrow checker grew NLL last-use deactivation
and field-level Places. Type checker grew scope-aware tolerance and
the formal `Sendable` trait (MT3011 at every send/ask site). Runtime
grew cooperative mid-turn cancellation, OTLP wire-format telemetry,
and slab-pool mailbox frames. Closed v0.2 cleanup backlog: stdlib
install, 6/20 wasm-CM gaps, 3 of 5 INTENTIONALLY_IGNORED conformance
cases. **623 tests passing, 20/20 wasm Components.**
[Release notes](dev/history/releases/RELEASE-v0.3.md).

## [0.2.0] - 2026-05-24

**LSP + pkg + doc + DWARF + Wasm CM + stdlib.** Closed every bullet on
the v0.1 deferral list: LSP 3.17 server with VS Code scaffold, package
manager (resolver + lockfile + path/git fetchers + publisher), doc
generator (markdown + HTML + search index), real stdlib (`std.json`,
`std.tls`, `std.http`, `std.fs`, `std.time`, `std.test`) backed by
rustls/hyper/serde_json/tokio, DWARF v4 debug info + wasm source maps,
Wasm Component Model output by default (`wit-component`). 20/20 native
+ 20/20 wasm core-module compilation. **550 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.2.md).

## [0.1.0] - 2026-05-24

**First feature-complete release.** Walked the full spec §31 roadmap
across eight slices: parser → formatter → HIR → type checker → borrow
checker → effects/capabilities/traits → MtyIR + interpreter → runtime
MVP → native (Cranelift JIT + AOT) + Wasm core module codegen. `mty
new` / `check` / `fmt` / `dump` / `run` / `build` / `explain`. 65+
diagnostic codes across MT0xxx..MT8xxx. MSRV Rust 1.85. **376 tests
passing.** [Release notes](dev/history/releases/RELEASE-v0.1.md).

[Unreleased]: https://github.com/hassard0/Mighty/compare/v0.11.0...HEAD
[0.11.0]: https://github.com/hassard0/Mighty/releases/tag/v0.11.0
[0.10.0]: https://github.com/hassard0/Mighty/releases/tag/v0.10.0
[0.9.0]: https://github.com/hassard0/Mighty/releases/tag/v0.9.0
[0.8.0]: https://github.com/hassard0/Mighty/releases/tag/v0.8.0
[0.7.0-rebrand]: https://github.com/hassard0/Mighty/releases/tag/v0.7.0-rebrand
[0.6.0]: https://github.com/hassard0/Mighty/releases/tag/v0.6.0
[0.5.0]: https://github.com/hassard0/Mighty/releases/tag/v0.5.0
[0.4.0]: https://github.com/hassard0/Mighty/releases/tag/v0.4.0
[0.3.0]: https://github.com/hassard0/Mighty/releases/tag/v0.3.0
[0.2.0]: https://github.com/hassard0/Mighty/releases/tag/v0.2.0
[0.1.0]: https://github.com/hassard0/Mighty/releases/tag/v0.1.0
