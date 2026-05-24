use insta::assert_snapshot;
use sdust_ast::{AstNode, File};
use sdust_syntax::{parse, SyntaxNode};

fn dump(src: &str) -> String {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).unwrap();
    let (pkg, _) = sdust_hir::lower::LoweringCtx::new().lower_file(f);
    sdust_hir::dump::dump_package(&pkg)
}

#[test]
fn d_fn() {
    assert_snapshot!(dump("fn add(a: I32, b: I32) -> I32 = a + b"));
}

#[test]
fn d_agent() {
    assert_snapshot!(dump(
        "protocol Echo { Ping(msg: Str) -> Str }\nagent Echoer: Echo { on Ping(msg) -> msg }"
    ));
}

#[test]
fn d_arena() {
    assert_snapshot!(dump("fn main() { arena turn { let x = 1; x } }"));
}
