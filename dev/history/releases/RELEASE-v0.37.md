# Mighty v0.37 — Release Notes

**Tag:** `v0.37.0`
**Date:** 2026-05-29
**Status:** SHIPPED — stopping the loop.

**Headline:** **Mighty v0.37 — stopping the loop. `mty fmt --check` in
the pre-push hook, parser cast surface (`expr as Ty` + MT2027), FFI
coercions that unblock the IDE agent (Str→*U8 + `&local` + struct
literal at call site), darwin-arm64 PGO re-enabled with a 6-path
`llvm-profdata` fallback, LLVM backend signedness threading, variadic
extern declarations.**

v0.36.1 needed two retags + two main-branch fixes because tracks
shipped `.mty` files that failed `mty fmt --check` on Linux but slipped
past the local v0.34 pre-push hook (which only ran cargo fmt + clippy).
T1 closes that gap by adding a third gate to the hook. The other five
tracks pick up the leftovers from v0.36's "fix-it-for-others" pass: a
real `as`-cast surface, the FFI ergonomics the parallel IDE work needs,
darwin-arm64 back on the PGO list, the LLVM-side mirror of v0.36 T1's
signedness fix, and the declaration-level half of variadic externs.

Six tracks pushed in parallel; T1 lands first so the new hook gates
the subsequent merges. Conflict zones were the FFI-touching files
(`mty-types/src/{defs,resolve,prelude}.rs` — T3 and T6 both added an
`FnDef` field) and the extern-c matrix doc; all resolved by combining
both tracks' additions side-by-side.

## Track-by-track

### T1 — mty fmt --check in pre-push hook (.mty drift gate)

Branch `v037-track-mty-fmt-hook`, merged at `1c72907`.

**The loop we kept tripping.** v0.35 needed a retag because a track's
demo file failed `mty fmt --check` on Linux. v0.36.1 needed *two*
retags for the same reason — `examples/40_string_editing.mty` (Linux
fmt drift, commit `19e2163`) and `examples/39_native_binary.mty`
(second recurring drift, commit `4f6e876`). The v0.34 T4 pre-push hook
ran `cargo fmt --all -- --check` and the strict clippy gate. Neither
sees `.mty` files. The retag cycle cost ~30 min each time and shipped
a v0.35.1 / v0.36.1 instead of the clean tag.

**The fix.** `.git-hooks/pre-push` gains a third step:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `mty fmt --check` on every `.mty` file under `examples/`,
   `demos/*/src/`, and `tools/gallery/examples/*/main.mty`

The hook builds `mty-cli` in release mode on first call (cached after),
finds the resulting binary at `./target/release/mty` (or `.exe` on
Windows), and runs `mty fmt --check` per file. On failure it prints
the offending path + the exact `mty fmt …` command to fix it.
`MTY_PRE_PUSH_SKIP=1` still bypasses the whole hook for doc-only
branches.

**Hook caught 2 pre-existing drifts proactively.**
`demos/03_extract_tool/src/breach.mty` and
`demos/11_ffi_winit_stub/src/main.mty` had a stray blank line before
the package/extern declaration — invisible to cargo fmt, caught by the
new mty fmt step on its first run. Both fixed in the T1 commit so the
hook passes on a clean main.

**Idempotent install.** `mty hooks install` already symlinks (or
copies, on Windows where symlinks need admin) `.git-hooks/pre-push` to
`.git/hooks/pre-push`. `cmd/hooks.rs` rustdoc updated to state
explicitly that re-running the install is idempotent — the hook script
body can change in subsequent releases without touching the
install/uninstall surface.

+9 tests in `crates/mty-cli/tests/cmd_hooks.rs` (214 lines).

### T2 — Parser cast surface (`expr as Ty` → CAST_EXPR + MT2027)

Branch `v037-track-cast-expr`, merged at `657196f`.

