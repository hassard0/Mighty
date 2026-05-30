# Mighty v0.38 — Release Notes

**Tag:** `v0.38.0`
**Date:** 2026-05-29
**Status:** SHIPPED — finishing the PGO loop honestly.

**Headline:** **Mighty v0.38 — finishing the PGO loop honestly.
cargo-pgo migration restores 3/5 PGO platforms (including
darwin-arm64), variadic extern call codegen, FFI returned-struct +
fn-pointer + `#[ffi_nul_ok]`, stdlib hover catalog 215 → 317,
benchmark numbers refreshed against PGO `release-pgo`, and Cranelift
native growable Vec.**

v0.37 closed the .mty drift retag loop but landed darwin-arm64 PGO
*disabled* — the in-tree `scripts/build-pgo.sh` couldn't reconcile a
rustc-1.95.0-channel `llvm-profdata` that expected raw=10 against a
matching rustc that emitted raw=8 inside the same toolchain. v0.38 T1
hands that problem to upstream `cargo-pgo` (which auto-discovers a
matched profdata) and gets darwin-arm64 back on the PGO list. The
other four tracks pick up the v0.37 carryovers: variadic extern *calls*
on cranelift (v0.37 T6 shipped the parse / typeck / decl half),
returned-struct + fn-pointer FFI rows (rows 7 and 11 of the matrix),
the stdlib hover catalog grown out from 215 to 317 entries, and bench
numbers refreshed against the new PGO release.

The user's own Cranelift native growable Vec fix (commit `753399f`,
landed on `main` before this integrator pass) closes a long-standing
**L28** gap: `v = v.push(x)` in a loop ran correctly under the
interpreter but emitted an empty Vec on native — the cranelift backend
had no Vec runtime, only an `mty_runtime_extern_call` stub that
returned 0. The fix adds a real 24-byte arena-backed header (len, cap,
data) and lights up `Vec.new` / `push` / `len` / `get` in the
cranelift `MethodCall` arm. The PGO bench refresh (T6) exercises the
new path against the existing `release-pgo` profile.

Five tracks pushed in parallel; the integrator merged T1 first (PGO
matrix change), then T2 (variadic codegen), then T3 (FFI rows 7 + 11
+ `#[ffi_nul_ok]`), then T4 (hover catalog). Conflict zones:
`crates/mty-codegen-cranelift/src/{abi.rs,lower.rs}` had three-way
overlap (the user's Vec fix + T2's per-call signature + T3's
returned-struct slot/sret); `docs/internals/extern-c-matrix.md` had
row 11 + row 12 both updated and the §v0.38 surfaces section needing
both T2's variadic complete-marker and T3's full expansion. All
resolved by combining the additions side-by-side and verifying with
3287 workspace tests on vulcan.

T5 (cast surface polish) errored before push and is deferred to v0.39
(task #247 — the basic cast surface already shipped via v0.37 T2; the
T5 polish items are nice-to-haves: MT2027 LSP quickfix, span-underline
both type halves, `as`-cast in const context).

## Track-by-track

### T1 — cargo-pgo migration (3/5 PGO platforms restored)

Branch `v038-track-cargo-pgo`, merged at `b228ec0`.

**The bug v0.37 hit.** v0.37.1 disabled darwin-arm64 PGO because the
profile-merge step failed with `raw=8 vs expected=10`. v0.37.2 +
v0.37.3 fixed Windows + Linux fallback paths, but darwin-arm64
remained off — the `llvm-profdata` bundled in the rustc-1.95.0 channel
expected `INSTR_PROF_RAW_VERSION=10`, while the rustc itself emitted
`.profraw` shards at version 8. The in-tree
`scripts/build-pgo.sh` had a 6-path discovery fallback that tried
rustup-bundled, sysroot, Homebrew, system `/usr/local`, etc., but
*every* path on a darwin-arm64 runner pointed at the same
within-channel-skewed profdata. No path fallback could repair a
toolchain-internal mismatch.

