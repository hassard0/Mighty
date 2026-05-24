use insta::assert_snapshot;

fn dump(src: &str) -> String {
    let r = mty_syntax::parse(src);
    let node = mty_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

fn dump_expr(src: &str) -> String {
    let r = mty_syntax::parser::parse_expr(src);
    let node = mty_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

#[test]
fn x_extern_c() {
    assert_snapshot!(dump("extern c {\n  fn strlen(s: *U8) -> USize\n}"));
}

#[test]
fn x_extern_js() {
    assert_snapshot!(dump("extern js {\n  fn alert(msg: Str) effect dom\n}"));
}

#[test]
fn x_export_c_fn() {
    assert_snapshot!(dump("export c fn add(a: I32, b: I32) -> I32 = a + b"));
}

#[test]
fn x_export_fn() {
    assert_snapshot!(dump(
        "export fn mount(dom: Dom) { let c = spawn Counter(dom) }"
    ));
}

#[test]
fn x_macro() {
    assert_snapshot!(dump(
        "macro assert_eq(a, b) => {\n  if a != b { panic(\"assert_eq failed\") }\n}"
    ));
}

#[test]
fn x_unsafe_block() {
    assert_snapshot!(dump_expr(
        "unsafe {\n  let p = raw_ptr(addr)\n  p.read()\n}"
    ));
}

#[test]
fn x_unsafe_fn_requires() {
    assert_snapshot!(dump("pub unsafe fn from_raw(ptr: *U8, len: USize) -> Bytes\n  requires ptr != null\n  requires valid(ptr, len)"));
}
