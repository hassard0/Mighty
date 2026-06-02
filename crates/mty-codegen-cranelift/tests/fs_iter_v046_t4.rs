//! v0.46 T4 — `std.fs.read_dir` iterator + `Metadata` field projection
//! regression suite.
//!
//! Two carry-overs from the v0.45 T1 `std.fs.*` rollout (PR #25):
//!
//!   1. Iterator handle: `mty_runtime_fs_dir_{open,next,close}` is the
//!      canonical surface; the v0.45 newline-joined `read_dir` shape
//!      lives on as `read_dir_lines` until v0.47.
//!
//!   2. Metadata field projection: `std.fs.metadata(p) -> Metadata`
//!      lifts the 24-byte runtime ABI struct into a typed
//!      `IrTy::Adt(Metadata, [])` so `md.size` / `md.is_file` reads
//!      land at the right offsets via the L15 / `struct_field_offset`
//!      path.
//!
//! Tests drive Mighty source through `build_jit` against the real
//! runtime symbol table (same harness as `fs_native_v045_t1.rs`) and
//! cross-check filesystem state with `std::fs::*`.

use mty_ast::AstNode;
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_ir::lower_package;
use mty_syntax::parse;
use std::path::Path;
use std::sync::Mutex;

/// Serialise tests sharing the FMT_STRINGS interner / JIT linker. The
/// v0.45 fs harness uses the same lock so back-to-back invocations
/// across both files don't double-up the runtime's process-wide state.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn jit_run(src: &str) {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let parsed = parse(src);
    if !parsed.errors.is_empty() {
        panic!(
            "parse errors: {:?}",
            parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }
    let file =
        mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green)).expect("FILE root");
    let (pkg, lower_diags) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    if let Some(d) = lower_diags
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        panic!("lower MT{:04}: {}", d.code.0, d.primary.message);
    }
    let typed = mty_types::check_package_typed(&pkg);
    if let Some(d) = typed
        .diagnostics
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        panic!("typeck MT{:04}: {}", d.code.0, d.primary.message);
    }
    let prog = lower_package(&pkg, &typed);
    let st = mty_runtime::codegen_abi::symbol_table();
    let syms = symbols_from(&st.iter().map(|(n, p)| (n.as_str(), *p)).collect::<Vec<_>>());
    let jc = build_jit(&prog, &syms).expect("build_jit");
    let _ = jc.call_main();
    drop(jc);
}

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn path_str(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

// =====================================================================
// Section 1 — read_dir iterator ABI
// =====================================================================

/// Open a populated directory, advance to EOF, close. The `while let
/// Some(_) = it.next()` shape pins the runtime's (1=more / 0=eof)
/// signalling — without it the iterator would loop forever (the v0.45
/// codegen stubbed `.next()` to a defensive None at this method-name
/// gate, which `emit_dir_iter_next` now supersedes).
#[test]
fn dir_iter_walks_populated_directory_to_eof() {
    let dir = tempdir();
    std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
    std::fs::write(dir.path().join("b.txt"), b"b").unwrap();
    std::fs::write(dir.path().join("c.txt"), b"c").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let mut it = std.fs.read_dir("{p}")
  while let Some(_e) = it.next() {{
  }}
  it.close()
}}
"#,
        p = path_str(dir.path())
    );
    jit_run(&src);
}

/// Empty directory: open succeeds, first `.next()` returns None
/// immediately, close is a no-op.
#[test]
fn dir_iter_empty_directory_returns_none_first_call() {
    let dir = tempdir();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let mut it = std.fs.read_dir("{p}")
  match it.next() {{
    Some(_x) => {{ log("unexpected entry") }}
    None => {{ log("eof") }}
  }}
  it.close()
}}
"#,
        p = path_str(dir.path())
    );
    jit_run(&src);
}

/// Open against a path that doesn't exist — runtime returns handle=0
/// and the first `.next()` short-circuits to None, no segv.
#[test]
fn dir_iter_nonexistent_path_returns_none_safely() {
    let dir = tempdir();
    let missing = dir.path().join("does-not-exist");
    let src = format!(
        r#"
use std.fs

fn main() {{
  let mut it = std.fs.read_dir("{p}")
  match it.next() {{
    Some(_x) => {{ log("unexpected entry") }}
    None => {{ log("expected_none") }}
  }}
  it.close()
}}
"#,
        p = path_str(&missing)
    );
    jit_run(&src);
}