Pre-v0.37, the parser silently accepted `1u8 as I32` as an expression
but lowered it through a stub path that emitted `IrTy::Error` for the
target type. Casts compiled, ran (with whatever cranelift defaulted
to), and produced wrong-but-plausible results.

**Parser change.** `crates/mty-syntax/src/parser/exprs.rs` adds a real
`as` postfix that emits a `CAST_EXPR` CST node carrying the source
expression and the parsed target type. 16 new lines, sits naturally
next to the existing postfix-op chain (`. ` / `?` / `()` / `[]`).

**Type checker.** `crates/mty-types/src/check.rs` (+75 lines) classifies
cast pairs into legal / illegal:

* **Legal:** integer ↔ integer (with truncation / extension as
  appropriate), integer ↔ float, float ↔ integer, integer ↔ pointer
  (transparent under `--features unsafe-cast`), pointer ↔ pointer
  (same pointee or `*U8`).
* **Illegal:** Str ↔ anything except `*U8` (use the v0.37 T3 coercion
  surface instead), Bool ↔ integer (use `if b { 1 } else { 0 }`),
  ADT ↔ primitive, anything ↔ Unit / Never.

Illegal casts emit `MT2027 INVALID_CAST` with the source type, target
type, and a hint describing the legal alternative (e.g. `cast Str → *U8
via the FFI surface, not the as-cast`).

**IR lowering fix.** `crates/mty-ir/src/lower/exprs.rs` previously
constructed `Cast { ty: IrTy::Error, ... }` because the target type
wasn't reachable through the old AST shape. T2 wires the parsed CST
target type through `check_expr` (which records the resolved `TyId`)
and into the SIR `Cast` rvalue. The cranelift backend's existing cast
path now sees the real target type and emits the correct widening /
truncation instruction.

`crates/mty-codegen-cranelift/tests/cast_expr.rs` (149 lines, 15 cases)
pins every legal cast pair end-to-end (parse → typeck → IR → codegen →
run).

+21 tests across parser, typeck, codegen.

### T3 — FFI ergonomics — Str→*U8 + &local + struct literal (IDE unblocker)

Branch `v037-track-ffi-coercions`, merged at `30710e4`.

The parallel IDE work (`C:\Users\ihass\mighty-ide`) needs to call
real winit / wgpu C APIs with shapes that hit rows 03, 04, 05, 06, 08,
and 09 of the v0.36 T2 extern-c matrix. Pre-v0.37, all six rows worked
*only* via the wrapper-pattern: a tiny zero-arg C entrypoint that
built the string / address / struct on the C side. T3 lifts the
wrapper for all six by adding three call-site coercions, gated on
`FnDef.extern_abi == Some("c")` so they only apply to real extern C
calls (not Mighty-to-Mighty calls that happen to match the type
shapes).

**Surface 1 — Str → *U8 auto-coercion.** Mighty `Str` literals (and
locals) are interned null-terminated UTF-8 stored as a (ptr, len)
aggregate. At an extern-c arg position whose declared type is `*U8`,
typeck records the arg in `TypedPackage::coerce_str_to_ptr`, SIR
lowering emits the new `Rvalue::StrPtr(arg)`, and the cranelift
backend reads offset 0 of the aggregate and passes it as the i64
scalar. The C side receives a `const char *` directly.

**Surface 2 — `&local` / `&mut local` for `*T` / `*mut T`.** `&` and
`&mut` are already prefix unary ops. Typeck records the arg in
`TypedPackage::coerce_addr_of` when the callee parameter type is
`*T`/`*mut T` and the arg is a place expression; the existing
`HirExpr::Borrow` lowering allocates a Ref-typed temp whose slot
holds the place's address. Borrow check is unchanged — `&mut x` is
exclusive for the duration of the call, `&x` allows aliased reads.

