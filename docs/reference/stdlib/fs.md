# `std.fs`

Capability-gated filesystem operations.

## Surface

```sd
fn read(fs: Fs, path: Path) -> Bytes!IoErr
fn read_to_string(fs: Fs, path: Path) -> String!IoErr
fn write(fs: Fs, path: Path, data: Bytes) -> Unit!IoErr
fn append(fs: Fs, path: Path, data: Bytes) -> Unit!IoErr
fn exists(fs: Fs, path: Path) -> Bool
fn create_dir_all(fs: Fs, path: Path) -> Unit!IoErr
fn remove_file(fs: Fs, path: Path) -> Unit!IoErr
fn remove_dir_all(fs: Fs, path: Path) -> Unit!IoErr
fn list_dir(fs: Fs, path: Path) -> Vec[Path]!IoErr
fn metadata(fs: Fs, path: Path) -> Metadata!IoErr   # v0.46 T4
fn read_dir(fs: Fs, path: Path) -> DirIter!IoErr    # v0.46 T4
```

The interpreter-backed host dispatcher also accepts the agent-friendly
aliases `read_file`, `write_file`, `write_string`, and `stat`. Those
names map to the same capability checks as the canonical operations
above. The default `mty run` (Cranelift JIT) and `mty build` paths
now lower the whole surface directly through the native runtime ABI
(see "Native runtime ABI" below); the interpreter dispatcher is only
used by `mty test`'s host-driven path and legacy `mty run` fallbacks.

In Rust:

```rust
pub fn read(cap: &FsCap, path: &Path) -> Result<Vec<u8>, IoErr>;
pub fn read_to_string(cap: &FsCap, path: &Path) -> Result<String, IoErr>;
pub fn write(cap: &FsCap, path: &Path, data: &[u8]) -> Result<(), IoErr>;
pub fn append(cap: &FsCap, path: &Path, data: &[u8]) -> Result<(), IoErr>;
pub fn exists(cap: &FsCap, path: &Path) -> bool;
pub fn create_dir_all(cap: &FsCap, path: &Path) -> Result<(), IoErr>;
pub fn remove_file(cap: &FsCap, path: &Path) -> Result<(), IoErr>;
pub fn remove_dir_all(cap: &FsCap, path: &Path) -> Result<(), IoErr>;
pub fn list_dir(cap: &FsCap, path: &Path) -> Result<Vec<PathBuf>, IoErr>;
pub fn metadata(cap: &FsCap, path: &Path) -> Result<Metadata, IoErr>;
pub fn read_dir(cap: &FsCap, path: &Path) -> Result<DirIter, IoErr>;
```

## `Metadata` (v0.46 T4)

```sd
struct Metadata {
  size:     U64,   # bytes, 0 for non-regular files
  mtime_ms: I64,   # ms since UNIX epoch, 0 if unavailable
  is_file:  Bool,
  is_dir:   Bool,
}
```

Returned by `metadata` / `stat`. User code projects fields naturally:

```mty
let md = std.fs.metadata(path)
if md.is_file && md.size > 1024 {
  log("large regular file")
}
```

Pre-v0.46 the runtime ABI wrote the 24-byte struct into a slot but the
typeck side hadn't lifted the ADT, so `md.size` etc. fell through to
the permissive `cx.fresh()` path and the codegen's field projection
read the wrong offsets. v0.46 T4 registers `Metadata` as a prelude
struct ADT (`Metadata { size: U64@0, mtime_ms: I64@8, is_file: Bool@16,
is_dir: Bool@17 }`) so the L15 / `struct_field_offset` machinery
lines the projections up with the runtime ABI's raw-pointer writes.

## `DirIter` (v0.46 T4)

```sd
struct DirIter   # opaque
impl DirIter {
  fn next(self) -> Option<String>
  fn close(self) -> Unit
}
```

Streaming iterator returned by `read_dir`. Entries are
lexicographically sorted (same contract as `list_dir`). Drop on a
`DirIter` calls `close` automatically (v0.47 T4) — source code does
not need to call `close` explicitly. Explicit `close` is still
supported (and idempotent) for early abort. Calling `next` on an
exhausted (or never-opened) iterator returns `None` without trapping.

```mty
# v0.47 — Drop auto-closes; explicit close is OPTIONAL:
let mut it = std.fs.read_dir("./inputs")
while let Some(entry) = it.next() {
  log(entry)
}
# `it` goes out of scope here — runtime drop-close fires automatically.
```

Auto-Drop is driven by the prelude's `#[mty_drop = "..."]` registration
(see `crates/mty-types/src/prelude.rs` —
`mty_drop_fns.insert(DirIter, "mty_runtime_fs_dir_close")`). The IR
lowerer injects a `Stmt::Drop(local)` in front of every fn-exit
terminator for locals typed `DirIter`; the cranelift / llvm backends
lower the drop to a call to `mty_runtime_fs_dir_close(handle)`, which
no-ops on `handle == 0`. Source-level `it.close()` zeroes the
receiver local, so an explicit close followed by the auto-Drop is
safe — the auto-Drop dispatches with handle=0 and the runtime does
nothing.

### Migration from v0.45 / v0.46