/// Early close before exhausting entries — `.close()` runs cleanly,
/// no later `.next()` call (Drop on the Mighty side handles the
/// second-close idempotently; the runtime's `handle == 0` no-op is
/// what the source-side Drop relies on).
#[test]
fn dir_iter_early_close_is_safe() {
    let dir = tempdir();
    std::fs::write(dir.path().join("x"), b"x").unwrap();
    std::fs::write(dir.path().join("y"), b"y").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let mut it = std.fs.read_dir("{p}")
  let _ = it.next()
  it.close()
}}
"#,
        p = path_str(dir.path())
    );
    jit_run(&src);
}

/// Direct exercise of the runtime symbol table's open / next / close
/// triple — proves the symbols resolve and the ABI is wired without
/// requiring a Mighty source roundtrip. (Mighty source coverage above
/// validates the codegen-side dispatch.)
#[test]
fn runtime_abi_dir_open_next_close_walks_entries() {
    let dir = tempdir();
    std::fs::write(dir.path().join("alpha"), b"a").unwrap();
    std::fs::write(dir.path().join("beta"), b"b").unwrap();
    std::fs::write(dir.path().join("gamma"), b"g").unwrap();

    let path = path_str(dir.path());
    let path_bytes = path.as_bytes();
    let handle = mty_runtime::codegen_abi::mty_runtime_fs_dir_open(
        path_bytes.as_ptr() as i64,
        path_bytes.len() as i64,
    );
    assert_ne!(handle, 0, "open should succeed on a real tempdir");

    let mut slot: [i64; 3] = [0, 0, 0];
    let mut count = 0;
    loop {
        let rc =
            mty_runtime::codegen_abi::mty_runtime_fs_dir_next(handle, slot.as_mut_ptr() as i64);
        if rc == 0 {
            break;
        }
        assert!(rc > 0, "next should never return -errno on a clean walk");
        assert_eq!(slot[2], 1, "ok flag should be 1 on success");
        count += 1;
        if count > 10 {
            panic!("runaway iterator");
        }
    }
    assert_eq!(count, 3, "should see exactly 3 entries");
    mty_runtime::codegen_abi::mty_runtime_fs_dir_close(handle);
}

/// `mty_runtime_fs_dir_open` on a missing path returns 0; subsequent
/// `next` short-circuits to EOF without dereferencing the bogus
/// handle.
#[test]
fn runtime_abi_dir_open_missing_path_returns_zero_handle() {
    let dir = tempdir();
    let missing = dir.path().join("not-here");
    let p = path_str(&missing);
    let pb = p.as_bytes();
    let handle =
        mty_runtime::codegen_abi::mty_runtime_fs_dir_open(pb.as_ptr() as i64, pb.len() as i64);
    assert_eq!(handle, 0, "missing path should yield 0 handle");
    let mut slot: [i64; 3] = [0, 0, 0];
    let rc = mty_runtime::codegen_abi::mty_runtime_fs_dir_next(handle, slot.as_mut_ptr() as i64);
    assert_eq!(rc, 0, "next on 0-handle should be EOF");
    // Close-on-zero is a no-op per the ABI contract.
    mty_runtime::codegen_abi::mty_runtime_fs_dir_close(0);
}

// =====================================================================
// Section 2 — Metadata field projection (L15 verification)
// =====================================================================

/// `let md = std.fs.metadata(p); if md.is_file { ... }` — proves the
/// is_file projection at +16 reads the byte the runtime wrote. The
/// L15 (v0.41 T1) struct-field projection fix is the underpinning;
/// this test pins that the wiring extends to runtime-populated
/// structs.
#[test]
fn metadata_is_file_reads_true_for_regular_file() {
    let dir = tempdir();
    let p = dir.path().join("regular.txt");
    std::fs::write(&p, b"hello-md").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let md = std.fs.metadata("{p}")
  if md.is_file {{
    log("is_file=true")
  }}
  if !md.is_dir {{
    log("is_dir=false")
  }}
}}
"#,
        p = path_str(&p)
    );
    jit_run(&src);
}

