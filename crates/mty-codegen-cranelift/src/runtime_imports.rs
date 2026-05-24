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
        name: "stardust_runtime_log",
        params: &[ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "stardust_runtime_print",
        params: &[ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "stardust_runtime_panic",
        params: &[ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "stardust_runtime_arena_push",
        params: &[],
        ret: Some(ct::I64),
    },
    RuntimeImport {
        name: "stardust_runtime_arena_pop",
        params: &[ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "stardust_runtime_alloc",
        params: &[ct::I64, ct::I64, ct::I64],
        ret: Some(ct::I64),
    },
    RuntimeImport {
        name: "stardust_runtime_budget_charge",
        params: &[ct::I64],
        ret: Some(ct::I8),
    },
    RuntimeImport {
        name: "stardust_runtime_send",
        params: &[ct::I64, ct::I64, ct::I64],
        ret: None,
    },
    RuntimeImport {
        name: "stardust_runtime_ask",
        params: &[ct::I64, ct::I64, ct::I64, ct::I64],
        ret: Some(ct::I64),
    },
    RuntimeImport {
        name: "stardust_runtime_spawn",
        params: &[ct::I64],
        ret: Some(ct::I64),
    },
    RuntimeImport {
        name: "stardust_runtime_extern_call",
        params: &[ct::I64, ct::I64, ct::I64],
        ret: Some(ct::I64),
    },
    RuntimeImport {
        name: "stardust_runtime_log_i64",
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
        assert!(lookup("stardust_runtime_log").is_some());
        assert!(lookup("stardust_runtime_panic").is_some());
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