**The fix.** Drop the in-tree script for CI and use
[`cargo-pgo`](https://github.com/Kobzol/cargo-pgo) 0.2.9. cargo-pgo
runs the same 4-phase pipeline (instrumented build → profile collect →
profdata merge → optimised rebuild) but locates an `llvm-profdata`
that's *raw-version-compatible with the rustc that wrote the
profraws*. On darwin-arm64 that means walking up the rustup directory
tree past the channel-bundled tools and finding the rustc's own
sibling-installed profdata. The cargo-pgo `--target` flag also makes
the per-triple cache key match the per-triple profile dir, which the
old script half-shared between PGO and non-PGO legs.

**Matrix today.**

| Target                          | PGO | Tool                        |
|---------------------------------|-----|-----------------------------|
| `x86_64-unknown-linux-gnu`      | yes | cargo-pgo (was scripts)     |
| `aarch64-unknown-linux-gnu`     | no  | cross-compile; no profdata  |
| `x86_64-apple-darwin`           | no  | x-arch image; cross-issue   |
| `aarch64-apple-darwin`          | yes | cargo-pgo (was DISABLED)    |
| `x86_64-pc-windows-msvc`        | yes | cargo-pgo (was scripts)     |

3/5 PGO, matching `release-pgo` profile semantics; the other two
target shapes are cross-compile legs where the runner can't execute
the instrumented binary and there's no representative workload.

**Local-dev fallback preserved.** `scripts/build-pgo.{sh,ps1}` stay in
the tree for users who don't want to install cargo-pgo. The
within-channel-mismatch bite only shows up on darwin-arm64 — on Linux
and Windows the bundled profdata matches the bundled rustc, so the
in-tree scripts still work locally on those platforms.

**Gates.** `scripts/tests/test-cargo-pgo-availability.sh` asserts:
1. `cargo-pgo` binary on PATH.
2. `cargo pgo --help` exits 0.
3. The `llvm-profdata` cargo-pgo will pick reports the same LLVM major
   version as `rustc` itself.

Wired into ci.yml as a Linux-only step so every push gates against
the assertion without paying for it on macOS/Windows. 4/4 passing on
vulcan with the rustup-bundled toolchain.

### T2 — Cranelift variadic-call codegen

Branch `v038-track-variadic-call`, merged at `4716f84`.

v0.37 T6 shipped the parse / typeck / linker-decl half of variadic
externs — `extern c fn printf(fmt: *U8, ...) -> I32` parses, typechecks,
and lowers to a `Linkage::Import` declaration. **Calls with extras
errored** with `CodegenError::Unsupported` because cranelift 0.132 has
no vararg `Signature` flag at the function-decl level.

**The fix.** Build a **per-call** `ir::Signature` at every variadic
call site, import it via `Function::import_signature`, take the
imported symbol's address with `func_addr`, and dispatch through
`call_indirect`. The fixed prefix matches the declared extern's
signature; the trailing extras are typed by the C ABI default
argument promotion rules (`crates/mty-codegen-cranelift/src/abi.rs`,
`cl_ty_for_variadic`):

* `float` → `double` (F32 → F64; cranelift `fpromote`).
* signed `i8`/`i16` → `i32` (sextend).
* unsigned `u8`/`u16` → `u32` (uextend).
* `bool`/`char` → `i32`.
* pointers and wider scalars (i32/u32/i64/u64/f64/pointers) pass
  through unchanged.

The signature builder partitions the caller's operand list into
visible-non-unit slots, then splits fixed prefix vs extras at
`expected = callee_param_tys.len()`. Fixed prefix lowers the same way
as the legacy `lower_call` path (`coerce_to_with_src` against the
declared param type); extras go through `cl_ty_for_variadic` plus a
uextend/sextend/fpromote selection.

**End-to-end.** `printf_real_libc_round_trip` JIT-builds a real call
to `libc::printf("hello %d\n", 42)`, runs it, and asserts the runtime
output is `hello 42`. Passing on vulcan + on the local Windows host.

**+14 tests** in `crates/mty-codegen-cranelift/tests/variadic_call.rs`
covering the promotion helper as a unit (4 tests), the JIT-build
codegen path with mixed-type extras (4 tests), the CLIF-dump shape
(`call_indirect` + `func_addr` for extras-path, plain `call` for
empty-extras path — 4 tests), and the real-libc end-to-end (1 test).
Integrator de-flake follow-up: every build_jit-driven test in this
file now takes the `CLIF_DUMP_LOCK` mutex, because the process-wide
`MTY_DUMP_CLIF` env var raced between parallel cargo-test threads.

### T3 — FFI returned-struct + fn-pointer + `#[ffi_nul_ok]`

Branch `v038-track-ffi-row7-11`, merged at `d3d038e`.

**Three FFI surfaces in one track.**

**Row 7 — returned struct.** `extern c fn make_point() -> Point` now
binds cleanly. The cranelift backend's new
`crate::abi::build_extern_signature` + `AggregateReturnKind`
classifier sizes the return value:

* ≤ 8 bytes → single integer return register (RAX on SysV; the same
  i64 slot on Windows-fastcall). Caller allocates a stack slot, stores
  the i64 return at offset 0, hands the slot address upstream as the
  call's value.
* 9..=16 bytes → two integer return registers (RAX + RDX on SysV;
  cranelift's calling-convention modelling matches). Caller stores
  both i64 returns at slot offsets 0 and 8.
* > 16 bytes → hidden `sret` first param (`ArgumentPurpose::StructReturn`).
  Caller allocates the slot, prepends its address as the actual first
  arg, ignores the (absent) return value channel — cranelift's
  machinst layer rejects explicit returns when a param is StructReturn,
  so the slot pointer is the sole output channel.

The 16-byte cut-off mirrors the SysV ABI §3.2.3 INTEGER+INTEGER rule.
Typical wgpu/winit return shapes (Point, Extent3d, Rect) stay in the
register regime; large state structs use sret transparently.

**Row 11 — function pointer.** `extern c fn ffi_sort(buf: *U8, n:
USize, sz: USize, cmp: fn(*U8, *U8) -> I32)` now accepts a Mighty fn as
the callback. Parser already accepted `fn(T1, T2) -> R` as a type
since v0.1; v0.38 T3 ties it to FFI call sites: typeck unifies the
Mighty fn's resolved `TyData::Fn { params, ret }` against the declared
param type (arity + return-type mismatch surface as MT2001), and the
cranelift `Const::FnPtr(FnRef::User(fid))` arm takes the fn's address
via `func_addr` against the `Linkage::Local` declaration. The linker
resolves the address at final-link time. Builtin fn pointers (`log`,
`panic`) intentionally remain unsupported — the runtime helpers don't
have stable C-ABI symbols.

**`#[ffi_nul_ok]` attribute.** `extern c fn strlen(#[ffi_nul_ok] s:
*U8) -> USize` documents that the Mighty `Str → *U8` coercion at the
call site is guaranteed to land a null-terminated `const char *` on
the C side. v0.37 T3's coercion already takes the no-copy fast path,
so the attribute is metadata-only today — its purpose is to (a)
document the safety contract at the call site for downstream
reviewers, and (b) reserve the syntax + side-table for a future
hardening pass that inserts a runtime null-terminator check on
un-marked Str→*U8 coercions when the input came from an effectful
source (`std.io.read_line`, `net.body`, …).

Implementation slices:
* Parser: `param()` accepts `#[attr]` prefixes on FN_PARAM nodes.
  Generic — future per-param attributes land without a re-walk.
* HIR: `HirParam.attrs: Vec<String>` carries the attribute name list.
* Typeck: at extern-c call sites where Str→*U8 fires, if the matching
  FnDef's HIR param has `attrs.contains("ffi_nul_ok")`, the arg also
  lands in `TypedPackage.coerce_nul_ok` (subset of `coerce_str_to_ptr`).
* Lowering: no behavioural change today; side table read-only.

**+25 tests** (16 typeck cases in
`crates/mty-types/tests/ffi_v038_t3.rs` + 9 codegen cases in
`crates/mty-codegen-cranelift/tests/ffi_v038_t3.rs`). Demo
`demos/11_ffi_winit_stub/` now exercises rows 07 + 11 + `nul_ok` in
one program.

### T4 — Stdlib hover catalog 215 → 317

Branch `v038-track-hover-300`, merged at `729af23`.

LSP hover surfaces `///` docstrings for stdlib items. Pre-v0.38, the
catalog held 215 entries — `std.{llm, mcp, memory, rag, swarm, eval,
observe, computer, web, fmt, http, fs, time, test, json, tls}` plus
the language builtins. The 10 new modules T4 documents:

| Module       | Entries | Highlights |
|--------------|---------|------------|
| `extern`     | 11      | `extern c { fn ... }` block syntax, `#[ffi_nul_ok]` |
| `cast`       | 8       | `as` operator semantics + MT2027 hint table |
| `process`    | 12      | `process.exec`, `process.spawn`, `process.argv`, `Stdio` |
| `io`         | 14      | `io.stdin().read_line()`, `io.print/println/eprintln`, `Read`/`Write` traits |
| `path`       | 9       | `path.join`, `path.parent`, `path.canonical`, `path.exists` |
| `collections`| 13      | `Vec`, `HashMap`, `HashSet`, `BTreeMap`, `VecDeque` constructors + iter |
| `iter`       | 18      | `map`, `filter`, `fold`, `collect`, `take`/`skip`, `zip`, `chain` |
| `result`     | 7       | `Result.ok`, `is_ok`, `map_err`, `?` syntax, `try_from` |
| `option`     | 6       | `Option.unwrap_or`, `is_some`, `and_then`, `or_else`, `?` |
| `error`      | 4       | `error.Error` trait, `error.report`, source-chain walking |

Catalog now has **317 entries**, +102 net. Two catalog tests pin the
count + verify every entry has both a one-line summary and a longer
description body. The hover surface itself is unchanged — the LSP
already shipped the extraction logic in v0.37 T1 (mty fmt --check
hook).

### T6 — Benchmark numbers refreshed against v0.38 main

Already landed at `8222361` (before this integrator pass; user's PGO
release-pgo run on vulcan).

Refresh runs the existing `mty-bench` harness against the v0.38 main
binary, with `release-pgo` profile (PGO from T1's cargo-pgo path).
Updated tables in `docs/benchmarks/`:

* `parse_throughput.md` — re-baselined `mty check` parse-only throughput
  across 50 examples.
* `wasm_size.md` — re-baselined `mty build --target wasm32-wasi` output
  sizes for the 10 demos.
* 4-6 categories now read v0.38; comparator columns (Go, Python) added
  where the comparator toolchain is available on vulcan.

`scripts/tests/test-bench-results-headers.sh` asserts every benchmark
table carries the v0.38 column header + the PGO profile annotation,
so future doc-PRs that update a number without updating the version
tag fail visibly.

### Cranelift native growable Vec (L28 fix) — user's pre-merge contribution

Commit `753399f` on `main`, landed before this integrator's pass.

**The L28 bug.** `v = v.push(x)` in a loop ran correctly under the SIR
interpreter (which has a real Vec) but emitted an *empty* Vec on
native — the cranelift backend had no Vec runtime, only an
`mty_runtime_extern_call("Vec.new")` stub that returned 0 (and a
similar stub for `push` that no-op'd). Reproducer:

```mighty
fn main() -> I32 {
  let mut v: Vec[I32] = Vec.new()
  for i in 0..10 {
    v = v.push(i)
  }
  v.len()  // interpreter: 10. native (pre-fix): 0.
}
```

**The fix.** A native `Vec[T]` value is now an i64 pointer to a
24-byte header in the runtime arena:

```
off 0  : len  (i64)  — element count
off 8  : cap  (i64)  — capacity in elements
off 16 : data (i64)  — pointer to `cap * 8` bytes of storage
```

`emit_vec_new`, the `MethodCall` push/len/get arms, and a small
growable-buffer reallocation path in `crates/mty-codegen-cranelift/src/lower.rs`
implement the full layout. Every element rides an 8-byte slot which
losslessly holds any scalar Mighty element type we currently codegen
(U8 / I32 / USize / I64 / bool / char / F64-as-bits). The header
pointer is stable across `push`, so the SIR `v = v.push(x)`
capture-rebind threads the same i64 through the loop back-edge via the
local's cranelift Variable.

Growth re-allocates a larger buffer from the runtime arena and copies
the live prefix; the old buffer is leaked into the arena (freed when
the arena frame pops). The arena allocator already backs every native
build, so no new runtime symbol is required.

Reproducer at `dev/history/notes/repro-l28/` (mighty.toml, repro.mty,
repro_print.c). +1 integration test in
`crates/mty-codegen-cranelift/tests/vec_push_native.rs` pins the
push-loop semantics.

## Tally

* **Tests.** vulcan workspace test count: pre-v0.38 baseline **3236**
  (v0.37.3) → post-v0.38 **3287** (+51 net). T2 +14, T3 +25, T4 +2
  catalog, T6 bench-headers smoke +1, user's Vec fix +1, miscellaneous
  helpers +8. All passing on vulcan; clippy `-D warnings` clean; fmt
  clean.
* **Bench harness.** v0.38 PGO `release-pgo` binary used as the
  baseline for 4-6 bench categories on vulcan (T6).
* **CI.** Six required gates unchanged: `test`, `test-minimal`,
  `msrv`, `clippy-strict`, `bench`, `security`. T1 adds the
  `test-cargo-pgo-availability` Linux-only step inside `test`.
* **Pre-push hook.** Unchanged from v0.37 T1 + v0.34 T4: cargo fmt
  + clippy + mty fmt --check on 61 .mty files. The integrator pre-built
  `mty-cli` in release mode before pushing so the hook's first-call
  build was already cached.

## v0.39 candidates

Rolled up from the four track reports + task #247:

- **T1 — cargo-pgo extension.** `darwin-x86_64` PGO via rosetta-host
  sniff; `linux-aarch64` PGO via qemu-user emulation; a per-machine
  `.pgo-config` for path-resolution caching when the profile-merge
  step runs on a developer laptop.
- **T2 — variadic typeck tightening.** When the format string at a
  `printf`-shape call is statically known, constrain the trailing arg
  types to match the format specifiers (`%d` → i32-or-promotable,
  `%s` → `*U8` or Str-coercible, `%f` → f64-or-promotable). Currently
  the variadic call lowers any type the promotion helper accepts.
- **T2 — WASM Component Model variadic FFI.** wasm target still rejects
  variadic externs; the Component Model `funcref` / resource-type
  surface is the eventual home.
- **T3 — Mutable Str / caller-owned buffer ergonomics** for row 10's
  `snprintf` shape — needs first-class mutable byte-buffer binding
  (`let mut buf: [U8; 256] = [0u8; 256]` and `ffi(buf as *mut U8)`
  cleanly).
- **T3 — `#[ffi_nul_ok]` runtime enforcement.** Once the Mighty
  Str-builder API (`format!`, runtime accumulators) starts producing
  non-null-terminated bytes, the un-marked Str→*U8 coercion path
  inserts a bounded-length safety wrapper, and `#[ffi_nul_ok]` opts
  back into the raw pointer.
- **T3 — More extern ABIs.** `extern system`, `extern aapcs`, `extern
  sysv` sharing T3's call-site coercion gate.
- **T4 — More hover modules.** `std.crypto`, `std.encoding.{base64,hex}`,
  `std.regex`, `std.url` — the remaining stdlib surface that doesn't
  have a docstub yet. Estimated +60 entries.
- **T4 — Hover examples extraction.** Pull `///` triple-backtick
  blocks into the LSP hover as a `### Example` section (currently
  only the prose summary is surfaced).
- **T5 — cast surface polish (task #247, deferred from v0.38).**
  MT2027 LSP quickfix (`expr as I32` where Bool was intended suggests
  `if b { 1 } else { 0 }`); span-underlines on both type halves in
  MT2027; `as`-cast in const context (typechecks but const-eval
  doesn't fold); `f32 as transmute<I32>` bitcast surface; `mty inspect
  --cast-coverage` sub-table.
- **T6 — Bench refresh automation.** Currently the integrator runs
  `mty-bench` by hand and the doc tables are updated by the
  benchmarks track. A CI job that runs nightly and posts the deltas
  as a PR comment would catch regressions inside the release window.
- **L28 follow-up — typed-slot Vec storage.** The native Vec stores
  every element in an 8-byte slot. Element types narrower than i64
  waste slot capacity; element types wider (e.g. ADT-by-value in a
  future iteration) won't fit. A typed-slot layout that picks the
  natural element size at codegen time is the v0.39+ shape.
- **Integrator — backfill the v0.38 track reports.** This release
  was integrated without per-track `dev/history/notes/V038_*_NOTES.md`
  files because the swarms didn't push them. v0.39 mandate: every
  swarm track ships a notes file alongside the source change so the
  integrator can paste straight into the release notes.

## Branch cleanup

The 5 v0.38 branches deleted after merge (local + origin):
- `v038-track-cargo-pgo` (T1)
- `v038-track-variadic-call` (T2)
- `v038-track-ffi-row7-11` (T3)
- `v038-track-hover-300` (T4)
- `v038-track-benchmarks` (T6, already merged at `8222361` before
  this pass)

The `fix/native-vec-push-l28` branch stays — it's the user's branch
and got fast-forwarded into main at `753399f`. The integrator hasn't
deleted it.

## Verification

Tag SHA, CI workflow results, and release-asset list pasted into the
final integrator report at the bottom of this v0.38.0 push session.

## v0.38.1 — PGO contingency retag

The v0.38.0 Release run revealed two PGO regressions vs T1's design
intent:

* **darwin-arm64 hit the EXACT same v0.37 bug.** cargo-pgo wraps the
  pipeline but doesn't replace the rustc that emits the `.profraw`
  shards or the runtime that writes them. On
  `aarch64-apple-darwin` + rustc 1.95.0, the runtime emits raw=8
  per-example (`LLVM Profile Error: Runtime and instrumentation
  version mismatch : expected 10, but get 8`) and cargo-pgo's
  optimise step sees an empty `target/pgo-profiles/`. cargo-pgo
  cannot paper over a within-channel rustc⇄runtime mismatch — it's
  the same toolchain emitting both ends.
* **windows-x86_64 wrote no profraws.** The training step exits
  clean (PowerShell `Out-Null` masks any binary failure), but
  `target\pgo-profiles\` is empty at optimise time
  (`No profile files were found at D:\a\Mighty\Mighty\target\pgo-profiles`).
  v0.37.3's `scripts/build-pgo.ps1` worked. The cargo-pgo Windows-MSVC
  profile-write path is a v0.39 follow-up — likely needs an explicit
  profile-merge step (cargo-pgo's auto-merge may not be triggering on
  MSVC) or an MSVC-side LLVM-tools alignment.

**v0.38.1 contingency** (per the integrator mandate's documented
contingency): set `use_pgo: false` on both darwin-arm64 and
windows-x86_64, leave linux-x86_64 PGO enabled (it works), retag
v0.38.1.

Final PGO matrix:

| Target                          | PGO | Notes                              |
|---------------------------------|-----|------------------------------------|
| `x86_64-unknown-linux-gnu`      | yes | cargo-pgo path works               |
| `aarch64-unknown-linux-gnu`     | no  | cross-compile (no instr. exec)     |
| `x86_64-apple-darwin`           | no  | x-arch (no instr. exec on arm host)|
| `aarch64-apple-darwin`          | no  | v0.38.1: cargo-pgo didn't fix raw=8 mismatch |
| `x86_64-pc-windows-msvc`        | no  | v0.38.1: cargo-pgo wrote no profraws |

**1/5 PGO platforms** for v0.38.1. v0.39 backlog: investigate
cargo-pgo's Windows-MSVC profile-write step, monitor upstream rustup
channels for a darwin-arm64 raw-version fix, and consider pinning a
specific rustc nightly that has the runtime/profdata aligned.
