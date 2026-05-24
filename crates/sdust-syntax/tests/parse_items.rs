use insta::assert_snapshot;

fn dump(src: &str) -> String {
    let r = sdust_syntax::parse(src);
    let node = sdust_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

#[test] fn use_simple()    { assert_snapshot!(dump("use std.io")); }
#[test] fn use_brace()     { assert_snapshot!(dump("use std.net.{Http, Url}")); }
#[test] fn use_alias()     { assert_snapshot!(dump("use app.model as model")); }
#[test] fn mod_decl()      { assert_snapshot!(dump("mod net.http")); }
#[test] fn package_decl()  { assert_snapshot!(dump("package search_api")); }
#[test] fn pub_use()       { assert_snapshot!(dump("pub use std.io")); }
