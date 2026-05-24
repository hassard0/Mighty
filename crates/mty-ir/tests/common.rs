//! Shared helpers for SIR-level tests.
//!
//! End-to-end driver tests live in `mty-driver`; here we want to be
//! able to lower + run small Mighty snippets without dragging in
//! `mty-runtime` (which has its own build state at this point in the
//! v0.5 swarm). We rebuild the pieces we need from the published
//! crates.

#![allow(dead_code)]

use mty_ast::{AstNode, File};
use mty_hir::Package;
use mty_ir::interp::{run_fn_by_name, Host, RunResult, Value};
use mty_ir::ir::{EffectOp, Program};
use mty_syntax::{parse, SyntaxNode};
use mty_types::{EffectId, TypedPackage};

#[derive(Default)]
pub struct TestHost {
    pub stdout: String,
}

impl Host for TestHost {
    fn print(&mut self, s: &str) {
        self.stdout.push_str(s);
    }
    fn effect_call(&mut self, _e: EffectId, _op: &EffectOp, _args: &[Value]) -> Value {
        Value::Unit
    }
    fn extern_call(&mut self, _name: &str, _args: &[Value]) -> Value {
        Value::Unit
    }
}

pub fn compile(src: &str) -> (Package, TypedPackage, Program) {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).expect("cast file");
    let (pkg, _diags) = mty_hir::lower::LoweringCtx::new().lower_file(f);
    let typed = mty_types::check_package_typed(&pkg);
    let prog = mty_ir::lower_package(&pkg, &typed);
    (pkg, typed, prog)
}

pub fn run_main(src: &str) -> (RunResult, TestHost) {
    let (_pkg, _typed, prog) = compile(src);
    let mut host = TestHost::default();
    let main_f = prog.fn_by_name("main").expect("main fn");
    let res = match run_fn_by_name(&prog, &main_f.name, vec![], &mut host) {
        Ok(v) => match v {
            // Mirror `main_exit_for_value` so tests can assert on the
            // numeric tail of `fn main() -> I32 { ... n }`.
            Value::Int(n, _) => RunResult::Ok {
                exit: (n as i32).max(0),
            },
            Value::Enum { variant: 1, .. } => RunResult::Ok { exit: 1 },
            _ => RunResult::Ok { exit: 0 },
        },
        Err(r) => r,
    };
    (res, host)
}
