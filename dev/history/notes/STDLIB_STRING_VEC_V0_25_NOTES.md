# Stdlib String + Vec[T] — v0.25 Track E

**Slice:** v0.25 Track E (foundational `std.String` + `std.Vec[T]`).
**Status:** shipped.
**Owner files:** `crates/mty-stdlib/src/{string,vec}.rs`,
`crates/mty-stdlib/tests/{string_real,vec_basic}.rs`,
`examples/26_string_vec.mty`.

## What shipped

### `std.String` (`crates/mty-stdlib/src/string.rs`)

Owned, growable, UTF-8 byte string. Wraps a `Vec<u8>` so the byte
buffer is shared with `crate::vec::Vec<u8>` for the wasm linear-memory
layout. Methods:

| Mighty                  | Rust impl                              | Notes                                |
|-------------------------|----------------------------------------|--------------------------------------|
| `String.new()`          | `String::new`                          | empty                                |
| `String.with_capacity(n)` | `String::with_capacity`              | pre-allocates `n` bytes              |
| `String.from_str(s)`    | `String::from_str`                     | clones a borrowed `&str`             |
| `String.from_utf8(bs)`  | `String::from_utf8`                    | re-validates; returns `Result`       |
| `s.len()`               | `len`                                  | **byte** count (UTF-8), NOT chars    |
| `s.is_empty()`          | `is_empty`                             |                                      |
| `s.push_str(t)`         | `push_str`                             |                                      |
| `s.push(c)`             | `push`                                 | one `char`                           |
| `s.clear()`             | `clear`                                | resets length, preserves capacity    |
| `s.as_str()`            | `as_str`                               | borrow                               |
| `s.to_str()`            | `to_str`                               | alias of `as_str` for format-macro   |

Plus host-internal helpers: `capacity`, `as_bytes`,
`Display`/`Debug` impls, `From<&str>`,
`From<std::string::String>`/`Into`.

The type deliberately avoids `unsafe`: every UTF-8 re-validation that
`std::string::String` skips with `from_utf8_unchecked`, we redo
through `std::str::from_utf8`. The ~5% throughput hit on `push_str` is
acceptable because Mighty's stdlib is supposed to be the trust anchor.

### `std.Vec[T]` (`crates/mty-stdlib/src/vec.rs`)

Generic, growable array. `#[repr(transparent)]` over
`std::vec::Vec<T>` so the storage layout is identical to what the
wasm Component ABI's `list<T>` lowers to.

| Mighty                  | Rust impl                              | Notes                                |
|-------------------------|----------------------------------------|--------------------------------------|
| `Vec.new()`             | `Vec::new`                             | empty                                |
| `Vec.with_capacity(n)`  | `Vec::with_capacity`                   |                                      |
| `v.push(x)`             | `push`                                 |                                      |
| `v.pop()`               | `pop`                                  | returns `Option[T]`                  |
| `v.len()`               | `len`                                  |                                      |
| `v.is_empty()`          | `is_empty`                             |                                      |
| `v.clear()`             | `clear`                                | preserves capacity                   |
| `v.get(i)`              | `get`                                  | `Option[&T]`                         |
| `v.get_mut(i)`          | `get_mut`                              | `Option[&mut T]`                     |
| `v.iter()`              | `iter`                                 | `&T`                                 |
| `v[i] = x`              | `IndexMut`                             |                                      |

Plus host-internal helpers: `as_slice` / `as_mut_slice`, `iter_mut`,
`capacity`, `append`, `from_elem` (for `T: Clone`), `IntoIterator`
impls, `FromIterator`, `from_std_vec` / `into_std_vec`.

Special-cased element types verified in the test module:
`Vec<u8>` (wasm `list<u8>`), `Vec<u32>` (Tetris board), and
`Vec<String>` (collections of names).

## Mighty-source binding

The methods are callable from Mighty source via three integration
points:

1. **Permissive method table** — `crates/mty-types/src/prelude.rs`
   registers `with_capacity`, `from_str`, `from_utf8`, `push_str`,
   `clear`, `get_mut`, `as_slice`, `as_mut_slice`, `capacity` in the
   `permissive_methods` array so the typechecker accepts them on any
   receiver.
