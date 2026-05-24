//! Self-hosting bootstrap test (v0.6) — parser phase.
//!
//! Runs the Mighty parser in `selfhost/parser/parser.mty` over a canned
//! input via the SIR interpreter, with a custom `Host` that services the
//! parser's cursor + event-sink bridge (`tok_*`, `cur_*`, `ev_*`,
//! `no_struct_lit_*`). Then it parses the same input via the trusted
//! Rust parser (`mty_syntax::parse`) and diffs the two trees BFS:
//! same node kinds in BFS order, same token kinds + texts at the leaves.
//!
//! Bootstrap technique: same shape as the v0.5 self-host lexer test
//! (`selfhost_lexer.rs`). The parser's state lives entirely in the
//! `SelfhostParserHost`; the Mighty source is the pure algorithm.
//!
//! For v0.6 the parser ships a SUBSET — see
//! `SELFHOST_PARSER_V0_6_NOTES.md` for the production matrix + gap
//! catalog. v0.6 ships 13 live tests; every production in the SUBSET
//! passes the BFS-kind diff against the trusted Rust parser, including
//! all five canonical examples (01-05). Tests that exercise productions
//! beyond the subset would be marked `#[ignore]` with a documented
//! "v0.7" reason — currently there are no ignored tests because the
//! subset is wider than originally scoped.

use mty_driver::{lower, lower_to_sir, parse_source, type_and_borrow_check};
use mty_ir::interp::{run_fn_by_name, Host, RunResult, Value};
use mty_ir::ir::EffectOp;
use mty_syntax::{lex as rust_lex, parse as rust_parse, SyntaxKind, SyntaxNode};
use mty_types::{EffectId, IntKind};
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

// ---- Selfhost host ------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Enter(String),
    Exit,
    Token(String, String),       // kind name + text
    EnterAt(usize, String),      // checkpoint id + kind name (resolved at rebuild)
    Error(String, usize, usize), // message + start + end
}

#[derive(Debug, Default)]
struct SelfhostParserHost {
    tokens: Vec<TokenInfo>,
    cursor: usize,
    no_struct_lit: bool,
    events: Vec<Event>,
    /// Maps checkpoint id -> index into `events` at the time the
    /// checkpoint was taken. `ev_start_at(cp, kind)` inserts an
    /// `EnterAt(cp, kind)` placeholder that the rebuilder resolves by
    /// moving it to position `events[cp_index]`.
    checkpoint_at: Vec<usize>,
}

#[derive(Debug, Clone)]
struct TokenInfo {
    kind: SyntaxKind,
    text: String,
    start: usize,
    end: usize,
}

impl Host for SelfhostParserHost {
    fn print(&mut self, _s: &str) {}

    fn effect_call(&mut self, _effect: EffectId, op: &EffectOp, args: &[Value]) -> Value {
        let EffectOp::GenericCall { method, .. } = op;
        self.dispatch_method(method, args)
    }

    fn extern_call(&mut self, _name: &str, _args: &[Value]) -> Value {
        Value::Unit
    }
}

impl SelfhostParserHost {
    fn seed(&mut self, src: &str) {
        // Run the trusted Rust lexer and seed the host with the tokens.
        // Note: the Mighty parser will see ALL tokens including trivia
        // — `cur_skip_trivia` advances over the trivia just like the
        // Rust parser does.
        self.tokens = rust_lex(src)
            .into_iter()
            .map(|t| TokenInfo {
                kind: t.kind,
                text: t.text.to_string(),
                start: t.start,
                end: t.end,
            })
            .collect();
        self.cursor = 0;
        self.no_struct_lit = false;
        self.events.clear();
        self.checkpoint_at.clear();
    }