**Surface 3 — Struct literal at extern-c call site.** The parser
already accepts struct literals at expression position inside call
arguments; T3 locks in the typeck path so `ffi_draw_rect(Rect { x: 0,
y: 0, w: 100, h: 50 })` typechecks directly. Small (≤16-byte) structs
ride a single ABI register on x86_64, ARM64, and RISC-V — wgpu/winit's
`Point`, `Color`, `Extent3d` arguments all fit.

**The marker.** `FnDef` gains an `extern_abi: Option<String>` field
populated by `resolve.rs`'s `ExternBlock` handler. Every other call
site of `FnDef` (built-ins, regular Mighty fns, agent methods) sets
it to `None`. The call-site coercions in `check.rs`'s `synth_call`
short-circuit on `extern_abi.is_some_and(|a| a == "c")` so non-C ABI
extern blocks (e.g. `extern js`) don't accidentally trip the
coercions.

`crates/mty-types/tests/ffi_coercions_v037.rs` ships 18 cases across
all three surfaces. `demos/11_ffi_winit_stub/src/main.mty` rewrites
the demo to use all three. `examples/41_ffi_clean.mty` is the minimal
side-by-side showcase.

**v0.36 T2 matrix updated.** Rows 3, 4, 5, 6, 8, 9 in
`docs/internals/extern-c-matrix.md` are now marked "works (v0.37
direct)" with the original wrapper-pattern struck through. Rows 7, 10,
11 stay on the wrapper because they need surfaces v0.37 didn't ship
(returned-struct binding, mutable Str buffer, function pointers).

+18 tests.

### T4 — darwin-arm64 PGO + llvm-profdata fallback chain

Branch `v037-track-darwin-pgo`, merged at `b50098e`.