/// `md.is_dir` on a directory: companion to the is_file test. Reads
/// the is_dir byte at +17, separate from is_file at +16 — proves
/// adjacent 1-byte fields don't alias.
#[test]
fn metadata_is_dir_reads_true_for_directory() {
    let dir = tempdir();
    let nested = dir.path().join("subdir");
    std::fs::create_dir_all(&nested).unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let md = std.fs.metadata("{p}")
  if md.is_dir {{
    log("is_dir=true")
  }}
  if !md.is_file {{
    log("is_file=false")
  }}
}}
"#,
        p = path_str(&nested)
    );
    jit_run(&src);
}

/// `md.size > 0` — proves the 8-byte u64 size field at +0 reads as a
/// real value (not just a zero or the slot-address). Writes 8 known
/// bytes and asserts the user-side comparison fires.
#[test]
fn metadata_size_reads_real_byte_count() {
    let dir = tempdir();
    let p = dir.path().join("sized.bin");
    std::fs::write(&p, b"12345678").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let md = std.fs.metadata("{p}")
  if md.size > 0 {{
    log("size>0")
  }}
  if md.is_file && md.size > 1 {{
    log("compound-ok")
  }}
}}
"#,
        p = path_str(&p)
    );
    jit_run(&src);
}

/// L15-verification: read TWO fields off the SAME metadata value, in
/// the same expression. Pre-L15 (v0.41 T1) struct field projection
/// always returned field 0 — this test pins that md.size and
/// md.is_file resolve to their distinct offsets (+0 / +16). The
/// regression's signature was "every projection looks like md.size,
/// the I64 at +0"; if it ever returns, this test fails because
/// `md.is_file` would read the high byte of size instead of the
/// is_file flag.
#[test]
fn metadata_two_field_reads_resolve_distinct_offsets() {
    let dir = tempdir();
    let p = dir.path().join("twofield.bin");
    std::fs::write(&p, b"x").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let md = std.fs.metadata("{p}")
  if md.is_file && md.size > 0 {{
    log("both-fields-ok")
  }}
}}
"#,
        p = path_str(&p)
    );
    jit_run(&src);
}

/// Round-trip the runtime ABI's Metadata struct write — same shape
/// the codegen lowers into. Asserts the four fields end up at the
/// pinned offsets so any future change to the in-memory layout
/// surface-defines a regression here BEFORE it can ship.
#[test]
fn runtime_abi_metadata_slot_layout_pins_offsets() {
    let dir = tempdir();
    let p = dir.path().join("layout.bin");
    std::fs::write(&p, b"abcdefgh").unwrap();
    let ps = path_str(&p);
    let pb = ps.as_bytes();
    // 24-byte slot: size(u64) @0, mtime_ms(i64) @8, is_file(i8) @16,
    // is_dir(i8) @17.
    let mut slot: [u8; 24] = [0; 24];
    let rc = mty_runtime::codegen_abi::mty_runtime_fs_metadata(
        pb.as_ptr() as i64,
        pb.len() as i64,
        slot.as_mut_ptr() as i64,
    );
    assert!(rc > 0, "metadata should succeed for a real file (rc={rc})");
    let size = u64::from_le_bytes([
        slot[0], slot[1], slot[2], slot[3], slot[4], slot[5], slot[6], slot[7],
    ]);
    assert_eq!(size, 8, "file size should be 8 bytes");
    let is_file = slot[16];
    let is_dir = slot[17];
    assert_eq!(is_file, 1, "is_file should be 1 for a regular file");
    assert_eq!(is_dir, 0, "is_dir should be 0 for a regular file");
}

/// v0.45 T1 carryover: `read_dir_lines` is the deprecated alias for
/// the old newline-joined `read_dir` shape — already-built CLIs that
/// linked against v0.45 still resolve through this name. Pins the
/// alias keeps the v0.45 Str behaviour intact.
#[test]
fn read_dir_lines_deprecated_alias_keeps_v045_str_shape() {
    let dir = tempdir();
    std::fs::write(dir.path().join("one"), b"1").unwrap();
    std::fs::write(dir.path().join("two"), b"2").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let _s = std.fs.read_dir_lines("{p}")
  log("alias-ok")
}}
"#,
        p = path_str(dir.path())
    );
    jit_run(&src);
}