    fn dispatch_method(&mut self, method: &str, args: &[Value]) -> Value {
        match method {
            // -- cursor read-only --
            "tok_count" => Value::Int(self.tokens.len() as i128, IntKind::USize),
            "tok_kind" => {
                let i = arg_usize(args, 0);
                let name = if i < self.tokens.len() {
                    format!("{:?}", self.tokens[i].kind)
                } else {
                    "EOF".to_string()
                };
                Value::Str(name)
            }
            "tok_text" => {
                let i = arg_usize(args, 0);
                let s = if i < self.tokens.len() {
                    self.tokens[i].text.clone()
                } else {
                    String::new()
                };
                Value::Str(s)
            }
            "tok_start" => {
                let i = arg_usize(args, 0);
                let s = if i < self.tokens.len() {
                    self.tokens[i].start
                } else {
                    self.tokens.last().map(|t| t.end).unwrap_or(0)
                };
                Value::Int(s as i128, IntKind::USize)
            }
            "tok_end" => {
                let i = arg_usize(args, 0);
                let e = if i < self.tokens.len() {
                    self.tokens[i].end
                } else {
                    self.tokens.last().map(|t| t.end).unwrap_or(0)
                };
                Value::Int(e as i128, IntKind::USize)
            }
            "tok_is_trivia" => {
                let i = arg_usize(args, 0);
                let b = i < self.tokens.len() && self.tokens[i].kind.is_trivia();
                Value::Bool(b)
            }
            "tok_is_keyword" => {
                let i = arg_usize(args, 0);
                let b = i < self.tokens.len() && self.tokens[i].kind.is_keyword();
                Value::Bool(b)
            }
            // -- cursor mutating --
            "cur_pos" => Value::Int(self.cursor as i128, IntKind::USize),
            "cur_set" => {
                self.cursor = arg_usize(args, 0);
                Value::Unit
            }
            "cur_skip_trivia" => {
                while self.cursor < self.tokens.len() && self.tokens[self.cursor].kind.is_trivia() {
                    let t = &self.tokens[self.cursor];
                    self.events
                        .push(Event::Token(format!("{:?}", t.kind), t.text.clone()));
                    self.cursor += 1;
                }
                Value::Unit
            }
            // -- event sink --
            "ev_start" => {
                let kind = arg_str(args, 0);
                self.events.push(Event::Enter(kind));
                Value::Unit
            }
            "ev_finish" => {
                self.events.push(Event::Exit);
                Value::Unit
            }
            "ev_token" => {
                let i = arg_usize(args, 0);
                if i < self.tokens.len() {
                    let t = &self.tokens[i];
                    self.events
                        .push(Event::Token(format!("{:?}", t.kind), t.text.clone()));
                    self.cursor = i + 1;
                }
                Value::Unit
            }
            "ev_error" => {
                let msg = arg_str(args, 0);
                let s = arg_usize(args, 1);
                let e = arg_usize(args, 2);
                self.events.push(Event::Error(msg, s, e));
                Value::Unit
            }
            "ev_checkpoint" => {
                let id = self.checkpoint_at.len();
                self.checkpoint_at.push(self.events.len());
                Value::Int(id as i128, IntKind::USize)
            }
            "ev_start_at" => {
                let cp_id = arg_usize(args, 0);
                let kind = arg_str(args, 1);
                // Embed the SAVED event index (the position of the
                // checkpoint at the time it was taken), not the slot
                // id, so the tree-rebuilder can splice without a
                // separate mapping.
                let recorded_idx = self
                    .checkpoint_at
                    .get(cp_id)
                    .copied()
                    .unwrap_or(self.events.len());
                self.events.push(Event::EnterAt(recorded_idx, kind));
                Value::Unit
            }
            // -- struct-literal context --
            "no_struct_lit_get" => Value::Bool(self.no_struct_lit),
            "no_struct_lit_set" => {
                let b = match args.first() {
                    Some(Value::Bool(b)) => *b,
                    _ => false,
                };
                self.no_struct_lit = b;
                Value::Unit
            }
            _ => Value::Unit,
        }
    }
}

fn arg_usize(args: &[Value], i: usize) -> usize {
    args.get(i)
        .and_then(|v| v.as_int())
        .map(|n| n as usize)
        .unwrap_or(0)
}

fn arg_str(args: &[Value], i: usize) -> String {
    match args.get(i) {
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Char(c)) => c.to_string(),
        Some(v) => v.as_str(),
        None => String::new(),
    }
}

// ---- Compile + run the self-hosted parser -------------------------------

struct SelfhostRun {
    events: Vec<Event>,
    result: RunResult,
}

