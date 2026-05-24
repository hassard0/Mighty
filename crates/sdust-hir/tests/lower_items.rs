use sdust_syntax::{parse, SyntaxNode};
use sdust_ast::{File, AstNode};

fn lower(src: &str) -> sdust_hir::Package {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).unwrap();
    let (pkg, _) = sdust_hir::lower::LoweringCtx::new().lower_file(f);
    pkg
}

#[test] fn lowers_fn() {
    let p = lower("fn add(a: I32, b: I32) -> I32 = a + b");
    assert_eq!(p.fns.len(), 1);
    let f = p.fns.values().next().unwrap();
    assert_eq!(f.name, "add");
    assert_eq!(f.params.len(), 2);
}

#[test] fn lowers_struct() {
    let p = lower("struct User { id: U64, name: String }");
    assert_eq!(p.structs.len(), 1);
    let s = p.structs.values().next().unwrap();
    assert_eq!(s.name, "User");
    assert_eq!(s.fields.len(), 2);
    assert_eq!(s.fields[0].name, "id");
}

#[test] fn lowers_enum() {
    let p = lower("enum Result[T, E] { Ok(T), Err(E) }");
    assert_eq!(p.enums.len(), 1);
    let e = p.enums.values().next().unwrap();
    assert_eq!(e.variants.len(), 2);
}

#[test] fn lowers_type_alias() {
    let p = lower("type UserId = U64");
    assert_eq!(p.type_aliases.len(), 1);
}

#[test] fn lowers_use() {
    let p = lower("use std.io");
    assert_eq!(p.top_level.len(), 1);
}

#[test] fn lowers_agent() {
    let p = lower("agent Counter: Count { n = 0\n on Inc() -> { n += 1; n } }");
    assert_eq!(p.agents.len(), 1);
    let a = p.agents.values().next().unwrap();
    assert_eq!(a.name, "Counter");
    assert_eq!(a.state.len(), 1);
    assert_eq!(a.handlers.len(), 1);
}

#[test] fn lowers_protocol() {
    let p = lower("protocol Echo { Ping(msg: Str) -> Str }");
    assert_eq!(p.protocols.len(), 1);
    let pr = p.protocols.values().next().unwrap();
    assert_eq!(pr.name, "Echo");
    assert_eq!(pr.messages.len(), 1);
}

#[test] fn lowers_arena_short() {
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
