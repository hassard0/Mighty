# `std.fs`

Capability-gated filesystem operations.

## Surface

```sd
fn read(fs: Fs, path: Path) -> Bytes!IoErr
fn write(fs: Fs, path: Path, data: Bytes) -> Unit!IoErr
fn exists(fs: Fs, path: Path) -> Bool
fn list_dir(fs: Fs, path: Path) -> Vec[Path]!IoErr
```

In Rust:

```rust
pub fn read(cap: &FsCap, path: &Path) -> Result<Vec<u8>, IoErr>;
pub fn write(cap: &FsCap, path: &Path, data: &[u8]) -> Result<(), IoErr>;
pub fn exists(cap: &FsCap, path: &Path) -> bool;
pub fn list_dir(cap: &FsCap, path: &Path) -> Result<Vec<PathBuf>, IoErr>;
```

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