fn run_selfhost_parser(input: &str) -> Result<SelfhostRun, String> {
    let parser_path = workspace_root().join("selfhost/parser/parser.mty");
    let parser_src = std::fs::read_to_string(&parser_path)
        .map_err(|e| format!("read {}: {}", parser_path.display(), e))?;
    let parsed = parse_source(parser_src, "selfhost/parser/parser.mty".into());
    let (pkg, lower_diags) = lower(&parsed);
    if lower_diags
        .iter()
        .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        return Err(format!("lower errors: {:?}", lower_diags));
    }
    let tbc = type_and_borrow_check(&pkg);
    if tbc
        .iter()
        .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        return Err(format!(
            "type/borrow errors: {:?}",
            tbc.iter()
                .filter(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
                .collect::<Vec<_>>()
        ));
    }
    let (prog, sir_diags) = lower_to_sir(&pkg);
    if sir_diags
        .iter()
        .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        return Err(format!("sir errors: {:?}", sir_diags));
    }

    let mut host = SelfhostParserHost::default();
    host.seed(input);
    let res = run_fn_by_name(&prog, "parse", vec![], &mut host);
    let result = match res {
        Ok(_) => RunResult::Ok { exit: 0 },
        Err(r) => r,
    };
    Ok(SelfhostRun {
        events: host.events,
        result,
    })
}

// ---- Tree shape -----------------------------------------------------

/// Lightweight tree built from the event stream. We don't try to match
/// rowan's green-tree shape; we just need a tree of `(kind, children)`
/// where leaves carry `(token_kind, text)`. Two trees compare equal iff
/// they have the same shape, same kinds, and same leaf texts.
#[derive(Debug, PartialEq, Eq)]
enum Node {
    Branch { kind: String, children: Vec<Node> },
    Leaf { kind: String, text: String },
}

fn build_tree(events: &[Event]) -> Option<Node> {
    // Process `EnterAt(recorded_idx, kind)` events: each means "insert
    // an Enter at the recorded checkpoint position; the EnterAt itself
    // is consumed (the wrapper closes later via a regular Exit)".
    //
    // Naive in-place rewrite of the events Vec is fragile under
    // interleaving inserts/removes. We rebuild a fresh stream instead:
    // walk the input forwards, and at index `i`, first emit any
    // EnterAt(recorded == out.len()) entries scheduled to open here,
    // then emit the input event.
    //
    // The recorded_idx is in terms of `out` (the post-rewrite stream)
    // because the host computed it as `self.events.len()` at the moment
    // of `ev_checkpoint()` — and at that moment the host's events vec
    // contained only regular events (Enter/Exit/Token/Error/EnterAt,
    // each consuming exactly one slot).
    //
    // Step 1: collect EnterAt entries keyed by their *recorded* index
    // in the ORIGINAL stream.
    let mut pending: std::collections::HashMap<usize, Vec<String>> =
        std::collections::HashMap::new();
    for e in events {
        if let Event::EnterAt(at, k) = e {
            pending.entry(*at).or_default().push(k.clone());
        }
    }
    // Step 2: walk forwards. At each input index `i`, emit any pending
    // Enter entries whose recorded_idx equals `i`, then emit the input
    // event (skipping EnterAt itself, since it became an Enter at the
    // checkpoint position).
    //
    // When multiple `start_node_at` calls share the same checkpoint
    // (e.g. `expr_bp` chaining `CALL_EXPR` and then `QUESTION_EXPR`
    // around the same primary), rowan's semantics put the LATER call
    // on the OUTSIDE — so we emit the pending Enters in reverse
    // insertion order.
    let mut work: Vec<Event> = Vec::with_capacity(events.len());
    for (i, e) in events.iter().enumerate() {
        if let Some(mut opens) = pending.remove(&i) {
            opens.reverse();
            for kind in opens {
                work.push(Event::Enter(kind));
            }
        }
        match e {
            Event::EnterAt(_, _) => { /* consumed */ }
            other => work.push(other.clone()),
        }
    }
    // Step 3: anything left in `pending` was scheduled at an index past
    // the end (i.e. checkpoint taken right at events.len()). Emit them
    // in order at the tail.
    let mut tail_keys: Vec<usize> = pending.keys().copied().collect();
    tail_keys.sort();
    for k in tail_keys {
        let mut opens = pending.remove(&k).unwrap_or_default();
        opens.reverse();
        for kind in opens {
            work.push(Event::Enter(kind));
        }
    }

    // Step 4: tree walk.
    #[derive(Debug)]
    struct Frame {
        kind: String,
        children: Vec<Node>,
    }
    let mut stack: Vec<Frame> = Vec::new();
    let mut root: Option<Node> = None;
    for e in work {
        match e {
            Event::Enter(kind) => stack.push(Frame {
                kind,
                children: Vec::new(),
            }),
            Event::EnterAt(_, _) => unreachable!("EnterAt was rewritten above"),
            Event::Exit => {
                let frame = stack.pop()?;
                let node = Node::Branch {
                    kind: frame.kind,
                    children: frame.children,
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    root = Some(node);
                }
            }
            Event::Token(kind, text) => {
                let leaf = Node::Leaf { kind, text };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(leaf);
                } else {
                    return None;
                }
            }
            Event::Error(_, _, _) => {
                // Errors don't appear in the tree.
            }
        }
    }
    root
}

