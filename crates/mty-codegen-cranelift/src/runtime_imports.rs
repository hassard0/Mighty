//! Symbol table for C-ABI fns the runtime exposes to JIT'd code
//! (spec §24.5, Slice 8). Cranelift declares these as imports;
//! the JIT linker resolves them via the [`RuntimeBridge`] table at
//! `finalize` time.
//!
//! The runtime crate provides the actual implementations in
//! `mty_runtime::codegen_abi`. Keeping the symbol *names* (and
//! their signatures) in this crate keeps a single source of truth
//! for both sides of the boundary.

use cranelift_codegen::ir::{types as ct, AbiParam, Signature};
use cranelift_codegen::isa::CallConv;

/// All runtime imports the codegen knows about.
pub const RUNTIME_IMPORTS: &[RuntimeImport] = &[
    RuntimeImport {
        name: "mty_runtime_log",
        params: &[ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print",
        params: &[ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_panic",
        params: &[ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_arena_push",
        params: &[],
        ret: Some(ct::I64),
    },
    RuntimeImport {
        name: "mty_runtime_arena_pop",
        params: &[ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_alloc",
        params: &[ct::I64, ct::I64, ct::I64],
        ret: Some(ct::I64),
    },
    RuntimeImport {
        name: "mty_runtime_budget_charge",
        params: &[ct::I64],
        ret: Some(ct::I8),
    },
    RuntimeImport {
        name: "mty_runtime_send",
        params: &[ct::I64, ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_ask",
        params: &[ct::I64, ct::I64, ct::I64, ct::I64],
        ret: Some(ct::I64),
    },
    RuntimeImport {
        name: "mty_runtime_spawn",
        params: &[ct::I64],
        ret: Some(ct::I64),
    },
    RuntimeImport {
        name: "mty_runtime_extern_call",
        params: &[ct::I64, ct::I64, ct::I64],
        ret: Some(ct::I64),
    },
    RuntimeImport {
        name: "mty_runtime_log_i64",
        params: &[ct::I64],
        ret: None,
    },
    // v0.42 T4 — typed log/print/format surface (L23 fix).
    RuntimeImport {
        name: "mty_runtime_log_i32",
        params: &[ct::I32],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_log_u32",
        params: &[ct::I32],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_log_u64",
        params: &[ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_log_usize",
        params: &[ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_log_f32",
        params: &[ct::F32],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_log_f64",
        params: &[ct::F64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_log_bool",
        params: &[ct::I8],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print_i32",
        params: &[ct::I32],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print_i64",
        params: &[ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print_u32",
        params: &[ct::I32],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print_u64",
        params: &[ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print_usize",
        params: &[ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print_f32",
        params: &[ct::F32],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print_f64",
        params: &[ct::F64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print_bool",
        params: &[ct::I8],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print_sep",
        params: &[],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_print_newline",
        params: &[],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_fmt_i32",
        params: &[ct::I32, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_fmt_i64_to_slot",
        params: &[ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_fmt_u32",
        params: &[ct::I32, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_fmt_u64",
        params: &[ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_fmt_usize",
        params: &[ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_fmt_f32",
        params: &[ct::F32, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_fmt_f64",
        params: &[ct::F64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_fmt_bool",
        params: &[ct::I8, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_str_concat",
        params: &[ct::I64, ct::I64, ct::I64, ct::I64, ct::I64],
        ret: None,
    },
    // v0.45 T1 — native std.fs surface (L18 fix).
    // read / read_to_string / read_dir: (path_ptr, path_len, dst_slot)
    // write the (ptr, len, ok) triple into a caller-supplied 24-byte
    // stack slot.
    RuntimeImport {
        name: "mty_runtime_fs_read",
        params: &[ct::I64, ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_fs_read_to_string",
        params: &[ct::I64, ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "mty_runtime_fs_read_dir",
        params: &[ct::I64, ct::I64, ct::I64],
        ret: None,
    },
    // write / write_string / append: (path_ptr, path_len, data_ptr,
    // data_len) -> i32 (1=ok, -errno on err).
    RuntimeImport {
        name: "mty_runtime_fs_write",
        params: &[ct::I64, ct::I64, ct::I64, ct::I64],
        ret: Some(ct::I32),
    },
    RuntimeImport {
        name: "mty_runtime_fs_write_string",
        params: &[ct::I64, ct::I64, ct::I64, ct::I64],
        ret: Some(ct::I32),
    },
    RuntimeImport {
        name: "mty_runtime_fs_append",
        params: &[ct::I64, ct::I64, ct::I64, ct::I64],
        ret: Some(ct::I32),
    },
    // exists: (path_ptr, path_len) -> i32 (1/0).
    RuntimeImport {
        name: "mty_runtime_fs_exists",
        params: &[ct::I64, ct::I64],
        ret: Some(ct::I32),
    },
    // metadata: (path_ptr, path_len, dst_slot) -> i32 (1=ok, -errno).
    // The 24-byte slot at `dst` receives {size:u64, mtime_ms:i64,
    // is_file:i8, is_dir:i8}.
    RuntimeImport {
        name: "mty_runtime_fs_metadata",
        params: &[ct::I64, ct::I64, ct::I64],
        ret: Some(ct::I32),
    },
    // create_dir_all / remove_file / remove_dir_all: (path_ptr,
    // path_len) -> i32 (1=ok, -errno).
    RuntimeImport {
        name: "mty_runtime_fs_create_dir_all",
        params: &[ct::I64, ct::I64],
        ret: Some(ct::I32),
    },
    RuntimeImport {
        name: "mty_runtime_fs_remove_file",
        params: &[ct::I64, ct::I64],
        ret: Some(ct::I32),
    },
    RuntimeImport {
        name: "mty_runtime_fs_remove_dir_all",
        params: &[ct::I64, ct::I64],
        ret: Some(ct::I32),
    },
    // v0.46 T4 — read_dir iterator handle ABI.
    // dir_open: (path_ptr, path_len) -> i64 handle (0 = open failed).
    RuntimeImport {
        name: "mty_runtime_fs_dir_open",
        params: &[ct::I64, ct::I64],
        ret: Some(ct::I64),
    },
    // dir_next: (handle, dst_slot) -> i32; writes the next entry's
    // (ptr, len, ok) triple into the 24-byte slot. Returns 1 if a
    // name was written (more entries follow) / 0 on EOF / -errno
    // on I/O failure during iteration.
    RuntimeImport {
        name: "mty_runtime_fs_dir_next",
        params: &[ct::I64, ct::I64],
        ret: Some(ct::I32),
    },
    // dir_close: (handle) -> (); frees the iterator state. Safe to
    // call with 0 so Drop on a never-opened DirIter is a no-op.
    RuntimeImport {
        name: "mty_runtime_fs_dir_close",
        params: &[ct::I64],
        ret: None,
    },
];

#[derive(Debug, Clone, Copy)]
pub struct RuntimeImport {
    pub name: &'static str,
    pub params: &'static [cranelift_codegen::ir::Type],
    pub ret: Option<cranelift_codegen::ir::Type>,
}

impl RuntimeImport {
    pub fn signature(&self, cc: CallConv) -> Signature {
        let mut sig = Signature::new(cc);
        for &p in self.params {
            sig.params.push(AbiParam::new(p));
        }
        if let Some(r) = self.ret {
            sig.returns.push(AbiParam::new(r));
        }
        sig
    }
}

/// Find a runtime import by name. Returns `None` if the codegen
/// references a symbol the runtime doesn't know about — that's a
/// codegen bug, not a user error.
pub fn lookup(name: &str) -> Option<&'static RuntimeImport> {
    RUNTIME_IMPORTS.iter().find(|r| r.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_finds_known() {
        assert!(lookup("mty_runtime_log").is_some());
        assert!(lookup("mty_runtime_panic").is_some());
    }

    #[test]
    fn lookup_misses_unknown() {
        assert!(lookup("not_a_real_runtime_symbol").is_none());
    }

    #[test]
    fn every_import_has_distinct_name() {
        let mut names: Vec<_> = RUNTIME_IMPORTS.iter().map(|r| r.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), RUNTIME_IMPORTS.len());
    }

    #[test]
    fn signature_param_count_matches() {
        let cc = CallConv::SystemV;
        for ri in RUNTIME_IMPORTS {
            let sig = ri.signature(cc);
            assert_eq!(sig.params.len(), ri.params.len());
            assert_eq!(sig.returns.len(), ri.ret.iter().count());
        }
    }
}