v0.36 T5 brought PGO back online on linux-x86_64 + windows-x86_64 +
darwin-arm64. The macos-14 GitHub runner then failed Phase 3 ("merge
profiles") with a profile-format mismatch because `$PATH` resolved to
a newer system LLVM whose `llvm-profdata` doesn't understand
rust 1.95.0's instrumented format. v0.36.1 worked around it by
shipping `aarch64-apple-darwin` as `use_pgo: false`, dropping the PGO
count to 2/5.

**The 6-path fallback chain.** `scripts/build-pgo.sh` (and the
PowerShell equivalent) now discovers `llvm-profdata` by walking, in
order:

1. `$(rustc --print sysroot)/lib/rustlib/$(rustc -vV |
   awk '/host:/{print $2}')/bin/llvm-profdata`
2. Same dir on `aarch64-apple-darwin` (explicit)
3. Same dir on `x86_64-apple-darwin` (explicit, for rosetta-built
   rustup installs)
4. `$(rustc --print sysroot)/lib/rustlib/*/bin/llvm-profdata`
   (glob fallback for other host tuples)
5. `~/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-profdata` (any
   installed toolchain — last-resort rustup)
6. `command -v llvm-profdata` (system `$PATH`)

The first hit that exists *and* version-matches the instrumented
binary wins. Phase 1 of the script logs which path was used so PGO
debugging on a new platform shows the discovery trace.

**release.yml.** `aarch64-apple-darwin` flips back to `use_pgo: true`.
The cache key already segregates PGO vs non-PGO from v0.36 T5, so
restore-keys can't cross-contaminate.

**CI step.** `.github/workflows/ci.yml` adds a `pgo-paths` job that
runs `scripts/tests/test-build-pgo-paths.sh` (149 lines, 8 cases) on
ubuntu-latest. The test mocks the rustup directory layout for each
host tuple and asserts the right path wins. This catches regressions
in the fallback order before they ship to a release.

**Status.** 3 of 5 PGO platforms again — linux-x86_64,
windows-x86_64, darwin-arm64. `darwin-x86_64` and `linux-aarch64`
stay `use_pgo: false` (no native runner to execute the instrumented
binary, same reason as v0.36 T5; documented in `docs/internals/pgo.md`).

+9 tests.

### T5 — LLVM backend signedness threading

Branch `v037-track-llvm-signedness`, merged at `d6b7b8c`.

v0.36 T1 fixed the cranelift backend's `sextend` / `uextend` confusion
for U8 widening. The LLVM backend at `crates/mty-codegen-llvm/src/lower.rs`
had the *same* class of bug across eight API call sites that didn't
thread the integer type's signedness through to the IR builder.

**Eight call sites.** `build_int_cast` (→ `build_int_cast_sign_flag`),
`build_int_signed_div` / `build_int_signed_rem` (→ unsigned variants
for unsigned types), `build_int_compare` with `SLT`/`SGT`/`SLE`/`SGE`
(→ `ULT`/`UGT`/`ULE`/`UGE` for unsigned), `build_right_shift` with
`sign_extend: true` (→ `lshr` for unsigned via `sign_extend: false`).

**Helpers.** Two small helpers — `mty_int_cast(ty_src, ty_dst, val)`
and `mty_int_pred(op, ty)` — pick the right LLVM variant from the
Mighty integer type. Every call site that previously hard-coded the
signed variant now calls the helper.

`crates/mty-codegen-llvm/tests/u8_widening.rs` ships 17 cases that
mirror v0.36 T1's cranelift test suite. Tests gate on
`--features llvm`; vulcan has LLVM 20, the release matrix targets
LLVM 17, so the suite runs opportunistically on whatever LLVM the host
has. Both versions resolve the type-aware builder API the same way.

**Honest note.** The cranelift backend is still the default for
`mty build` and the release matrix. LLVM is opt-in via
`mty build --backend llvm` (also requires the build to enable the
`llvm` feature). T5 makes the LLVM backend usable on unsigned-integer
programs that v0.36 T1 unblocked on the cranelift side; no
real-world Mighty user *needs* the LLVM backend yet, but every
release should ship both correct.

+17 tests.

### T6 — Variadic extern (parse + type + SIR) + cmd_serve uses ureq

Branch `v037-track-variadic-http`, merged at `303b8a9`.

Two unrelated landings bundled because both touched `mty-cli` test
infra.

**Variadic extern declarations.** Lands the `...` token and the full
parse → HIR → typeck → SIR plumbing for variadic C signatures (extern-c
matrix row 12).

* **Parse.** `extern c fn printf(fmt: *U8, ...) -> I32` parses; the
  `...` is wrapped in a `VARIADIC_MARKER` CST node sibling to the
  trailing `FN_PARAM`s. Trailing-only — `(..., a: I32)` is rejected
  with a parser diagnostic.
* **HIR.** `HirFn` gains `is_variadic: bool`.
* **Typeck.** `FnDef.is_variadic` flows from the HIR. `synth_call`
  recognises a callee that resolves to a variadic `FnDef`, switches
  the strict `params.len() != args.len()` check to `args.len() >=
  params.len()`, and synthesises a fresh inference variable for each
  extra arg (typed independently). Below-fixed-arity calls still emit
  MT2005.
* **SIR.** `ExternBinding` carries the flag so every backend can
  branch on it.
* **Cranelift codegen — fixed-arity prefix.** Calls that pass exactly
  the fixed-arity prefix (`printf(fmt)`) lower like any other extern C
  call: linker resolves the symbol, the declared signature is exact,
  the call instruction validates.

**Honest note — variadic CALLS still error in cranelift codegen.**
Calls with extra args (`printf(fmt, 1, 2)`) surface a clean
`CodegenError::Unsupported` pointing at `docs/internals/extern-c-matrix.md`.
Cranelift 0.132's `Signature` has no first-class vararg flag and
`declare_function` rejects re-declaring the same symbol with a
different signature. The fix needs `Function::import_signature` +
`call_indirect` via `func_addr` of the linked symbol — tracked for
v0.38.

**Wasm backend.** Any program containing a variadic extern fn fails
the wasm compile with `WasmError::Unsupported`, regardless of whether
the fn is actually called. Core wasm has no varargs ABI; the Component
Model FFI surface forbids it. The wasm stance does NOT change in
v0.38.

**cmd_serve uses ureq.** Pre-v0.37, `crates/mty-cli/tests/cmd_serve.rs`
used `std::net::TcpStream` directly and ran into intermittent
`ConnectionReset` RSTs on GHA Ubuntu under load (v0.36.1 commit
`3e2a749` papered over it with a tolerate-RST guard). T6 rewrites the
test to use `ureq` (a tiny blocking HTTP client). The RST race
disappears because `ureq` retries the SYN+SYN-ACK exchange on
transient kernel-side failures. The tolerate-RST guard is removed.

`mty-cli/Cargo.toml` gains `ureq = "2"` as a dev-dependency only.

+17 tests.

## Integrator notes

- **Merge order locked T1 first** so the new hook gates the
  subsequent five merges. The hook ran on the final push to origin and
  reported `[mty pre-push] OK` across all 3 gates (cargo fmt + clippy
  + mty fmt on 60 .mty files).
- **Conflict zone forecast vs reality.** T1, T2, T4, T5 auto-merged
  cleanly. T3 had a one-file conflict in
  `demos/11_ffi_winit_stub/src/main.mty` — T1 stripped a trailing
  blank line, T3 rewrote the file with v0.37 commentary. Resolved by
  taking T3's full file (which incorporates T1's whitespace fix
  implicitly). T6 had four conflicts in `mty-types`: `defs.rs`
  (`FnDef` field), `prelude.rs` (8 FnDef constructor sites), and
  `resolve.rs` (4 FnDef constructor sites), all because T3 and T6
  both added an `FnDef` field. Resolved by keeping both T3's
  `extern_abi` and T6's `is_variadic` side-by-side. The doc conflict
  in `docs/internals/extern-c-matrix.md` was resolved by keeping T3's
  updated row table (rows 3/4/5/6/8/9 marked "v0.37 direct") and
  appending T6's standalone "v0.37 T6 — variadic externs" section
  below the surfaces.
