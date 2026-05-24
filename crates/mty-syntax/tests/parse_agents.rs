use insta::assert_snapshot;

fn dump(src: &str) -> String {
    let r = mty_syntax::parse(src);
    let node = mty_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

#[test]
fn a_echo() {
    assert_snapshot!(dump(
        "protocol Echo {\n  Ping(msg: Str) -> Str\n}\n\nagent Echoer: Echo {\n  on Ping(msg) -> msg\n}"
    ));
}

#[test]
fn a_counter() {
    assert_snapshot!(dump(
        "agent Counter: Count {\n  n = 0\n  on Inc() -> { n += 1; n }\n}"
    ));
}

#[test]
fn a_with_caps() {
    assert_snapshot!(dump(
        "agent Fetcher(net, clock): Fetch {\n  on Page(url) -> net.get(url) @2s?\n}"
    ));
}

#[test]
fn a_protocol_composition() {
    assert_snapshot!(dump("protocol Web = Fetch + Cache + Health"));
}

#[test]
fn a_protocol_stream() {
    assert_snapshot!(dump(
        "protocol Stream[T] {\n  Next() -> Option[T]!StreamErr\n  Close() -> Unit\n}"
    ));
}

#[test]
fn a_supervisor_long() {
    assert_snapshot!(dump(
        "supervisor SearchFlow(strategy: one_for_one) {\n  child planner = spawn Planner()\n  child fetcher = spawn Fetcher(net)\n  on_fail(planner) { restart up_to 3 in 30s }\n  on_fail(fetcher) { backoff 100ms..2s; restart }\n}"
    ));
}

#[test]
fn a_supervisor_compact() {
    assert_snapshot!(dump(
        "sup SearchFlow one_for_one {\n  planner = Planner()\n  fetcher = Fetcher(net)\n}"
    ));
}

#[test]
fn a_protocol_versioned() {
    assert_snapshot!(dump(
        "protocol Fetch v1 {\n  Page(url: Url) -> Page!FetchErr\n}"
    ));
}
