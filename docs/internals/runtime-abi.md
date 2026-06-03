# Runtime ABI

The Mighty compiler lowers a handful of operations — print, log, panic,
arena alloc, budget charging, format-to-string, `std.fs.*`, string
concatenation — to calls into a fixed family of `mty_runtime_*` C-ABI
symbols. The runtime crate (`crates/mty-runtime`) implements them; both
the Cranelift JIT and the AOT-via-LLVM backend emit calls against the
same signatures.

This doc covers the **public consumer side** of that surface: who
links what, what stability promise we make, and how to upgrade
vendored copies between releases. The compiler-side codegen lowering
rules live in `docs/internals/codegen-cranelift.md` and
`docs/internals/codegen-llvm.md`.

## Why an official artifact

Pre-v0.46, the runtime symbols were de-facto public — JIT'd Mighty
code calls them by name, and so does any AOT-built binary — but they
were never *officially* published. Downstream consumers had two
options:

1. Re-implement them in a hand-maintained shim
   (the **Mighty IDE** route — `crates/mty-rt-abi` in
   `C:\Users\ihass\mighty-ide`). Brittle: every release adds new
   typed-log / fs / format symbols, and the shim silently fell
   behind until the next link failure.
2. Link the whole `mty-runtime` rlib and pray no symbol got renamed.
   Brittle for a different reason: the rlib pulls in tokio / hyper /
   rustls / opentelemetry.

v0.46 T1 (this release) ships:

- A generated **C header** declaring every symbol.
- A **static library** with just the runtime ABI surface (no tokio,
  no executor — just the codegen-runtime fns).
- A **`mty abi`** CLI subcommand for tooling that wants to verify
  against the ground truth at runtime.
- A **release-pipeline artifact** packaging the header + per-platform
  staticlib so consumers can grab the right archive for their target
  triple from each GitHub Release.

## Surface

Every symbol the compiler may emit a call to is declared in the
generated header:

```
crates/mty-runtime/include/mty_runtime_abi.h
```

That file is **checked in** so agents browsing the repo see it
directly without running a build, and so consumers can `git diff` it
across releases to spot ABI churn at a glance. It is also re-emitted
on every `cargo build -p mty-runtime`; the
`runtime_abi_header_in_sync` integration test fails if the check-in
gets stale.

The header looks like:

```c
#define MTY_RUNTIME_ABI_VERSION       "0.47.0"
#define MTY_RUNTIME_ABI_VERSION_MAJOR 0
#define MTY_RUNTIME_ABI_VERSION_MINOR 47
#define MTY_RUNTIME_ABI_VERSION_PATCH 0

/* @since 0.42.0 */
void mty_runtime_log_i32(int32_t v);

/* @since 0.45.0 */
int32_t mty_runtime_fs_write(int64_t path_ptr, int64_t path_len,
                             int64_t data_ptr, int64_t data_len);

/* @since 0.45.0 @deprecated 0.47.0 — use mty_runtime_fs_dir_open */
void mty_runtime_fs_read_dir(int64_t path_ptr, int64_t path_len, int64_t dst);
/* … one declaration per symbol … */
```

Verify the same surface programmatically:

```sh
mty abi list                 # plain text, one fn per line (with @since / @deprecated tail)
mty abi list --format json   # JSON, machine-parseable (with `since` and `deprecated` fields)
mty abi version              # the version macro, on its own line
mty abi header               # dump the generated header to stdout
```

## Stability

We make the following guarantees within a major release line
(currently `0.x`):

- **Existing symbols never change signature.** If `mty_runtime_log_i32`
  takes `(int32_t)` today, it takes `(int32_t)` in every later patch /
  minor release until the next major bump.
- **New symbols may be added.** Each release that grows the surface
  bumps the `MTY_RUNTIME_ABI_VERSION` macro (and the numeric
  `_MAJOR/_MINOR/_PATCH` macros below) to match the toolchain version.
  Consumers can soft-pin with the numeric macros — see the next
  section.
- **Symbols may be marked deprecated** by adding a `// @deprecated
  X.Y.Z` doc comment above the `#[no_mangle]` in `codegen_abi.rs`.
  The build.rs preserves the marker in the generated header
  (`/* @deprecated X.Y.Z — use foo */`) and in
  `RUNTIME_ABI_SIGNATURES[i].deprecated`. Deprecated symbols keep
  working for at least one minor release after the deprecation lands
  before being considered for removal.

## Stability tiers + version probing

Every declaration in the header carries a `/* @since X.Y.Z */`
comment showing the release the symbol shipped in. Symbols on the
way out additionally carry `/* … @deprecated X.Y.Z — use foo */`.
Both come from `// @since` / `// @deprecated` markers above the
`#[no_mangle]` attribute in
`crates/mty-runtime/src/codegen_abi.rs`; the build.rs parser pulls
them through.