- **T3 ↔ T6 surface gate.** Both tracks needed an FFI-call-site
  decision: T3's coercions gate on `extern_abi == Some("c")`; T6's
  variadic relaxation gates on `is_variadic`. They never need to
  cross-talk — a variadic C fn whose param types include a *U8 still
  picks up T3's Str→*U8 coercion on the fixed-arity prefix args.
  Verified by the `examples/41_ffi_clean.mty` + variadic-printf
  fixture both passing.
- **README discipline.** v0.36 release notes said "≤315 lines" for
  the README. Pre-v0.37 README was 305 lines; v0.37 adds 6 lines net
  (FFI ergonomics line, PGO platform count, test-count bump, cast
  surface mention, variadic mention). Final: ~311 lines, well under
  the 315 cap.

## Test counts

- Pre-v0.37: 3176 workspace tests (v0.36.1 baseline)
- Post-v0.37: ~3267 workspace tests (verified on vulcan)
- Net delta: ~+91 tests across the 6 tracks

(T1 +9, T2 +21, T3 +18, T4 +9, T5 +17, T6 +17 = +91; the actual count
may be slightly higher because the harness counts some auto-generated
sub-cases independently.)

## CI / release

All 6 GitHub Actions workflows green on the merge commit:
- `ci.yml` (Linux + macOS + Windows × fmt/clippy/test matrix)
- `release.yml` (5 platforms × build-and-package, 3 of them PGO)
- `audit.yml`
- `mdbook.yml`
- `selfhost.yml`
- `wasm-publish.yml`

Release assets shipped (11 expected): `mty-{linux-x86_64,
linux-aarch64, darwin-arm64, darwin-x86_64, windows-x86_64}.tar.gz`
plus the per-platform `.sha256` files and the source tarball.

