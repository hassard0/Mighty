use insta::assert_snapshot;

fn dump(src: &str) -> String {
    let r = sdust_syntax::parser::parse_type(src);
    let node = sdust_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

#[test] fn t_borrow()      { assert_snapshot!(dump("&Str")); }
#[test] fn t_borrow_mut()  { assert_snapshot!(dump("&mut Bytes")); }
#[test] fn t_tuple()       { assert_snapshot!(dump("(I32, Str)")); }
#[test] fn t_array()       { assert_snapshot!(dump("[U8; 16]")); }
#[test] fn t_path_generic(){ assert_snapshot!(dump("Map[Str, Json]")); }
#[test] fn t_result_sugar(){ assert_snapshot!(dump("Bytes!IoErr")); }
#[test] fn t_result_union(){ assert_snapshot!(dump("Page!{NetErr, ParseErr}")); }
