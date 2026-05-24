use insta::assert_snapshot;

fn dump(src: &str) -> String {
    let r = mty_syntax::parse(src);
    let node = mty_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

#[test]
fn d_fn_simple() {
    assert_snapshot!(dump("fn add(a: I32, b: I32) -> I32 = a + b"));
}

#[test]
fn d_fn_pub() {
    assert_snapshot!(dump("pub fn add(a: I32, b: I32) -> I32 { a + b }"));
}

#[test]
fn d_fn_effect() {
    assert_snapshot!(dump(
        "pub fn fetch(url: Url) -> Page!FetchErr effect net,time"
    ));
}

#[test]
fn d_fn_generic() {
    assert_snapshot!(dump("fn first[T](xs: &[T]) -> Option[&T] { None }"));
}

#[test]
fn d_struct() {
    assert_snapshot!(dump("struct User { id: U64, name: String }"));
}

#[test]
fn d_enum() {
    assert_snapshot!(dump("enum Result[T, E] { Ok(T), Err(E) }"));
}

#[test]
fn d_type_alias() {
    assert_snapshot!(dump("type UserId = U64"));
}

#[test]
fn d_impl() {
    assert_snapshot!(dump(
        "impl Hash for UserId { fn hash(self) -> U64 = self.value }"
    ));
}

#[test]
fn d_trait() {
    assert_snapshot!(dump("trait Hash { fn hash(self) -> U64 }"));
}

#[test]
fn d_const() {
    assert_snapshot!(dump("const PAGE: USize = 4096"));
}

#[test]
fn d_multi() {
    assert_snapshot!(dump(
        "pub fn a() = 1\n\nstruct B { x: I32 }\n\nconst C: I32 = 0\n"
    ));
}