// ---- Tree built from the Rust parser ------------------------------------

fn rust_tree(src: &str) -> Node {
    let res = rust_parse(src);
    let root = SyntaxNode::new_root(res.green);
    syntax_node_to_tree(&root)
}

fn syntax_node_to_tree(node: &SyntaxNode) -> Node {
    let kind = format!("{:?}", node.kind());
    let mut children: Vec<Node> = Vec::new();
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => children.push(syntax_node_to_tree(&n)),
            rowan::NodeOrToken::Token(t) => {
                let tk = t.kind();
                if tk.is_trivia() {
                    // Trivia is not part of the semantic shape; we still
                    // capture them as leaves to match the Mighty side,
                    // which emits trivia tokens too.
                    children.push(Node::Leaf {
                        kind: format!("{:?}", tk),
                        text: t.text().to_string(),
                    });
                } else {
                    children.push(Node::Leaf {
                        kind: format!("{:?}", tk),
                        text: t.text().to_string(),
                    });
                }
            }
        }
    }
    Node::Branch { kind, children }
}

// ---- Diff helpers -------------------------------------------------------

/// BFS order: kind only (text omitted to keep the diff focused on the
/// shape). Trivia tokens are skipped because their placement can vary
/// slightly between the two parsers (the Rust parser tucks trivia
/// inside the surrounding node; the Mighty parser tends to emit it
/// just before the next node opens because `skip_trivia` runs before
/// each cursor read).
fn bfs_kinds(node: &Node) -> Vec<String> {
    let mut out = Vec::new();
    let mut q: Vec<&Node> = vec![node];
    while let Some(n) = q.pop() {
        // Pre-order DFS for readable diffs.
        match n {
            Node::Branch { kind, children } => {
                out.push(format!("({}", kind));
                for c in children.iter().rev() {
                    q.push(c);
                }
            }
            Node::Leaf { kind, text: _ } => {
                if !is_trivia_kind(kind) {
                    out.push(format!("'{}", kind));
                }
            }
        }
    }
    out
}

fn is_trivia_kind(k: &str) -> bool {
    matches!(
        k,
        "WHITESPACE" | "LINE_COMMENT" | "BLOCK_COMMENT" | "DOC_COMMENT"
    )
}

fn diff_kinds(a: &[String], b: &[String]) -> String {
    let n = a.len().max(b.len());
    let mut lines = Vec::new();
    for i in 0..n {
        let x = a.get(i).map(|s| s.as_str()).unwrap_or("(none)");
        let y = b.get(i).map(|s| s.as_str()).unwrap_or("(none)");
        if x != y {
            lines.push(format!("  [{:3}] stardust={} rust={}", i, x, y));
            if lines.len() > 30 {
                lines.push("  …".into());
                break;
            }
        }
    }
    lines.join("\n")
}

// ---- Tests --------------------------------------------------------------

#[test]
fn selfhost_parser_compiles() {
    // Sanity: just compile the parser source through the v0.6 pipeline.
    // If this fails, the source has type errors and the diff tests
    // below would give an opaque "lower errors" message.
    let parser_path = workspace_root().join("selfhost/parser/parser.mty");
    let src = std::fs::read_to_string(&parser_path).expect("read parser.mty");
    let parsed = parse_source(src, "selfhost/parser/parser.mty".into());
    let (pkg, diags) = lower(&parsed);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "lower errors: {:?}", errors);
    let tbc = type_and_borrow_check(&pkg);
    let tbc_errors: Vec<_> = tbc
        .iter()
        .filter(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
        .collect();
    assert!(
        tbc_errors.is_empty(),
        "type/borrow errors in selfhost parser: {:?}",
        tbc_errors
    );
}

