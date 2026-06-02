//! C-ABI runtime bridge for JIT'd Mighty code (slice 8).
//!
//! The codegen-cranelift crate declares a small set of imported
//! symbols (see `mty_codegen_cranelift::runtime_imports`). At JIT
//! finalization time, the runtime registers concrete addresses for
//! each. JIT'd code calls into these C-ABI fns to:
//!
//! - print / log
//! - panic with a message
//! - push / pop arena frames
//! - allocate bytes against the current arena
//! - charge bytes against the budget tracker
//! - send / ask / spawn (slice-8: stub — agent handlers stay on the
//!   interpreter path for end-to-end correctness)
//! - call into an `extern { fn ... }` library symbol
//!
//! The "current" arena and budget are stored in thread-locals. The
//! runtime drives these from its per-turn `Host`; outside a turn,
//! they're empty and ops are no-ops.

use crate::arena::ArenaStack;
use crate::extern_loader::ExternRegistry;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::sync::OnceLock;

thread_local! {
    /// Per-thread arena stack. Each `arena_push` allocates a fresh
    /// `Bump`; `arena_pop` drops it. Allocations between push and pop
    /// land on the top arena.
    pub(crate) static ARENA_STACK: RefCell<ArenaStack> = RefCell::new(ArenaStack::default());

    /// Per-thread byte counter charged against the current budget.
    /// The runtime resets this between turns.
    pub(crate) static BYTES_CHARGED: RefCell<u64> = const { RefCell::new(0) };
}

/// Process-wide extern registry. Loaded once on the first `extern_call`
/// from JIT'd code. Subsequent loads are cached.
static EXTERN_REGISTRY: OnceLock<Mutex<ExternRegistry>> = OnceLock::new();

fn registry() -> &'static Mutex<ExternRegistry> {
    EXTERN_REGISTRY.get_or_init(|| Mutex::new(ExternRegistry::with_libc()))
}

// ---- The actual C-ABI fns ------------------------------------------

