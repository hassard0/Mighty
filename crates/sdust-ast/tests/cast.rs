use sdust_ast::{AgentDecl, AstNode, File, FnDecl};
use sdust_syntax::{parse, SyntaxNode};

fn root(src: &str) -> File {
    let r = parse(src);
    File::cast(SyntaxNode::new_root(r.green)).expect("FILE root")
}

#[test]
fn casts_fn() {
    let f = root("fn add(a: I32, b: I32) -> I32 = a + b");
    let fns: Vec<FnDecl> = f.0.descendants().filter_map(FnDecl::cast).collect();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name().unwrap().text(), "add");
}

#[test]
fn casts_agent_with_handlers() {
    let f = root("agent Counter: Count { n = 0\n on Inc() -> { n += 1; n }\n }");
    let agents: Vec<AgentDecl> = f.0.descendants().filter_map(AgentDecl::cast).collect();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].handlers().count(), 1);
    assert_eq!(agents[0].state_fields().count(), 1);
}