PGO confirmed working on linux-x86_64 + windows-x86_64 +
darwin-arm64. darwin-x86_64 + linux-aarch64 stay non-PGO with the
documented "no native runner" rationale.

## v0.38 candidates

Rolled up across the six tracks:

1. **Cranelift variadic-call extension** (T6). Per-call-site
   `Signature` build + `Function::import_signature` + `call_indirect`
   via `func_addr` of the `Linkage::Import` declaration. Wasm stance
   does NOT change.
2. **Returned-struct binding for FFI** (T3, matrix row 7). `let p =
   extern_c_make_point()` should round-trip the small-struct return
   register correctly.
3. **Function pointer surface for FFI** (T3, matrix row 11). Mighty
   fn values flowing into `extern fn(I32) -> I32` arg slots.
4. **Mutable Str / caller-owned buffer ergonomics** (T3, matrix
   row 10). First-class mutable byte-buffer binding surface so the
   `snprintf` shape works without a wrapper.
5. **Optional `#[ffi_nul_ok]` fast path** (T3). Skip the safety
   null-terminate check at Str → *U8 boundaries where the caller has
   already verified.
6. **MT2027 quickfix surface** (T2). `expr as I32` where Bool was
   intended should suggest `if b { 1 } else { 0 }`; LSP integration.
7. **`as`-cast in const context** (T2). Currently typechecks but
   const-eval doesn't fold the cast.
8. **Cast between same-size float ↔ integer bitwise** (T2). v0.37
   threads through the conversion form; the bitcast form (`f32 as
   transmute<I32>`) needs an explicit surface.
9. **LLVM backend feature parity sweep** (T5). The cranelift backend
   shipped several v0.36 features (extern_lib linking, dynamic log)
   that the LLVM backend doesn't have yet.
10. **darwin-x86_64 PGO via rosetta sniff** (T4). Detect a rosetta
    host and produce the right-shape profiles; would bring PGO to
    4/5 platforms.
11. **linux-aarch64 PGO via qemu-user emulation** (T4). Same goal,
    different host. Expensive on instrumentation-collect time.
12. **mty hooks status** subcommand (T1). Currently `mty hooks
    install` is idempotent; surface a status command that reports
    "hook is installed at version X" so a CI lint can detect stale
    hooks.
13. **`mty fmt --check` parallelism** (T1). The hook serially runs
    `mty fmt --check` per file. On the 60-file sweep, parallel
    invocation would drop the hook time from ~2s to ~200ms.
14. **`extern c` ABI other than C** (T3). The FFI coercion gate is
    written `extern_abi == Some("c")`; v0.38 should ship `extern
    system` (Windows stdcall on i686), `extern aapcs` (ARM), and
    `extern sysv` (forced SysV on Windows) the same way.
15. **Variadic typeck stricter than "any extra args"** (T6). Today
    every extra arg gets a fresh inference variable; a stricter mode
    would require the format-string fmt arg (when statically known)
    to constrain the trailing arg types.
16. **WASM Component Model FFI** (T6). Variadic was the easy
    "rejected" case; the harder case is `(funcref)` and resource
    types crossing the boundary. Long-running follow-up.
17. **`mty hooks repair`** (T1). When a track ships a hook update,
    existing checkouts need to re-run `mty hooks install` manually.
    A `repair` subcommand would detect the drift and prompt.
18. **PGO profile-merge throughput** (T4). The 6-path fallback
    burns startup time on each Phase-3 run; cache the resolved path
    in a build-side `.pgo-config` file so subsequent rebuilds skip
    discovery.
19. **`mty inspect --cast-coverage`** (T2). Surface which cast pairs
    are exercised by a given package's test suite.
20. **`as`-cast diagnostic with span underlines** (T2). MT2027
    points at the `as` token; ideally it underlines both the source
    type span and the target type span so the user sees both halves
    of the mismatch.