2. **Vec[T] as a type** — `prelude.rs` registers `Vec` as a generic
   opaque ADT (mirrors `AgentRef[T]`) so `Vec[U32]` parses as a type
   position. `String` was already registered.
3. **SIR interpreter dispatch** —
   `crates/mty-ir/src/interp/run.rs::eval_method` gets new arms for
   the v0.25 methods (`with_capacity`, `from_str`, `from_utf8`,
   `get_mut`, `as_slice`, `as_mut_slice`, `capacity`). Receiver-less
   constructors (`String.new()`, `Vec.with_capacity(200)`) hit
   `Interp::call_builtin::BuiltinId::Extern` and route through a new
   `try_stdlib_ctor` helper that synthesises the matching value
   before falling back to the host extern table.

## Example

`examples/26_string_vec.mty` exercises the full surface:

```mty
let mut s = String.with_capacity(32)
s.push_str("score: ")
s.push_str(format!("{}", n))
s.clear()

let mut board = Vec.with_capacity(200)
board.push(0_u32)
board[0] = 7_u32
```

Typechecks via `cargo run -q -p mty-cli -- check examples/26_string_vec.mty`.

## Build / test gate

```
cargo build --workspace
cargo test -p mty-stdlib --test string_real --test vec_basic
cargo test -p mty-stdlib --lib string
cargo test -p mty-stdlib --lib vec
cargo run -q -p mty-cli -- check examples/26_string_vec.mty
```

22 dedicated tests pass: 11 in `tests/string_real.rs` + 11 in
`tests/vec_basic.rs`, plus 14 inline String + 12 inline Vec
`#[cfg(test)] mod tests` cases.

## What Track F (demo 06 V2) can now consume

* `String` as a real owned UTF-8 buffer for accumulating per-frame
  HUD strings and entity name lists.
* `Vec[U32]` for the Tetris 10×20 = 200-cell board flat array. The
  v0.23 / v0.24 demos punted on board-state in Mighty because there
  was no Vec-of-U32; Track F's demo 06 V2 can index `board[row * 10 + col]`
  through `IndexMut` straight from Mighty source.
* `Vec[U8]` for sprite-data byte buffers and any future net-frame
  shape the canvas-game agent emits.
* `Vec[String]` for collections of strings (entity names, log lines).

## v0.26 follow-ups

* The interpreter still treats Vec values as `Value::Array(Vec<Value>)`
  with by-clone semantics; the `get_mut` arm returns the same
  `Option[T]` shape as `get` instead of a true mutable reference.
  Mutations go through `Stmt::Assign` on the indexed place (which
  works today via the existing `assign_place` deref-write path).
  A true `&mut T` model is a v0.26 deliverable that needs to wait
  for the borrow-checker integration.
* `Vec.from_elem(v, n)` (Rust's `vec![v; n]`) is implemented in the
  host-side Rust impl but not yet wired through the Mighty-source
  permissive method table. Trivial v0.26 add — just one entry plus
  a `dyn`-friendly clone path in the interp.
* `String.push_back_bytes(bs: &[U8])` and `String.split_off(at)`
  for the v0.26 self-host text-utility chunk.
* Cabi_realloc layout integration: the v0.18 wasm allocator hands
  buffer (ptr, len, cap) triples back to the host; the `Vec`'s
  `from_std_vec` / `into_std_vec` round-trips are zero-copy so the
  wasm-side packer can claim ownership without an intermediate copy.

## Coordination notes (v0.25 swarm)

Track D's in-flight `format!` width/precision work added
`FormatExpandError::{BadWidth, BadPrecision}` variants to
`mty-macros::stdlib::format::FormatExpandError` but didn't update
`crates/mty-hir/src/lower/macros.rs::diag_format_error` to match them
exhaustively. That's a 1-block patch — added here as a coordination
fix so the workspace builds while Track D continues on the runtime
helpers; remove it from this slice's diff and refold it into Track
D's PR if/when their slice lands first.
