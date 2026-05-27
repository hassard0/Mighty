//! v0.25 Track C: array types in agent field declarations.
//!
//! Track E's v0.24 game-canvas work hit a wall on `agent Notetris { board:
//! [U32; 200] }` — the fixed-size array type shape parsed fine in
//! `let`-decls and fn-signatures but was rejected as an agent field. These
//! tests pin the surface shape so the Notetris 10×20 board can live on the
//! agent itself.

fn parse_dump(src: &str) -> (mty_syntax::SyntaxNode, Vec<mty_syntax::ParseError>) {
    let r = mty_syntax::parse(src);
    (mty_syntax::SyntaxNode::new_root(r.green), r.errors)
}

#[test]
fn agent_field_with_fixed_array_type_parses() {
    let (node, errs) = parse_dump("agent X { board: [U32; 200] }");
    let dump = format!("{:#?}", node);
    assert!(
        errs.is_empty(),
        "expected no parse errors, got {:?}\n{}",
        errs,
        dump
    );
    assert!(
        dump.contains("AGENT_STATE_DECL"),
        "expected AGENT_STATE_DECL in dump:\n{}",
        dump
    );
    assert!(
        dump.contains("TYPE_ARRAY"),
        "expected TYPE_ARRAY (the `[T; N]` shape) in dump:\n{}",
        dump
    );
}

#[test]
fn agent_field_array_with_complex_element_type() {
    let (node, errs) = parse_dump("agent Bag { slots: [Piece; 7] }");
    let dump = format!("{:#?}", node);
    assert!(
        errs.is_empty(),
        "expected no parse errors, got {:?}\n{}",
        errs,
        dump
    );
    assert!(
        dump.contains("TYPE_ARRAY"),
        "expected TYPE_ARRAY in dump:\n{}",
        dump
    );
}

#[test]
fn agent_with_multiple_fields_including_array() {
    let src = "agent Game {
        score: U32
        board: [I32; 16]
        running: Bool
    }";
    let (_node, errs) = parse_dump(src);
    assert!(errs.is_empty(), "expected no parse errors, got {:?}", errs);
}

#[test]
fn agent_array_field_then_handler() {
    // Real shape Track E wanted: agent stores a board then handles a key.
    let src = "agent Notetris {
        board: [U32; 200]
        score: U32
        on KeyDown(k) -> { score += 1 }
    }";
    let (node, errs) = parse_dump(src);
    let dump = format!("{:#?}", node);
    assert!(
        errs.is_empty(),
        "expected no parse errors, got {:?}\n{}",
        errs,
        dump
    );
    assert!(
        dump.contains("ON_HANDLER"),
        "expected ON_HANDLER:\n{}",
        dump
    );
    assert!(
        dump.contains("TYPE_ARRAY"),
        "expected TYPE_ARRAY:\n{}",
        dump
    );
}

#[test]
fn agent_field_with_named_size_const_parses() {
    // const-as-size form. The lexer treats `N` as an IDENT inside the
    // array-size slot, and `exprs::expr` accepts identifier references.
    // Typeck enforcement of "const must resolve" is a separate layer —
    // here we only pin that the parser doesn't choke.
    let src = "const N: U32 = 200\nagent X { board: [U32; N] }";
    let (_node, errs) = parse_dump(src);
    assert!(errs.is_empty(), "expected no parse errors, got {:?}", errs);
}