`read_dir` returned a newline-joined `Str` in v0.45 T1. v0.46 T4
promoted the iterator handle to the canonical shape and renamed the
old shape to `read_dir_lines` (behind a `#[deprecated(since = "0.46.0")]`
attribute). **v0.47 T4 removed `read_dir_lines` entirely** — agent code
still calling it now fails type-check with MT2021 ("name not found").
Migrate to the iterator handle:

```mty
# v0.45 (REMOVED in v0.47):
let s = std.fs.read_dir_lines("./inputs")

# v0.46+ canonical:
let mut it = std.fs.read_dir("./inputs")
while let Some(_) = it.next() { ... }
```

The runtime ABI symbol `mty_runtime_fs_read_dir` stays exported so
v0.45-built native binaries still link at load time — only the
Mighty-side surface is gone.

## Native runtime ABI

The Cranelift JIT / AOT and LLVM backends lower `std.fs.*` calls
through these C-ABI symbols. Each call is unconditional at the codegen
layer — the capability gate is enforced at typeck (`effect fs`) so the
runtime entrypoints don't need to re-check.

```c
void  mty_runtime_fs_read           (i64 path_ptr, i64 path_len, i64 dst_slot);
void  mty_runtime_fs_read_to_string (i64 path_ptr, i64 path_len, i64 dst_slot);
void  mty_runtime_fs_read_dir       (i64 path_ptr, i64 path_len, i64 dst_slot);
i32   mty_runtime_fs_write          (i64 path_ptr, i64 path_len, i64 data_ptr, i64 data_len);
i32   mty_runtime_fs_write_string   (i64 path_ptr, i64 path_len, i64 data_ptr, i64 data_len);
i32   mty_runtime_fs_append         (i64 path_ptr, i64 path_len, i64 data_ptr, i64 data_len);
i32   mty_runtime_fs_exists         (i64 path_ptr, i64 path_len);
i32   mty_runtime_fs_metadata       (i64 path_ptr, i64 path_len, i64 dst_slot);
i32   mty_runtime_fs_create_dir_all (i64 path_ptr, i64 path_len);
i32   mty_runtime_fs_remove_file    (i64 path_ptr, i64 path_len);
i32   mty_runtime_fs_remove_dir_all (i64 path_ptr, i64 path_len);

# v0.46 T4 — read_dir iterator handle ABI:
i64   mty_runtime_fs_dir_open       (i64 path_ptr, i64 path_len);
i32   mty_runtime_fs_dir_next       (i64 handle, i64 dst_slot);
void  mty_runtime_fs_dir_close      (i64 handle);
```

`dst_slot` is a caller-supplied stack slot the runtime writes into:

- 24-byte `(ptr@+0, len@+8, ok@+16)` triple for the `read*` /
  `read_dir` (legacy v0.45 ABI symbol, kept exported for linkage but
  no longer reached by Mighty-side codegen) / `dir_next` family —
  the Mighty Str layout reads
  offsets +0 / +8 transparently.
- 24-byte `Metadata` record `{ size: u64@+0, mtime_ms: i64@+8,
  is_file: i8@+16, is_dir: i8@+17 }` for `metadata`. The field
  offsets here are part of the surface contract: cranelift's
  `struct_field_offset` reproduces these natural-alignment offsets
  from the prelude `Metadata` ADT, and the runtime writes through
  raw pointers at the same offsets.

`dir_open` returns an opaque i64 handle (0 on open failure). Calling
`dir_next` with `handle == 0` short-circuits to EOF without touching
the slot, and `dir_close(0)` is a no-op so source-side `Drop`
sequences are double-call safe. The handle's iterator state owns a
pre-collected `Vec<PathBuf>` cursor — entries are materialised
eagerly at open time so `dir_next` can't surface I/O errors mid-walk.

## Capability model

Every op takes an `FsCap` carrying an optional prefix-allowlist:

- `FsCap::unrestricted()` — no allowlist (used by trusted CLI entry
  points and tests).
- `FsCap::rooted([root1, root2, ...])` — only paths starting with one
  of the listed roots are allowed; anything else fails with
  `IoErr::Denied(path)`.

The Mighty runtime synthesizes an `FsCap` per agent based on the
agent's manifest `fs.read_paths` / `fs.write_paths` grants. Code that
calls `std.fs` outside an agent context (e.g. `main`) gets an
unrestricted cap.

## Determinism

- `list_dir` returns entries in lexicographic order so callers don't
  depend on the OS-specific `readdir` order.
- `write` recursively creates parent directories if missing (matches
  `std::fs::write` + `create_dir_all`).

## Example

```rust
use sdust_stdlib::fs::{exists, list_dir, read, write, FsCap};
use std::path::Path;

let cap = FsCap::rooted(["/var/data/myapp"]);
write(&cap, Path::new("/var/data/myapp/log.txt"), b"hello").unwrap();
assert!(exists(&cap, Path::new("/var/data/myapp/log.txt")));
let listing = list_dir(&cap, Path::new("/var/data/myapp")).unwrap();
```

## Error mapping

```rust
pub enum IoErr {
    Io(std::io::Error),
    Denied(String),
    Utf8(String),
}
```

`Denied` is surfaced as a trap (SD55xx range) when called from
Mighty source; the runtime's effect-call sink translates the Rust
error into the matching Mighty `IoErr` variant.
