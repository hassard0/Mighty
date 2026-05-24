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
