use insta::assert_snapshot;

fn dump_expr(src: &str) -> String {
    let r = sdust_syntax::parser::parse_expr(src);
    let node = sdust_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

#[test] fn c_arena_short() {
    assert_snapshot!(dump_expr("arena turn: lower(parse(tokenize(input))?)"));
}
#[test] fn c_arena_block() {
    assert_snapshot!(dump_expr("arena turn { let toks = tokenize(input); parse(toks)? }"));
}
#[test] fn c_task_scope() {
    assert_snapshot!(dump_expr("task scope { let a = spawn task fetch(a_url); join a? }"));
}
#[test] fn c_task_scope_deadline() {
    assert_snapshot!(dump_expr("task scope @5s { work()? }"));
}
#[test] fn c_task_call() {
    assert_snapshot!(dump_expr("task.all(fetch(a_url), fetch(b_url))"));
}
#[test] fn c_budget() {
    assert_snapshot!(dump_expr("budget { cpu 150ms wall 2s mem 128MiB mb 1k } run job(input)?"));
}
#[test] fn c_sandbox() {
    assert_snapshot!(dump_expr("sandbox ToolRun with { fs.read = [\"/models\"], net = [\"api.example.com:443\"], cpu = 150ms, wall = 2s, memory = 128MiB, mailbox = 1024 } { run job(input) }"));
}