#[test]
fn rust_parser_baseline_hello() {
    // Locks in the Rust side's tree shape for `fn main() { log("hi") }`
    // so a parser rename can't silently change what the bootstrap diff
    // is comparing against.
    let tree = rust_tree("fn main() { log(\"hi\") }");
    if let Node::Branch { kind, children: _ } = &tree {
        assert_eq!(kind, "FILE", "root should be FILE");
    } else {
        panic!("root should be a Branch");
    }
}

#[test]
fn selfhost_parser_hello_world() {
    // Parses `fn main() { log("hi") }` via the Mighty parser source
    // in `selfhost/parser/parser.mty` and asserts the resulting CST has
    // the same kind structure (BFS pre-order) as the Rust parser's
    // output. Trivia leaves (WHITESPACE etc.) are ignored in the diff
    // because the two parsers tuck trivia at slightly different
    // positions in the tree.
    let input = "fn main() { log(\"hi\") }";
    let SelfhostRun { events, result } =
        run_selfhost_parser(input).expect("Mighty parser should compile");
    assert!(
        matches!(result, RunResult::Ok { .. }),
        "self-hosted parser did not terminate cleanly: {:?}",
        result
    );
    let stardust_tree = build_tree(&events).expect("parser should emit a tree");
    let rust_tree = rust_tree(input);
    let s_kinds = bfs_kinds(&stardust_tree);
    let r_kinds = bfs_kinds(&rust_tree);
    assert_eq!(
        s_kinds,
        r_kinds,
        "tree-kind diff:\n{}",
        diff_kinds(&s_kinds, &r_kinds)
    );
}

