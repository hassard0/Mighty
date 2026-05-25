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
    eprintln!("stardust panic: {s}");
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
        entry!(
            "mty_runtime_budget_charge",
            mty_runtime_budget_charge
        ),
        entry!("mty_runtime_send", mty_runtime_send),
        entry!("mty_runtime_ask", mty_runtime_ask),
        entry!("mty_runtime_spawn", mty_runtime_spawn),
        entry!("mty_runtime_extern_call", mty_runtime_extern_call),
        entry!("mty_runtime_log_i64", mty_runtime_log_i64),
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
}
