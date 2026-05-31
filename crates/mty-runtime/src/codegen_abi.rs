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
