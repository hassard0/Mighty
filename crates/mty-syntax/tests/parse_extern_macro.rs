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

// ---- v0.37 T6 — variadic extern C fn declarations ----

/// Parses a `printf`-style signature with a trailing `...` after one
/// fixed param. The variadic marker should land in the CST as a
/// `VARIADIC_MARKER` node, NOT as an extra `FN_PARAM`.
#[test]
fn x_extern_c_variadic_printf() {
    assert_snapshot!(dump("extern c {\n  fn printf(fmt: *U8, ...) -> I32\n}"));
}

/// Two fixed params + variadic — the most common shape for snprintf
/// and friends.
#[test]
fn x_extern_c_variadic_snprintf() {
    assert_snapshot!(dump(
        "extern c {\n  fn snprintf(buf: *U8, n: USize, fmt: *U8, ...) -> I32\n}"
    ));
}

/// Leading `...` (zero fixed params before the marker). C technically
/// requires at least one fixed param before `...`, but the parser
/// accepts this shape — typeck / clippy-style lints can flag it later.
/// What we care about at parse time is the CST shape: no `FN_PARAM`,
/// one `VARIADIC_MARKER`.
#[test]
fn x_extern_c_variadic_leading() {
    assert_snapshot!(dump("extern c {\n  fn fff(...) -> I32\n}"));
}

/// Mixed extern block: one variadic + one regular fn. Each fn's
/// variadic-ness is recorded independently.
#[test]
fn x_extern_c_variadic_mixed_block() {
    assert_snapshot!(dump(
        "extern c {\n  fn printf(fmt: *U8, ...) -> I32\n  fn strlen(s: *U8) -> USize\n}"
    ));
}