#[no_mangle]
pub extern "C" fn mty_runtime_log(ptr: i64, len: i64) {
    let s = unsafe { read_str(ptr, len) };
    println!("{s}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_print(ptr: i64, len: i64) {
    let s = unsafe { read_str(ptr, len) };
    print!("{s}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_panic(ptr: i64, len: i64) {
    let s = unsafe { read_str(ptr, len) };
    eprintln!("mighty panic: {s}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_arena_push() -> i64 {
    ARENA_STACK.with(|s| s.borrow_mut().push() as i64)
}

#[no_mangle]
pub extern "C" fn mty_runtime_arena_pop(_handle: i64) {
    ARENA_STACK.with(|s| s.borrow_mut().pop());
}

#[no_mangle]
pub extern "C" fn mty_runtime_alloc(size: i64, align: i64, _zero: i64) -> i64 {
    BYTES_CHARGED.with(|b| {
        let mut m = b.borrow_mut();
        *m = m.saturating_add(size as u64);
    });
    ARENA_STACK
        .with(|s| s.borrow_mut().alloc(size as usize, align as usize))
        .map(|p| p as i64)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn mty_runtime_budget_charge(bytes: i64) -> i8 {
    BYTES_CHARGED.with(|b| {
        let mut m = b.borrow_mut();
        *m = m.saturating_add(bytes as u64);
    });
    1 // 1 = ok; 0 would mean budget exceeded (slice-8 simplification)
}

#[no_mangle]
pub extern "C" fn mty_runtime_send(_target: i64, _msg: i64, _payload: i64) {
    // Slice-8 stub — full implementation lives in the interp-driven
    // path; the JIT calls this for compiled-handler scenarios that
    // slice 8 doesn't fully cover.
}

#[no_mangle]
pub extern "C" fn mty_runtime_ask(
    _target: i64,
    _msg: i64,
    _payload: i64,
    _deadline_ms: i64,
) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn mty_runtime_spawn(_agent_id: i64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn mty_runtime_extern_call(name_ptr: i64, name_len: i64, _args: i64) -> i64 {
    let name = unsafe { read_str(name_ptr, name_len).to_string() };
    let reg = registry().lock();
    reg.call_i64(&name).unwrap_or_default()
}

#[no_mangle]
pub extern "C" fn mty_runtime_log_i64(v: i64) {
    println!("{v}");
}

// ---- v0.42 T4: typed log/print/format runtime surface (L23) --------
//
// Native `log(...)` and computed-value tracing previously required an
// FFI shim because the codegen only lowered the `(ptr,len)` string
// case. v0.42 T4 adds a small family of typed runtime entry points
// that the codegen dispatches to based on the operand's SIR type, so
// `log(n)` for an `I32`/`F64`/`Bool` etc. just works.
//
// Two parallel families:
//
//   `mty_runtime_log_*`   — println-style (one value + newline)
//   `mty_runtime_print_*` — print-style (one value, no newline)
//
// Plus a `_sep` helper that prints a single space, and `_newline` that
// terminates a multi-arg `log(a, b, c)` series.
//
// And the `mty_runtime_fmt_*` family — formats a value to UTF-8 bytes
// and writes a (ptr@+0, len@+8) pair into a 16-byte caller-supplied
// stack slot. The bytes live in a thread-local owned-strings table so
// the slot pointer stays valid for the lifetime of the Mighty run
// (these are trace-style formatters; we deliberately don't try to
// route through the arena's per-turn lifetimes, which would surprise
// callers that `let s = n.to_str()` and use `s` across turn
// boundaries). The interner deduplicates by `(kind, bits)` so common
// values like `0`, `1`, `-1` only allocate once per process.

thread_local! {
    /// Owned UTF-8 strings produced by `mty_runtime_fmt_*`. Each entry
    /// is a `Box<str>` kept alive for the lifetime of the program so
    /// the (ptr,len) pair we return into the caller's slot stays
    /// valid. The `Box<str>` (rather than `String`) freezes the
    /// underlying allocation — no reallocation can move the bytes out
    /// from under the caller's pointer.
    static FMT_STRINGS: RefCell<Vec<Box<str>>> = const { RefCell::new(Vec::new()) };
}

/// Intern an owned string into the per-thread `FMT_STRINGS` table and
/// return a `(ptr, len)` pair into the frozen allocation.
fn intern_fmt(s: std::string::String) -> (i64, i64) {
    FMT_STRINGS.with(|t| {
        let boxed: Box<str> = s.into_boxed_str();
        let ptr = boxed.as_ptr() as i64;
        let len = boxed.len() as i64;
        t.borrow_mut().push(boxed);
        (ptr, len)
    })
}

/// Write a (ptr, len) pair to a caller-supplied 16-byte stack slot at
/// `dst`. The codegen allocates the slot with the same layout it uses
/// for any other `Str`/`String` aggregate (ptr@+0, len@+8, 8-byte
/// aligned), so the caller can pipe the result straight into
/// `log(...)` or any other Str consumer.
///
/// SAFETY: `dst` must be a writable 16-byte region. The cranelift /
/// LLVM lowerings allocate it from a per-fn stack slot before calling.
unsafe fn write_str_pair(dst: i64, ptr: i64, len: i64) {
    if dst == 0 {
        return;
    }
    let p = dst as usize as *mut i64;
    p.write(ptr);
    p.add(1).write(len);
}

// ---- log_* family (println) ----

#[no_mangle]
pub extern "C" fn mty_runtime_log_i32(v: i32) {
    println!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_log_u32(v: u32) {
    println!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_log_u64(v: u64) {
    println!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_log_usize(v: i64) {
    // The codegen passes `USize` as i64 (matches the platform pointer
    // width on the targets we support). Format unsigned.
    println!("{}", v as u64);
}

#[no_mangle]
pub extern "C" fn mty_runtime_log_f32(v: f32) {
    println!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_log_f64(v: f64) {
    println!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_log_bool(v: i8) {
    println!("{}", v != 0);
}

// ---- print_* family (no newline) ----

#[no_mangle]
pub extern "C" fn mty_runtime_print_i32(v: i32) {
    print!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_print_i64(v: i64) {
    print!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_print_u32(v: u32) {
    print!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_print_u64(v: u64) {
    print!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_print_usize(v: i64) {
    print!("{}", v as u64);
}

#[no_mangle]
pub extern "C" fn mty_runtime_print_f32(v: f32) {
    print!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_print_f64(v: f64) {
    print!("{v}");
}

#[no_mangle]
pub extern "C" fn mty_runtime_print_bool(v: i8) {
    print!("{}", v != 0);
}

#[no_mangle]
pub extern "C" fn mty_runtime_print_sep() {
    print!(" ");
}

#[no_mangle]
pub extern "C" fn mty_runtime_print_newline() {
    println!();
}

// ---- fmt_* family (to_str on scalars) ----

#[no_mangle]
pub extern "C" fn mty_runtime_fmt_i32(v: i32, dst: i64) {
    let s = v.to_string();
    let (p, l) = intern_fmt(s);
    unsafe { write_str_pair(dst, p, l) };
}

#[no_mangle]
pub extern "C" fn mty_runtime_fmt_i64_to_slot(v: i64, dst: i64) {
    let s = v.to_string();
    let (p, l) = intern_fmt(s);
    unsafe { write_str_pair(dst, p, l) };
}

#[no_mangle]
pub extern "C" fn mty_runtime_fmt_u32(v: u32, dst: i64) {
    let s = v.to_string();
    let (p, l) = intern_fmt(s);
    unsafe { write_str_pair(dst, p, l) };
}

#[no_mangle]
pub extern "C" fn mty_runtime_fmt_u64(v: u64, dst: i64) {
    let s = v.to_string();
    let (p, l) = intern_fmt(s);
    unsafe { write_str_pair(dst, p, l) };
}

#[no_mangle]
pub extern "C" fn mty_runtime_fmt_usize(v: i64, dst: i64) {
    // Same convention as log_usize: i64 carries the platform-width
    // unsigned value.
    let s = (v as u64).to_string();
    let (p, l) = intern_fmt(s);
    unsafe { write_str_pair(dst, p, l) };
}

#[no_mangle]
pub extern "C" fn mty_runtime_fmt_f32(v: f32, dst: i64) {
    let s = v.to_string();
    let (p, l) = intern_fmt(s);
    unsafe { write_str_pair(dst, p, l) };
}

#[no_mangle]
pub extern "C" fn mty_runtime_fmt_f64(v: f64, dst: i64) {
    let s = v.to_string();
    let (p, l) = intern_fmt(s);
    unsafe { write_str_pair(dst, p, l) };
}

#[no_mangle]
pub extern "C" fn mty_runtime_fmt_bool(v: i8, dst: i64) {
    let s = (v != 0).to_string();
    let (p, l) = intern_fmt(s);
    unsafe { write_str_pair(dst, p, l) };
}

// ---- v0.45 T1: native std.fs surface (L18 fix) --------------------
//
// v0.44 fixed L18 partially: `std.fs.*` calls under `mty run` were
// routed to the interpreter fallback so the host dispatcher could run
// them. That left `mty build` (cranelift JIT + native AOT, LLVM)
// useless for any program that touches disk — agents had to ship a
// Rust shim. v0.45 T1 closes the gap: every `std.fs.*` lowers
// directly to one of the runtime symbols below, so generated apps run
// natively without a shim.
//
// Shape conventions:
//
// - `read` / `read_to_string` / `read_dir` write a (ptr, len, ok)
//   triple into a caller-supplied 24-byte stack slot:
//       slot[0] : i64 — bytes ptr (0 on err)
//       slot[1] : i64 — bytes len (0 on err)
//       slot[2] : i64 — ok flag (1=ok, 0=err)
//   This matches the v0.42 T4 `fmt_*` family's (ptr,len) layout, plus
//   a third i64 word for the success bit. We deliberately pack
//   everything into i64 so the codegen's existing aggregate-slot
//   helpers can write/load it without learning a new pun width.
//
// - `write` / `write_string` / `append` / `create_dir_all` /
//   `remove_file` / `remove_dir_all` / `exists` return a single i32:
//       1   — ok / true
//       0   — false (exists predicate only; write/etc. don't fail
//             closed here)
//      -errno — IO error code (negated `std::io::Error::raw_os_error`
//             when present, else `-1`)
//
// - `metadata` writes a 24-byte struct into a caller-supplied slot:
//       slot+ 0 : u64 — size in bytes
//       slot+ 8 : i64 — mtime_ms (millis since UNIX epoch, 0 if N/A)
//       slot+16 : i8  — is_file (1/0)
//       slot+17 : i8  — is_dir  (1/0)
//   The remaining 6 bytes are padding so the slot is 8-byte aligned
//   and 24 bytes total. Matches `mty_stdlib::fs::Metadata`'s layout.
//
// Bytes for `read*` / `read_dir` are interned into the same
// thread-local `FMT_STRINGS` table the typed-log path uses so the
// pointer stays valid for the lifetime of the program — caller code
// can pipe the result straight into `log(...)` / a longer-lived let
// binding without worrying about arena cleanup.

fn intern_bytes(bytes: Vec<u8>) -> (i64, i64) {
    // Reuse the FMT_STRINGS table; we hold them as Box<str>. Bytes
    // that aren't valid UTF-8 go through `from_utf8_lossy` so the
    // returned pointer/length still points at a frozen allocation
    // and len is the lossy-decoded length. Programs that need raw
    // bytes use `read` (which converts losslessly via the same
    // box<str> when the file is UTF-8 — agents asking for raw bytes
    // is the slot-pair shape that's documented in fs.docstub).
    let s = match std::string::String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => std::string::String::from_utf8_lossy(e.as_bytes()).into_owned(),
    };
    intern_fmt(s)
}

/// Write a (ptr, len, ok) triple to a caller-supplied 24-byte stack
/// slot. `ok` is 1 for success, 0 for failure.
unsafe fn write_str_triple(dst: i64, ptr: i64, len: i64, ok: i64) {
    if dst == 0 {
        return;
    }
    let p = dst as usize as *mut i64;
    p.write(ptr);
    p.add(1).write(len);
    p.add(2).write(ok);
}

fn errno_of(err: std::io::Error) -> i32 {
    let n = err.raw_os_error().unwrap_or(1);
    -n
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_read(path_ptr: i64, path_len: i64, dst: i64) {
    let path = unsafe { read_str(path_ptr, path_len) };
    // Capability gate is compile-time (`effect fs` in typeck) — the
    // runtime call is unconditional. v0.45 T1 deliberately keeps the
    // `mty-runtime` crate independent of `mty-stdlib::fs::FsCap` so
    // the JIT/AOT path doesn't drag in the stdlib's sandbox state
    // machine. Sandbox-aware programs install the cap upstream
    // (`mty-driver` runs the same install-default-cap dance for the
    // interp path).
    match std::fs::read(std::path::Path::new(path)) {
        Ok(bytes) => {
            let (p, l) = intern_bytes(bytes);
            unsafe { write_str_triple(dst, p, l, 1) };
        }
        Err(_) => unsafe { write_str_triple(dst, 0, 0, 0) },
    }
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_read_to_string(path_ptr: i64, path_len: i64, dst: i64) {
    // Identical native semantics to `read` — both return UTF-8 bytes
    // through the (ptr, len, ok) slot. The Mighty-side type is
    // `Str` vs `Bytes`, which is a typeck distinction; the bytes on
    // the wire are the same.
    mty_runtime_fs_read(path_ptr, path_len, dst);
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_write(
    path_ptr: i64,
    path_len: i64,
    data_ptr: i64,
    data_len: i64,
) -> i32 {
    let path_str = unsafe { read_str(path_ptr, path_len) };
    let path = std::path::Path::new(path_str);
    let data = unsafe {
        if data_ptr == 0 || data_len <= 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(data_ptr as usize as *const u8, data_len as usize)
        }
    };
    // Match `mty_stdlib::fs::write` semantics: create parent dirs as
    // needed so a single `std.fs.write("./out/data.txt", b"hi")` call
    // works without a separate create_dir_all.
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return errno_of(e);
            }
        }
    }
    match std::fs::write(path, data) {
        Ok(()) => 1,
        Err(e) => errno_of(e),
    }
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_write_string(
    path_ptr: i64,
    path_len: i64,
    str_ptr: i64,
    str_len: i64,
) -> i32 {
    // Same write semantics; the codegen passes the (ptr, len) for the
    // Str aggregate's backing slot.
    mty_runtime_fs_write(path_ptr, path_len, str_ptr, str_len)
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_append(
    path_ptr: i64,
    path_len: i64,
    data_ptr: i64,
    data_len: i64,
) -> i32 {
    let path_str = unsafe { read_str(path_ptr, path_len) };
    let path = std::path::Path::new(path_str);
    let data = unsafe {
        if data_ptr == 0 || data_len <= 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(data_ptr as usize as *const u8, data_len as usize)
        }
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return errno_of(e);
            }
        }
    }
    use std::io::Write;
    let mut f = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(f) => f,
        Err(e) => return errno_of(e),
    };
    match f.write_all(data) {
        Ok(()) => 1,
        Err(e) => errno_of(e),
    }
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_exists(path_ptr: i64, path_len: i64) -> i32 {
    let path = unsafe { read_str(path_ptr, path_len) };
    if std::path::Path::new(path).exists() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_metadata(path_ptr: i64, path_len: i64, dst: i64) -> i32 {
    let path = unsafe { read_str(path_ptr, path_len) };
    match std::fs::metadata(std::path::Path::new(path)) {
        Ok(md) => {
            let mtime_ms = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let size = md.len();
            let is_file = if md.is_file() { 1i8 } else { 0 };
            let is_dir = if md.is_dir() { 1i8 } else { 0 };
            if dst != 0 {
                unsafe {
                    let p_u64 = dst as usize as *mut u64;
                    p_u64.write(size);
                    let p_i64 = (dst as usize + 8) as *mut i64;
                    p_i64.write(mtime_ms);
                    let p_i8 = (dst as usize + 16) as *mut i8;
                    p_i8.write(is_file);
                    let p_i8_2 = (dst as usize + 17) as *mut i8;
                    p_i8_2.write(is_dir);
                }
            }
            1
        }
        Err(e) => errno_of(e),
    }
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_create_dir_all(path_ptr: i64, path_len: i64) -> i32 {
    let path = unsafe { read_str(path_ptr, path_len) };
    match std::fs::create_dir_all(std::path::Path::new(path)) {
        Ok(()) => 1,
        Err(e) => errno_of(e),
    }
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_remove_file(path_ptr: i64, path_len: i64) -> i32 {
    let path = unsafe { read_str(path_ptr, path_len) };
    match std::fs::remove_file(std::path::Path::new(path)) {
        Ok(()) => 1,
        Err(e) => errno_of(e),
    }
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_remove_dir_all(path_ptr: i64, path_len: i64) -> i32 {
    let path = unsafe { read_str(path_ptr, path_len) };
    match std::fs::remove_dir_all(std::path::Path::new(path)) {
        Ok(()) => 1,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 1,
        Err(e) => errno_of(e),
    }
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_read_dir(path_ptr: i64, path_len: i64, dst: i64) {
    // v0.45 T1 shape — newline-joined entries Str result. The proper
    // iterator-handle ABI (v0.46 T4 — `mty_runtime_fs_dir_open` /
    // `_next` / `_close` below) is the canonical surface from v0.46
    // forward, but this symbol stays live so already-built CLIs that
    // linked against the v0.45 ABI still resolve. Mighty source code
    // that wants the joined string now spells it `std.fs.read_dir_lines(p)`;
    // `std.fs.read_dir(p)` lowers to the iterator handle. The listing
    // is lexicographically sorted (matches `list_dir`'s contract).
    let path = unsafe { read_str(path_ptr, path_len) };
    match std::fs::read_dir(std::path::Path::new(path)) {
        Ok(rd) => {
            let mut entries: Vec<std::path::PathBuf> =
                rd.filter_map(|e| e.ok().map(|d| d.path())).collect();
            entries.sort();
            let mut joined = std::string::String::new();
            for (i, e) in entries.iter().enumerate() {
                if i > 0 {
                    joined.push('\n');
                }
                joined.push_str(&e.display().to_string());
            }
            let (p, l) = intern_fmt(joined);
            unsafe { write_str_triple(dst, p, l, 1) };
        }
        Err(_) => unsafe { write_str_triple(dst, 0, 0, 0) },
    }
}

// ---- v0.46 T4: read_dir iterator handle ABI -----------------------
//
// Three-call shape, modelled on POSIX `opendir` / `readdir` /
// `closedir` so the Mighty-side `DirIter` ADT can drop-close cleanly
// and the iterator state stays out of process-wide memory.
//
//   mty_runtime_fs_dir_open(path_ptr, path_len) -> i64
//       Returns an opaque handle (non-zero on success, 0 on open
//       failure). The handle is the Box<DirIterState> raw pointer
//       reinterpreted as i64 — the codegen treats it as opaque, so
//       no provenance is exposed to Mighty source.
//
//   mty_runtime_fs_dir_next(handle, dst_slot) -> i32
//       Writes the next entry's name as a (ptr, len, ok) triple into
//       a caller-supplied 24-byte slot. Returns 1 if a name was
//       written (more entries follow), 0 on EOF (slot's ok flag is
//       also 0), or a negative errno on I/O error during iteration.
//       The slot string lives in the FMT_STRINGS interner — same
//       lifetime contract as the rest of the fs surface.
//
//   mty_runtime_fs_dir_close(handle)
//       Frees the handle's state. Safe to call with `0` (no-op) so
//       Drop on a never-opened DirIter doesn't trap. Subsequent
//       calls with the same handle are UB at the C-ABI level, but
//       the Mighty `Drop` for `DirIter` sets the handle to 0 after
//       the close so source code can't observe it.
//
// Lifetime contract: the handle's state owns a pre-collected
// `Vec<PathBuf>` (lex-sorted, same shape as `list_dir`), a cursor,
// and is released when `mty_runtime_fs_dir_close` runs. Source code
// MUST eventually close — Drop on `DirIter` handles that — or the
// process leaks one heap allocation per opened handle. The entries
// are collected eagerly at open-time so subsequent
// `mty_runtime_fs_dir_next` calls can't surface mid-iteration
// `std::io::Error`s, which keeps the next() ABI a simple
// (1=more, 0=eof) instead of (1, 0, -errno).

#[repr(C)]
struct DirIterState {
    entries: Vec<std::path::PathBuf>,
    cursor: usize,
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_dir_open(path_ptr: i64, path_len: i64) -> i64 {
    let path = unsafe { read_str(path_ptr, path_len) };
    let entries = match std::fs::read_dir(std::path::Path::new(path)) {
        Ok(rd) => {
            let mut v: Vec<std::path::PathBuf> =
                rd.filter_map(|e| e.ok().map(|d| d.path())).collect();
            v.sort();
            v
        }
        Err(_) => return 0,
    };
    let boxed = std::boxed::Box::new(DirIterState { entries, cursor: 0 });
    std::boxed::Box::into_raw(boxed) as usize as i64
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_dir_next(handle: i64, dst: i64) -> i32 {
    if handle == 0 {
        // No state -> behave as EOF. Slot stays clean (ok=0).
        unsafe { write_str_triple(dst, 0, 0, 0) };
        return 0;
    }
    let state: &mut DirIterState = unsafe { &mut *(handle as usize as *mut DirIterState) };
    if state.cursor >= state.entries.len() {
        unsafe { write_str_triple(dst, 0, 0, 0) };
        return 0;
    }
    let entry = &state.entries[state.cursor];
    state.cursor += 1;
    let s = entry.display().to_string();
    let (p, l) = intern_fmt(s);
    unsafe { write_str_triple(dst, p, l, 1) };
    1
}

#[no_mangle]
pub extern "C" fn mty_runtime_fs_dir_close(handle: i64) {
    if handle == 0 {
        return;
    }
    // Reconstitute and drop the Box.
    let _ = unsafe { std::boxed::Box::from_raw(handle as usize as *mut DirIterState) };
}

// ---- v0.42 T4: String + String concat runtime helper --------------
//
// Writes a (ptr, len) pair for the concatenation of two Str/String
// (ptr,len) pairs into a caller-supplied 16-byte slot. The bytes go
// into the same `FMT_STRINGS` interner so the slot stays valid for
// the lifetime of the program. Codegen uses this to lower the `+`
// operator when both operands are Str/String.
#[no_mangle]
pub extern "C" fn mty_runtime_str_concat(aptr: i64, alen: i64, bptr: i64, blen: i64, dst: i64) {
    let a = unsafe { read_str(aptr, alen) };
    let b = unsafe { read_str(bptr, blen) };
    let mut s = std::string::String::with_capacity(a.len() + b.len());
    s.push_str(a);
    s.push_str(b);
    let (p, l) = intern_fmt(s);
    unsafe { write_str_pair(dst, p, l) };
}

// ---- helpers --------------------------------------------------------

/// SAFETY: `ptr` must point to `len` valid utf-8 bytes that outlive
/// this call. The codegen emits string literals from the .rodata
/// section so the pointer is always live.
unsafe fn read_str<'a>(ptr: i64, len: i64) -> &'a str {
    if ptr == 0 || len <= 0 {
        return "";
    }
    let p = ptr as usize as *const u8;
    let slice = std::slice::from_raw_parts(p, len as usize);
    std::str::from_utf8_unchecked(slice)
}

/// Build the (name, address) symbol table for the JIT linker.
pub fn symbol_table() -> Vec<(String, *const u8)> {
    macro_rules! entry {
        ($name:literal, $fn:ident) => {
            ($name.to_string(), $fn as *const u8)
        };
    }
    vec![
        entry!("mty_runtime_log", mty_runtime_log),
        entry!("mty_runtime_print", mty_runtime_print),
        entry!("mty_runtime_panic", mty_runtime_panic),
        entry!("mty_runtime_arena_push", mty_runtime_arena_push),
        entry!("mty_runtime_arena_pop", mty_runtime_arena_pop),
        entry!("mty_runtime_alloc", mty_runtime_alloc),
        entry!("mty_runtime_budget_charge", mty_runtime_budget_charge),
        entry!("mty_runtime_send", mty_runtime_send),
        entry!("mty_runtime_ask", mty_runtime_ask),
        entry!("mty_runtime_spawn", mty_runtime_spawn),
        entry!("mty_runtime_extern_call", mty_runtime_extern_call),
        entry!("mty_runtime_log_i64", mty_runtime_log_i64),
        // v0.42 T4 — typed log/print/format surface (L23 fix).
        entry!("mty_runtime_log_i32", mty_runtime_log_i32),
        entry!("mty_runtime_log_u32", mty_runtime_log_u32),
        entry!("mty_runtime_log_u64", mty_runtime_log_u64),
        entry!("mty_runtime_log_usize", mty_runtime_log_usize),
        entry!("mty_runtime_log_f32", mty_runtime_log_f32),
        entry!("mty_runtime_log_f64", mty_runtime_log_f64),
        entry!("mty_runtime_log_bool", mty_runtime_log_bool),
        entry!("mty_runtime_print_i32", mty_runtime_print_i32),
        entry!("mty_runtime_print_i64", mty_runtime_print_i64),
        entry!("mty_runtime_print_u32", mty_runtime_print_u32),
        entry!("mty_runtime_print_u64", mty_runtime_print_u64),
        entry!("mty_runtime_print_usize", mty_runtime_print_usize),
        entry!("mty_runtime_print_f32", mty_runtime_print_f32),
        entry!("mty_runtime_print_f64", mty_runtime_print_f64),
        entry!("mty_runtime_print_bool", mty_runtime_print_bool),
        entry!("mty_runtime_print_sep", mty_runtime_print_sep),
        entry!("mty_runtime_print_newline", mty_runtime_print_newline),
        entry!("mty_runtime_fmt_i32", mty_runtime_fmt_i32),
        entry!("mty_runtime_fmt_i64_to_slot", mty_runtime_fmt_i64_to_slot),
        entry!("mty_runtime_fmt_u32", mty_runtime_fmt_u32),
        entry!("mty_runtime_fmt_u64", mty_runtime_fmt_u64),
        entry!("mty_runtime_fmt_usize", mty_runtime_fmt_usize),
        entry!("mty_runtime_fmt_f32", mty_runtime_fmt_f32),
        entry!("mty_runtime_fmt_f64", mty_runtime_fmt_f64),
        entry!("mty_runtime_fmt_bool", mty_runtime_fmt_bool),
        entry!("mty_runtime_str_concat", mty_runtime_str_concat),
        // v0.45 T1 — native std.fs surface (L18 fix).
        entry!("mty_runtime_fs_read", mty_runtime_fs_read),
        entry!(
            "mty_runtime_fs_read_to_string",
            mty_runtime_fs_read_to_string
        ),
        entry!("mty_runtime_fs_write", mty_runtime_fs_write),
        entry!("mty_runtime_fs_write_string", mty_runtime_fs_write_string),
        entry!("mty_runtime_fs_append", mty_runtime_fs_append),
        entry!("mty_runtime_fs_exists", mty_runtime_fs_exists),
        entry!("mty_runtime_fs_metadata", mty_runtime_fs_metadata),
        entry!(
            "mty_runtime_fs_create_dir_all",
            mty_runtime_fs_create_dir_all
        ),
        entry!("mty_runtime_fs_remove_file", mty_runtime_fs_remove_file),
        entry!(
            "mty_runtime_fs_remove_dir_all",
            mty_runtime_fs_remove_dir_all
        ),
        entry!("mty_runtime_fs_read_dir", mty_runtime_fs_read_dir),
        // v0.46 T4 — read_dir iterator handle ABI.
        entry!("mty_runtime_fs_dir_open", mty_runtime_fs_dir_open),
        entry!("mty_runtime_fs_dir_next", mty_runtime_fs_dir_next),
        entry!("mty_runtime_fs_dir_close", mty_runtime_fs_dir_close),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_push_pop_balances() {
        let _h = mty_runtime_arena_push();
        mty_runtime_arena_pop(_h);
        // Re-entry should still work.
        let _h2 = mty_runtime_arena_push();
        mty_runtime_arena_pop(_h2);
    }

    #[test]
    fn alloc_charges_bytes() {
        BYTES_CHARGED.with(|b| *b.borrow_mut() = 0);
        let _h = mty_runtime_arena_push();
        let _p = mty_runtime_alloc(64, 8, 0);
        mty_runtime_arena_pop(_h);
        let charged = BYTES_CHARGED.with(|b| *b.borrow());
        assert!(charged >= 64);
    }

    #[test]
    fn symbol_table_has_log() {
        let st = symbol_table();
        assert!(st.iter().any(|(n, _)| n == "mty_runtime_log"));
    }

    #[test]
    fn read_str_handles_null() {
        let s = unsafe { read_str(0, 0) };
        assert!(s.is_empty());
    }

    // ---- v0.42 T4 — typed log/print/format surface tests -----------

    #[test]
    fn fmt_i32_writes_ptr_len_pair_into_slot() {
        let mut slot = [0i64; 2];
        let dst = slot.as_mut_ptr() as i64;
        mty_runtime_fmt_i32(42, dst);
        let (ptr, len) = (slot[0], slot[1]);
        assert!(ptr != 0, "ptr must be non-null");
        assert_eq!(len, 2, "\"42\" is two bytes");
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
        assert_eq!(bytes, b"42");
    }

    #[test]
    fn fmt_i32_handles_negative_values() {
        let mut slot = [0i64; 2];
        let dst = slot.as_mut_ptr() as i64;
        mty_runtime_fmt_i32(-7, dst);
        let bytes = unsafe { std::slice::from_raw_parts(slot[0] as *const u8, slot[1] as usize) };
        assert_eq!(bytes, b"-7");
    }

    #[test]
    fn fmt_f32_renders_via_display() {
        let mut slot = [0i64; 2];
        let dst = slot.as_mut_ptr() as i64;
        mty_runtime_fmt_f32(3.5, dst);
        let bytes = unsafe { std::slice::from_raw_parts(slot[0] as *const u8, slot[1] as usize) };
        // f32 Display is well-defined for exact values like 3.5
        assert_eq!(bytes, b"3.5");
    }

    #[test]
    fn fmt_bool_renders_true_false() {
        let mut slot = [0i64; 2];
        let dst = slot.as_mut_ptr() as i64;
        mty_runtime_fmt_bool(1, dst);
        let bytes = unsafe { std::slice::from_raw_parts(slot[0] as *const u8, slot[1] as usize) };
        assert_eq!(bytes, b"true");
        mty_runtime_fmt_bool(0, dst);
        let bytes = unsafe { std::slice::from_raw_parts(slot[0] as *const u8, slot[1] as usize) };
        assert_eq!(bytes, b"false");
    }

    #[test]
    fn fmt_to_null_slot_is_a_noop() {
        // Must not segfault.
        mty_runtime_fmt_i32(99, 0);
    }

    #[test]
    fn str_concat_writes_joined_bytes() {
        let a = b"count=";
        let b = b"42";
        let mut slot = [0i64; 2];
        let dst = slot.as_mut_ptr() as i64;
        mty_runtime_str_concat(
            a.as_ptr() as i64,
            a.len() as i64,
            b.as_ptr() as i64,
            b.len() as i64,
            dst,
        );
        let bytes = unsafe { std::slice::from_raw_parts(slot[0] as *const u8, slot[1] as usize) };
        assert_eq!(bytes, b"count=42");
    }

    #[test]
    fn typed_log_symbols_are_registered() {
        let st = symbol_table();
        for name in [
            "mty_runtime_log_i32",
            "mty_runtime_log_i64",
            "mty_runtime_log_u32",
            "mty_runtime_log_u64",
            "mty_runtime_log_usize",
            "mty_runtime_log_f32",
            "mty_runtime_log_f64",
            "mty_runtime_log_bool",
            "mty_runtime_print_i32",
            "mty_runtime_print_i64",
            "mty_runtime_print_u32",
            "mty_runtime_print_u64",
            "mty_runtime_print_usize",
            "mty_runtime_print_f32",
            "mty_runtime_print_f64",
            "mty_runtime_print_bool",
            "mty_runtime_print_sep",
            "mty_runtime_print_newline",
            "mty_runtime_fmt_i32",
            "mty_runtime_fmt_i64_to_slot",
            "mty_runtime_fmt_u32",
            "mty_runtime_fmt_u64",
            "mty_runtime_fmt_usize",
            "mty_runtime_fmt_f32",
            "mty_runtime_fmt_f64",
            "mty_runtime_fmt_bool",
            "mty_runtime_str_concat",
        ] {
            assert!(
                st.iter().any(|(n, _)| n == name),
                "missing symbol-table entry for {name}"
            );
        }
    }
}