The drift gate `every_no_mangle_fn_has_since_tag` (see
`crates/mty-runtime/tests/runtime_abi_header.rs`) fails CI if a new
`#[no_mangle] pub extern "C" fn mty_runtime_*` is added without a
`@since`. This keeps the header self-documenting as the surface
grows.

Downstream consumers who vendor the header can soft-pin to a minimum
ABI minor with the numeric macros:

```c
#include "mty_runtime_abi.h"

#if !(MTY_RUNTIME_ABI_VERSION_MAJOR == 0 \
      && MTY_RUNTIME_ABI_VERSION_MINOR >= 46)
#  error "this consumer needs mty-runtime 0.46+"
#endif

/* …safe to call symbols marked `@since 0.46.0` or earlier… */
```

…or branch within a single source tree to call a newer symbol when
available and fall back otherwise:

```c
#if MTY_RUNTIME_ABI_VERSION_MINOR >= 46
    int64_t h = mty_runtime_fs_dir_open(p, l);
    /* ... */
    mty_runtime_fs_dir_close(h);
#else
    /* fall back to the v0.45 read_dir Str ABI */
    int64_t slot[3] = {0};
    mty_runtime_fs_read_dir(p, l, (int64_t)slot);
#endif
```

The same fields are available programmatically:

```sh
mty abi list --format json | jq '.symbols[] | {name, since, deprecated}'
```

When a symbol carries `deprecated.since == "X.Y.Z"`, treat it as a
last-call notice — the symbol is still live for at least one more
minor, but the next major bump may remove it. The replacement
symbol (if any) lives in `deprecated.note`.

### Deprecated symbols

The following symbols are still exported (so binaries built against a
prior runtime ABI still link) but the Mighty-side codegen no longer
emits any call to them. They are slated for removal in the next
breaking-runtime-ABI release:

| Symbol | Deprecated since | Replacement |
|---|---|---|
| `mty_runtime_fs_read_dir(path_ptr, path_len, dst)` | `@deprecated 0.47.0` | `mty_runtime_fs_dir_open` / `_next` / `_close` (the v0.46 T4 iterator-handle ABI) |

We do **not** guarantee:

- ABI parity across major bumps. v1.0 is allowed to rename or remove
  symbols, but will ship a header diff in the release notes.
- Behavioral compatibility for `_to_slot` aggregate slot layouts
  beyond what the header declares — see the inline comments in
  `crates/mty-runtime/src/codegen_abi.rs` for the (ptr,len,ok)
  conventions.

## Linking

The release tarballs are named:

```
mty-runtime-abi-<version>-<triple>.tar.gz
```

Where `<triple>` is one of:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

Each tarball extracts to a directory containing:

```
include/mty_runtime_abi.h
lib/libmty_runtime.a          # Unix-style static archive
lib/mty_runtime.lib           # MSVC import library (Windows only)
```

### Linux / macOS (clang or gcc)

```sh
cc -o myprog myprog.c \
    -I path/to/include \
    -L path/to/lib \
    -lmty_runtime
```

### Windows (MSVC)

```bat
cl /Fe:myprog.exe myprog.c ^
   /I path\to\include ^
   path\to\lib\mty_runtime.lib
```

Some platforms require linking against `pthread`, `dl`, and
`m` for the underlying Rust runtime symbols. The release notes for
each tag list any new platform-specific link requirements.

## Refreshing vendored copies

Consumers who vendor the artifact into their own source tree (the
Mighty IDE does this — `vendor/mty_rt_abi.lib`) should script the
refresh per release:

```sh
TAG=v0.46.0
TRIPLE=x86_64-pc-windows-msvc
curl -L -o abi.tar.gz \
   "https://github.com/hassard0/Mighty/releases/download/$TAG/mty-runtime-abi-${TAG#v}-$TRIPLE.tar.gz"
tar -xzf abi.tar.gz
cp lib/mty_runtime.lib vendor/mty_rt_abi.lib
cp include/mty_runtime_abi.h vendor/include/
```

After bumping a vendored copy, smoke-test:

```sh
mty abi list > expected.txt
nm -D vendor/lib/libmty_runtime.a | grep mty_runtime_ > actual.txt
diff <(awk '{ print $1 }' expected.txt) actual.txt
```

The `mty abi list` output is the authoritative reference; any new
entry there must be present in the vendored archive, or the link
will fail at the next compiler bump that emits a call to it.

## See also

- `crates/mty-runtime/src/codegen_abi.rs` — the implementations.
- `crates/mty-runtime/build.rs` — header generator + side table.
- `crates/mty-runtime/src/abi_export.rs` — the Rust-side
  introspection surface (`RUNTIME_ABI_SIGNATURES`,
  `RUNTIME_ABI_VERSION`, `RUNTIME_ABI_HEADER`).
- `crates/mty-cli/src/cmd/abi.rs` — the `mty abi` CLI subcommand.
- `.github/workflows/release.yml` — the per-platform artifact build.
- `mty-language-lessons.md` L51 — the IDE-side history that motivated
  this work.
