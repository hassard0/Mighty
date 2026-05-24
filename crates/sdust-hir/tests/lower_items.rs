use sdust_ast::{AstNode, File};
use sdust_syntax::{parse, SyntaxNode};

fn lower(src: &str) -> sdust_hir::Package {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).unwrap();
    let (pkg, _) = sdust_hir::lower::LoweringCtx::new().lower_file(f);
    pkg
}

#[test]
fn lowers_fn() {
    let p = lower("fn add(a: I32, b: I32) -> I32 = a + b");
    assert_eq!(p.fns.len(), 1);
    let f = p.fns.values().next().unwrap();
    assert_eq!(f.name, "add");
    assert_eq!(f.params.len(), 2);
}

#[test]
fn lowers_struct() {
    let p = lower("struct User { id: U64, name: String }");
    assert_eq!(p.structs.len(), 1);
    let s = p.structs.values().next().unwrap();
    assert_eq!(s.name, "User");
    assert_eq!(s.fields.len(), 2);
    assert_eq!(s.fields[0].name, "id");
}

#[test]
fn lowers_enum() {
    let p = lower("enum Result[T, E] { Ok(T), Err(E) }");
    assert_eq!(p.enums.len(), 1);
    let e = p.enums.values().next().unwrap();
    assert_eq!(e.variants.len(), 2);
}

#[test]
fn lowers_type_alias() {
    let p = lower("type UserId = U64");
    assert_eq!(p.type_aliases.len(), 1);
}

#[test]
fn lowers_use() {
    let p = lower("use std.io");
    assert_eq!(p.top_level.len(), 1);
}

#[test]
fn lowers_agent() {
    let p = lower("agent Counter: Count { n = 0\n on Inc() -> { n += 1; n } }");
    assert_eq!(p.agents.len(), 1);
    let a = p.agents.values().next().unwrap();
    assert_eq!(a.name, "Counter");
    assert_eq!(a.state.len(), 1);
    assert_eq!(a.handlers.len(), 1);
}

#[test]
fn lowers_protocol() {
    let p = lower("protocol Echo { Ping(msg: Str) -> Str }");
    assert_eq!(p.protocols.len(), 1);
    let pr = p.protocols.values().next().unwrap();
    assert_eq!(pr.name, "Echo");
    assert_eq!(pr.messages.len(), 1);
}

// ---- slice-2 lowering ----

#[test]
fn lower_lambda_expr() {
    let p = lower("fn f() { let g = fn(x: I32) -> I32 { x + 1 } }");
    let has_lambda = p
        .exprs
        .iter()
        .any(|(_, e)| matches!(e, sdust_hir::HirExpr::Lambda { .. }));
    assert!(has_lambda, "expected HirExpr::Lambda");
}

#[test]
fn lower_if_let_expr() {
    let p = lower("fn f() { if let Some(x) = opt { x } else { 0 } }");
    let has_iflet = p
        .exprs
        .iter()
        .any(|(_, e)| matches!(e, sdust_hir::HirExpr::IfLet { .. }));
    assert!(has_iflet, "expected HirExpr::IfLet");
}

#[test]
fn lower_plain_if_still_works() {
    let p = lower("fn f() { if a { 1 } else { 0 } }");
    let has_if = p
        .exprs
        .iter()
        .any(|(_, e)| matches!(e, sdust_hir::HirExpr::If { .. }));
    let has_iflet = p
        .exprs
        .iter()
        .any(|(_, e)| matches!(e, sdust_hir::HirExpr::IfLet { .. }));
    assert!(has_if, "expected HirExpr::If");
    assert!(!has_iflet, "plain if should NOT lower to IfLet");
}

#[test]
fn lower_run_expr() {
    let p = lower("fn f() { run g() }");
    let has_run = p
        .exprs
        .iter()
        .any(|(_, e)| matches!(e, sdust_hir::HirExpr::Run(_)));
    assert!(has_run, "expected HirExpr::Run");
}

#[test]
fn lower_turbofish_path() {
    let p = lower("fn f() { let _ = Some::[I32](1) }");
    let has_pg = p
        .exprs
        .iter()
        .any(|(_, e)| matches!(e, sdust_hir::HirExpr::PathGeneric { .. }));
    assert!(has_pg, "expected HirExpr::PathGeneric");
}

#[test]
fn lower_plain_path_still_works() {
    let p = lower("fn f() { let _ = foo.bar }");
    let has_p = p
        .exprs
        .iter()
        .any(|(_, e)| matches!(e, sdust_hir::HirExpr::Path(_)));
    assert!(has_p, "expected HirExpr::Path");
}

#[test]
fn lowers_arena_short() {
    let p = lower("fn main() { arena turn: tokenize(input) }");
    assert_eq!(p.fns.len(), 1);
    let f = p.fns.values().next().unwrap();
    let body = f.body.expect("fn body");
    let block = &p.blocks[body];
    // The block should have a tail expression that's an Arena, or a single Arena stmt.
    if let Some(tail_id) = block.tail {
        let tail = &p.exprs[tail_id];
        assert!(
            matches!(tail, sdust_hir::HirExpr::Arena { .. }),
            "expected Arena expr, got {:?}",
            tail
        );
    } else if !block.stmts.is_empty() {
        match &block.stmts[0] {
            sdust_hir::HirStmt::Expr(eid) => {
                assert!(matches!(&p.exprs[*eid], sdust_hir::HirExpr::Arena { .. }));
            }
            _ => panic!("expected expr stmt"),
        }
    } else {
        panic!("block had neither tail nor stmts");
    }
}
