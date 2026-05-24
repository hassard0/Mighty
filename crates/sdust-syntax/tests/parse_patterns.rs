use insta::assert_snapshot;

fn dump(src: &str) -> String {
    let r = sdust_syntax::parser::parse_pattern(src);
    let node = sdust_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

#[test] fn p_wildcard()    { assert_snapshot!(dump("_")); }
#[test] fn p_binding()     { assert_snapshot!(dump("x")); }
#[test] fn p_at_binding()  { assert_snapshot!(dump("name @ User { id, name }")); }
#[test] fn p_literal()     { assert_snapshot!(dump("42")); }
#[test] fn p_ref_mut()     { assert_snapshot!(dump("&mut buf")); }
#[test] fn p_tuple()       { assert_snapshot!(dump("(a, b, _)")); }
#[test] fn p_struct()      { assert_snapshot!(dump("User { id, name }")); }
#[test] fn p_enum_some()   { assert_snapshot!(dump("Some(x)")); }
#[test] fn p_enum_ok()     { assert_snapshot!(dump("Ok(v)")); }
#[test] fn p_range()       { assert_snapshot!(dump("1..5")); }
#[test] fn p_range_eq()    { assert_snapshot!(dump("1..=5")); }