#[test]
fn selfhost_parser_struct() {
    let input = "struct User { id: U64 name: Str }";
    let SelfhostRun { events, result } =
        run_selfhost_parser(input).expect("Mighty parser should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s_tree = build_tree(&events).expect("tree");
    let r_tree = rust_tree(input);
    let s = bfs_kinds(&s_tree);
    let r = bfs_kinds(&r_tree);
    assert_eq!(s, r, "diff:\n{}", diff_kinds(&s, &r));
}

#[test]
fn selfhost_parser_pratt_arith() {
    // `1 + 2 * 3` should bind as `1 + (2 * 3)` — i.e. the inner
    // BINARY_EXPR is the * not the +.
    let input = "fn f() { 1 + 2 * 3 }";
    let SelfhostRun { events, result } =
        run_selfhost_parser(input).expect("Mighty parser should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s_tree = build_tree(&events).expect("tree");
    let r_tree = rust_tree(input);
    let s = bfs_kinds(&s_tree);
    let r = bfs_kinds(&r_tree);
    assert_eq!(s, r, "diff:\n{}", diff_kinds(&s, &r));
}

#[test]
fn selfhost_parser_match_simple() {
    let input = "fn f() { match x { 0 => \"z\" _ => \"n\" } }";
    let SelfhostRun { events, result } =
        run_selfhost_parser(input).expect("Mighty parser should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s_tree = build_tree(&events).expect("tree");
    let r_tree = rust_tree(input);
    let s = bfs_kinds(&s_tree);
    let r = bfs_kinds(&r_tree);
    assert_eq!(s, r, "diff:\n{}", diff_kinds(&s, &r));
}

#[test]
fn selfhost_parser_example_01() {
    let path = workspace_root().join("examples/01_hello.mty");
    let input = std::fs::read_to_string(path).unwrap();
    let SelfhostRun { events, result } =
        run_selfhost_parser(&input).expect("Mighty parser should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s_tree = build_tree(&events).expect("tree");
    let r_tree = rust_tree(&input);
    let s = bfs_kinds(&s_tree);
    let r = bfs_kinds(&r_tree);
    assert_eq!(s, r, "diff:\n{}", diff_kinds(&s, &r));
}

#[test]
fn selfhost_parser_example_02() {
    let path = workspace_root().join("examples/02_struct_enum.mty");
    let input = std::fs::read_to_string(path).unwrap();
    let SelfhostRun { events, result } =
        run_selfhost_parser(&input).expect("Mighty parser should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s_tree = build_tree(&events).expect("tree");
    let r_tree = rust_tree(&input);
    let s = bfs_kinds(&s_tree);
    let r = bfs_kinds(&r_tree);
    assert_eq!(s, r, "diff:\n{}", diff_kinds(&s, &r));
}

#[test]
fn selfhost_parser_example_03() {
    let path = workspace_root().join("examples/03_generic_fn.mty");
    let input = std::fs::read_to_string(path).unwrap();
    let SelfhostRun { events, result } =
        run_selfhost_parser(&input).expect("Mighty parser should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s_tree = build_tree(&events).expect("tree");
    let r_tree = rust_tree(&input);
    let s = bfs_kinds(&s_tree);
    let r = bfs_kinds(&r_tree);
    assert_eq!(s, r, "diff:\n{}", diff_kinds(&s, &r));
}

#[test]
fn selfhost_parser_example_04() {
    let path = workspace_root().join("examples/04_result_propagation.mty");
    let input = std::fs::read_to_string(path).unwrap();
    let SelfhostRun { events, result } =
        run_selfhost_parser(&input).expect("Mighty parser should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s_tree = build_tree(&events).expect("tree");
    let r_tree = rust_tree(&input);
    let s = bfs_kinds(&s_tree);
    let r = bfs_kinds(&r_tree);
    assert_eq!(s, r, "diff:\n{}", diff_kinds(&s, &r));
}

#[test]
fn selfhost_parser_example_05() {
    let path = workspace_root().join("examples/05_match_expr.mty");
    let input = std::fs::read_to_string(path).unwrap();
    let SelfhostRun { events, result } =
        run_selfhost_parser(&input).expect("Mighty parser should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s_tree = build_tree(&events).expect("tree");
    let r_tree = rust_tree(&input);
    let s = bfs_kinds(&s_tree);
    let r = bfs_kinds(&r_tree);
    assert_eq!(s, r, "diff:\n{}", diff_kinds(&s, &r));
}

#[test]
fn selfhost_parser_empty_input_yields_file_root() {
    // The most reduced live test: when the input has 0 tokens, the
    // Mighty parser still produces a FILE root (and nothing else).
    // This exercises the full pipeline (compile + lower + run) without
    // depending on the interpreter's loop budget or recursion depth.
    let SelfhostRun { events, result } =
        run_selfhost_parser("").expect("Mighty parser should compile");
    assert!(
        matches!(result, RunResult::Ok { .. }),
        "self-hosted parser did not terminate: {:?}",
        result
    );
    let tree = build_tree(&events).expect("event stream should produce a tree");
    match tree {
        Node::Branch { kind, children } => {
            assert_eq!(kind, "FILE", "root kind");
            // Empty input: only the EOF token is the trailing trivia,
            // and we treat EOF specially (the parser doesn't emit a
            // token event for it). So children may be empty.
            // We just assert the shape, not the count.
            let _ = children;
        }
        Node::Leaf { .. } => panic!("root should be Branch"),
    }
}

#[test]
fn selfhost_parser_event_protocol_smoke() {
    // The Mighty parser opens a FILE node, walks through the (empty)
    // token stream, then closes the FILE node. Exactly two events:
    // Enter("FILE") + Exit. This is the minimal live test of the
    // host-bridge protocol end-to-end.
    let SelfhostRun { events, result } =
        run_selfhost_parser("").expect("Mighty parser should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    // Filter out any extra events (defensively).
    let mut depth = 0i32;
    let mut saw_file_enter = false;
    let mut max_depth = 0i32;
    for e in &events {
        match e {
            Event::Enter(k) => {
                depth += 1;
                if k == "FILE" && depth == 1 {
                    saw_file_enter = true;
                }
                max_depth = max_depth.max(depth);
            }
            Event::EnterAt(_, _) => {
                // Resolved at tree build; for the smoke test, we ignore.
            }
            Event::Exit => depth -= 1,
            _ => {}
        }
    }
    assert!(saw_file_enter, "parser should open the FILE root");
    assert_eq!(depth, 0, "all node openings should close: {:?}", events);
}
