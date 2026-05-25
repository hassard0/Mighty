//! Self-hosting bootstrap test (v0.8) — HIR phase.
//!
//! Runs the Mighty HIR lowerer in `selfhost/hir/lower.mty` over a canned
//! input via the SIR interpreter, with a custom `Host` that services the
//! lowerer's CST navigation bridge (`cst_*`) and HIR sink bridge
//! (`hir_emit_*`). Then it lowers the same input via the trusted Rust
//! HIR pipeline (`mty_hir::lower::LoweringCtx`) and diffs the two
//! "item-kind sequences".
//!
//! Bootstrap technique: see `docs/internals/self-hosting.md`. Same
//! shape as the v0.5 self-host lexer test and v0.6 self-host parser
//! test — the Mighty source is the pure algorithm; the host services
//! the read side (CST) and the write side (HIR sink).
//!
//! For v0.8 the lowerer ships a SUBSET — see
//! `SELFHOST_HIR_V0_8_NOTES.md` for the production matrix + gap
//! catalog. The bootstrap test passes on examples 01-03 (the canonical
//! small-but-broad-coverage trio); examples 04-05 are #[ignore]'d.

use mty_driver::{lower, lower_to_sir, parse_source, type_and_borrow_check};
use mty_hir::nodes::{HirExpr, HirStmt, Item, Package};
use mty_ir::interp::{run_fn_by_name, Host, RunResult, Value};
use mty_ir::ir::EffectOp;
use mty_syntax::{parse as rust_parse, SyntaxKind, SyntaxNode};
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

/// Event captured from the Mighty lowerer's `hir_emit_*` bridge calls.
#[derive(Debug, Clone, PartialEq, Eq)]
enum HirEvent {
    Item { kind: String, name: String },
    Expr { kind: String },
    Stmt { kind: String },
    Type { kind: String },
    Pat { kind: String },
    BlockStart,
    BlockEnd,
}

const SENTINEL_NONE: i128 = 4294967295;

#[derive(Debug, Default)]
struct SelfhostHirHost {
    cst: CstArena,
    events: Vec<HirEvent>,
    next_id: usize,
}

/// Flat arena over the rowan CST. Each non-trivia node is assigned a
/// stable USize id; the Mighty lowerer navigates via these ids.
#[derive(Debug, Default)]
struct CstArena {
    nodes: Vec<SyntaxNode>,
}

impl CstArena {
    fn from_root(root: SyntaxNode) -> Self {
        let mut nodes = Vec::new();
        Self::collect(&root, &mut nodes);
        Self { nodes }
    }
    fn collect(n: &SyntaxNode, out: &mut Vec<SyntaxNode>) {
        out.push(n.clone());
        for c in n.children() {
            Self::collect(&c, out);
        }
    }
    fn root_id(&self) -> usize {
        0
    }
    fn get(&self, id: usize) -> Option<&SyntaxNode> {
        self.nodes.get(id)
    }
    fn id_of(&self, n: &SyntaxNode) -> usize {
        self.nodes.iter().position(|x| x == n).unwrap_or(usize::MAX)
    }
}

impl Host for SelfhostHirHost {
    fn print(&mut self, _s: &str) {}

    fn effect_call(&mut self, _effect: EffectId, op: &EffectOp, args: &[Value]) -> Value {
        let EffectOp::GenericCall { method, .. } = op;
        self.dispatch_method(method, args)
    }

    fn extern_call(&mut self, _name: &str, _args: &[Value]) -> Value {
        Value::Unit
    }
}

impl SelfhostHirHost {
    fn seed(&mut self, src: &str) {
        let parsed = rust_parse(src);
        let root = SyntaxNode::new_root(parsed.green);
        self.cst = CstArena::from_root(root);
        self.events.clear();
        self.next_id = 0;
    }

    fn alloc_id(&mut self) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        Value::Int(id as i128, IntKind::USize)
    }

    fn dispatch_method(&mut self, method: &str, args: &[Value]) -> Value {
        match method {
            "cst_root" => Value::Int(self.cst.root_id() as i128, IntKind::USize),
            "cst_node_kind" => {
                let id = arg_usize(args, 0);
                let name = self
                    .cst
                    .get(id)
                    .map(|n| format!("{:?}", n.kind()))
                    .unwrap_or_else(|| "ERROR".to_string());
                Value::Str(name)
            }
            "cst_child_count" => {
                let id = arg_usize(args, 0);
                let count = self.cst.get(id).map(|n| n.children().count()).unwrap_or(0);
                Value::Int(count as i128, IntKind::USize)
            }
            "cst_child" => {
                let id = arg_usize(args, 0);
                let i = arg_usize(args, 1);
                let cid = self
                    .cst
                    .get(id)
                    .and_then(|n| n.children().nth(i))
                    .map(|c| self.cst.id_of(&c))
                    .unwrap_or(usize::MAX);
                Value::Int(cid as i128, IntKind::USize)
            }
            "cst_token_count" => {
                let id = arg_usize(args, 0);
                let count = self
                    .cst
                    .get(id)
                    .map(|n| {
                        n.children_with_tokens()
                            .filter_map(|e| e.into_token())
                            .filter(|t| !t.kind().is_trivia())
                            .count()
                    })
                    .unwrap_or(0);
                Value::Int(count as i128, IntKind::USize)
            }
            "cst_token_kind" => {
                let id = arg_usize(args, 0);
                let i = arg_usize(args, 1);
                let kind = self
                    .cst
                    .get(id)
                    .and_then(|n| {
                        n.children_with_tokens()
                            .filter_map(|e| e.into_token())
                            .filter(|t| !t.kind().is_trivia())
                            .nth(i)
                    })
                    .map(|t| format!("{:?}", t.kind()))
                    .unwrap_or_default();
                Value::Str(kind)
            }
            "cst_token_text" => {
                let id = arg_usize(args, 0);
                let i = arg_usize(args, 1);
                let text = self
                    .cst
                    .get(id)
                    .and_then(|n| {
                        n.children_with_tokens()
                            .filter_map(|e| e.into_token())
                            .filter(|t| !t.kind().is_trivia())
                            .nth(i)
                    })
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                Value::Str(text)
            }
            "cst_text" => {
                let id = arg_usize(args, 0);
                let text = self
                    .cst
                    .get(id)
                    .map(|n| n.text().to_string())
                    .unwrap_or_default();
                Value::Str(text)
            }
            "cst_span_start" => {
                let id = arg_usize(args, 0);
                let s = self
                    .cst
                    .get(id)
                    .map(|n| usize::from(n.text_range().start()))
                    .unwrap_or(0);
                Value::Int(s as i128, IntKind::USize)
            }
            "cst_span_end" => {
                let id = arg_usize(args, 0);
                let e = self
                    .cst
                    .get(id)
                    .map(|n| usize::from(n.text_range().end()))
                    .unwrap_or(0);
                Value::Int(e as i128, IntKind::USize)
            }
            "cst_first_name" => {
                let id = arg_usize(args, 0);
                let name = self
                    .cst
                    .get(id)
                    .and_then(|n| n.children().find(|c| c.kind() == SyntaxKind::NAME))
                    .and_then(|n| n.first_token())
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                Value::Str(name)
            }
            "cst_has_pub" => {
                let id = arg_usize(args, 0);
                let has = self
                    .cst
                    .get(id)
                    .map(|n| n.children().any(|c| c.kind() == SyntaxKind::VISIBILITY))
                    .unwrap_or(false);
                Value::Bool(has)
            }
            "cst_has_unsafe" => {
                let id = arg_usize(args, 0);
                let has = self
                    .cst
                    .get(id)
                    .map(|n| {
                        n.children_with_tokens()
                            .filter_map(|e| e.into_token())
                            .any(|t| t.kind() == SyntaxKind::UNSAFE_KW)
                    })
                    .unwrap_or(false);
                Value::Bool(has)
            }
            "cst_find_child" => {
                let id = arg_usize(args, 0);
                let kind = arg_str(args, 1);
                let cid = self
                    .cst
                    .get(id)
                    .and_then(|n| n.children().find(|c| format!("{:?}", c.kind()) == kind))
                    .map(|c| self.cst.id_of(&c) as i128)
                    .unwrap_or(SENTINEL_NONE);
                Value::Int(cid, IntKind::USize)
            }
            "cst_find_descendant" => {
                let id = arg_usize(args, 0);
                let kind = arg_str(args, 1);
                let cid = self
                    .cst
                    .get(id)
                    .and_then(|n| n.descendants().find(|c| format!("{:?}", c.kind()) == kind))
                    .map(|c| self.cst.id_of(&c) as i128)
                    .unwrap_or(SENTINEL_NONE);
                Value::Int(cid, IntKind::USize)
            }
            // -- HIR sink --
            "hir_emit_item" => {
                let kind = arg_str(args, 0);
                let name = arg_str(args, 1);
                self.events.push(HirEvent::Item { kind, name });
                self.alloc_id()
            }
            "hir_emit_fn_sig" => {
                let name = arg_str(args, 0);
                self.events.push(HirEvent::Item {
                    kind: "Fn".to_string(),
                    name,
                });
                self.alloc_id()
            }
            "hir_emit_fn_param" => Value::Unit,
            "hir_emit_fn_generic" => Value::Unit,
            "hir_emit_fn_body_start" => Value::Unit,
            "hir_emit_fn_body_end" => Value::Unit,
            "hir_emit_struct" => {
                let name = arg_str(args, 0);
                self.events.push(HirEvent::Item {
                    kind: "Struct".to_string(),
                    name,
                });
                self.alloc_id()
            }
            "hir_emit_struct_field" => Value::Unit,
            "hir_emit_enum" => {
                let name = arg_str(args, 0);
                self.events.push(HirEvent::Item {
                    kind: "Enum".to_string(),
                    name,
                });
                self.alloc_id()
            }
            "hir_emit_enum_variant" => Value::Unit,
            "hir_emit_type_alias" => {
                let name = arg_str(args, 0);
                self.events.push(HirEvent::Item {
                    kind: "TypeAlias".to_string(),
                    name,
                });
                self.alloc_id()
            }
            "hir_emit_use" => {
                self.events.push(HirEvent::Item {
                    kind: "Use".to_string(),
                    name: String::new(),
                });
                self.alloc_id()
            }
            "hir_emit_mod" => {
                self.events.push(HirEvent::Item {
                    kind: "Mod".to_string(),
                    name: String::new(),
                });
                self.alloc_id()
            }
            "hir_emit_extern_block" => {
                self.events.push(HirEvent::Item {
                    kind: "ExternBlock".to_string(),
                    name: String::new(),
                });
                self.alloc_id()
            }
            "hir_emit_expr" => {
                let kind = arg_str(args, 0);
                self.events.push(HirEvent::Expr { kind });
                self.alloc_id()
            }
            "hir_emit_stmt" => {
                let kind = arg_str(args, 0);
                self.events.push(HirEvent::Stmt { kind });
                self.alloc_id()
            }
            "hir_emit_type" => {
                let kind = arg_str(args, 0);
                self.events.push(HirEvent::Type { kind });
                self.alloc_id()
            }
            "hir_emit_pat" => {
                let kind = arg_str(args, 0);
                self.events.push(HirEvent::Pat { kind });
                self.alloc_id()
            }
            "hir_emit_block_start" => {
                self.events.push(HirEvent::BlockStart);
                self.alloc_id()
            }
            "hir_emit_block_end" => {
                self.events.push(HirEvent::BlockEnd);
                self.alloc_id()
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

// ---- Compile + run the self-hosted lowerer ------------------------------

struct SelfhostHirRun {
    events: Vec<HirEvent>,
    result: RunResult,
}

fn run_selfhost_hir(input: &str) -> Result<SelfhostHirRun, String> {
    let lower_path = workspace_root().join("selfhost/hir/lower.mty");
    let lower_src = std::fs::read_to_string(&lower_path)
        .map_err(|e| format!("read {}: {}", lower_path.display(), e))?;
    let parsed = parse_source(lower_src, "selfhost/hir/lower.mty".into());
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

    let mut host = SelfhostHirHost::default();
    host.seed(input);
    let res = run_fn_by_name(&prog, "lower_file", vec![], &mut host);
    let result = match res {
        Ok(_) => RunResult::Ok { exit: 0 },
        Err(r) => r,
    };
    Ok(SelfhostHirRun {
        events: host.events,
        result,
    })
}

// ---- Reference HIR via the trusted Rust pipeline ------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct HirSummary {
    items: Vec<String>,
    item_names: Vec<String>,
    expr_kinds: Vec<String>,
}

fn rust_hir(src: &str) -> HirSummary {
    let parsed = parse_source(src.to_string(), "test.mty".into());
    let (pkg, _) = lower(&parsed);
    summarize_package(&pkg)
}

fn summarize_package(pkg: &Package) -> HirSummary {
    let mut items: Vec<String> = vec![];
    let mut item_names: Vec<String> = vec![];
    let mut expr_kinds: Vec<String> = vec![];
    for &item_id in &pkg.top_level {
        let item = &pkg.items[item_id];
        let (kind, name) = item_kind_name(pkg, item);
        items.push(kind);
        item_names.push(name);
        if let Item::Fn(fn_id) = item {
            let f = &pkg.fns[*fn_id];
            if let Some(block_id) = f.body {
                collect_expr_kinds_block(pkg, block_id, &mut expr_kinds);
            }
        }
    }
    HirSummary {
        items,
        item_names,
        expr_kinds,
    }
}

fn item_kind_name(pkg: &Package, item: &Item) -> (String, String) {
    match item {
        Item::Fn(id) => ("Fn".into(), pkg.fns[*id].name.clone()),
        Item::Struct(id) => ("Struct".into(), pkg.structs[*id].name.clone()),
        Item::Enum(id) => ("Enum".into(), pkg.enums[*id].name.clone()),
        Item::TypeAlias(id) => ("TypeAlias".into(), pkg.type_aliases[*id].name.clone()),
        Item::Use(_) => ("Use".into(), String::new()),
        Item::Mod(_) => ("Mod".into(), String::new()),
        Item::ExternBlock(_) => ("ExternBlock".into(), String::new()),
        Item::Impl(_) => ("Impl".into(), String::new()),
        Item::Trait(t) => ("Trait".into(), t.name.clone()),
        Item::Const(c) => ("Const".into(), c.name.clone()),
        Item::Agent(id) => ("Agent".into(), pkg.agents[*id].name.clone()),
        Item::Protocol(id) => ("Protocol".into(), pkg.protocols[*id].name.clone()),
        Item::Supervisor(id) => ("Supervisor".into(), pkg.supervisors[*id].name.clone()),
        Item::ExportDecl(_) => ("Export".into(), String::new()),
        Item::Macro(m) => ("Macro".into(), m.name.clone()),
        Item::Sandbox(s) => ("Sandbox".into(), s.name.clone()),
    }
}

fn collect_expr_kinds_block(pkg: &Package, block_id: mty_hir::ids::BlockId, out: &mut Vec<String>) {
    let block = &pkg.blocks[block_id];
    for stmt in &block.stmts {
        match stmt {
            HirStmt::Let { init, .. } => {
                if let Some(eid) = init {
                    collect_expr_kinds(pkg, *eid, out);
                }
            }
            HirStmt::Expr(eid) => collect_expr_kinds(pkg, *eid, out),
        }
    }
    if let Some(tail) = block.tail {
        collect_expr_kinds(pkg, tail, out);
    }
}

fn collect_expr_kinds(pkg: &Package, eid: mty_hir::ids::ExprId, out: &mut Vec<String>) {
    let e = &pkg.exprs[eid];
    out.push(expr_kind_name(e));
    match e {
        HirExpr::Literal(_) | HirExpr::Path(_) | HirExpr::PathGeneric { .. } => {}
        HirExpr::Call { callee, args } => {
            collect_expr_kinds(pkg, *callee, out);
            for a in args {
                collect_expr_kinds(pkg, a.value, out);
            }
        }
        HirExpr::MethodCall { receiver, args, .. } => {
            collect_expr_kinds(pkg, *receiver, out);
            for a in args {
                collect_expr_kinds(pkg, a.value, out);
            }
        }
        HirExpr::Field { receiver, .. } => collect_expr_kinds(pkg, *receiver, out),
        HirExpr::Index { receiver, idx } => {
            collect_expr_kinds(pkg, *receiver, out);
            collect_expr_kinds(pkg, *idx, out);
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            collect_expr_kinds(pkg, *lhs, out);
            collect_expr_kinds(pkg, *rhs, out);
        }
        HirExpr::Unary { rhs, .. } => collect_expr_kinds(pkg, *rhs, out),
        HirExpr::If { cond, then, else_ } => {
            collect_expr_kinds(pkg, *cond, out);
            collect_expr_kinds_block(pkg, *then, out);
            if let Some(e) = else_ {
                collect_expr_kinds(pkg, *e, out);
            }
        }
        HirExpr::Match { scrutinee, arms } => {
            collect_expr_kinds(pkg, *scrutinee, out);
            for arm in arms {
                if let Some(g) = arm.guard {
                    collect_expr_kinds(pkg, g, out);
                }
                collect_expr_kinds(pkg, arm.body, out);
            }
        }
        HirExpr::For { iter, body, .. } => {
            collect_expr_kinds(pkg, *iter, out);
            collect_expr_kinds_block(pkg, *body, out);
        }
        HirExpr::While { cond, body } => {
            collect_expr_kinds(pkg, *cond, out);
            collect_expr_kinds_block(pkg, *body, out);
        }
        HirExpr::Loop { body } => collect_expr_kinds_block(pkg, *body, out),
        HirExpr::Return(opt) | HirExpr::Break(opt) => {
            if let Some(e) = opt {
                collect_expr_kinds(pkg, *e, out);
            }
        }
        HirExpr::Continue => {}
        HirExpr::Block(b) => collect_expr_kinds_block(pkg, *b, out),
        HirExpr::Tuple(es) | HirExpr::Array(es) => {
            for e in es {
                collect_expr_kinds(pkg, *e, out);
            }
        }
        HirExpr::Struct { fields, .. } => {
            for (_, v) in fields {
                collect_expr_kinds(pkg, *v, out);
            }
        }
        HirExpr::Map(es) => {
            for (k, v) in es {
                collect_expr_kinds(pkg, *k, out);
                collect_expr_kinds(pkg, *v, out);
            }
        }
        HirExpr::Send { target, args, .. } | HirExpr::Ask { target, args, .. } => {
            collect_expr_kinds(pkg, *target, out);
            for a in args {
                collect_expr_kinds(pkg, a.value, out);
            }
        }
        HirExpr::Deadline { inner, dur } => {
            collect_expr_kinds(pkg, *inner, out);
            collect_expr_kinds(pkg, *dur, out);
        }
        HirExpr::Question(e) | HirExpr::Move(e) | HirExpr::Detach(e) | HirExpr::Join(e) => {
            collect_expr_kinds(pkg, *e, out);
        }
        HirExpr::Borrow { inner, .. } => collect_expr_kinds(pkg, *inner, out),
        HirExpr::Spawn { inner, .. } => collect_expr_kinds(pkg, *inner, out),
        HirExpr::HtmlTemplate(_) => {}
        HirExpr::Unsafe(b) => collect_expr_kinds_block(pkg, *b, out),
        HirExpr::Arena { body, .. } => collect_expr_kinds(pkg, *body, out),
        HirExpr::TaskScope { deadline, body } => {
            if let Some(d) = deadline {
                collect_expr_kinds(pkg, *d, out);
            }
            collect_expr_kinds_block(pkg, *body, out);
        }
        HirExpr::Budget { entries, body } => {
            for (_, v) in entries {
                collect_expr_kinds(pkg, *v, out);
            }
            collect_expr_kinds(pkg, *body, out);
        }
        HirExpr::Sandbox { entries, body, .. } => {
            for (_, v) in entries {
                collect_expr_kinds(pkg, *v, out);
            }
            collect_expr_kinds_block(pkg, *body, out);
        }
        HirExpr::Cast { lhs, .. } => collect_expr_kinds(pkg, *lhs, out),
        HirExpr::Lambda { body, .. } => collect_expr_kinds_block(pkg, *body, out),
        HirExpr::IfLet {
            scrutinee,
            then,
            else_,
            ..
        } => {
            collect_expr_kinds(pkg, *scrutinee, out);
            collect_expr_kinds_block(pkg, *then, out);
            if let Some(e) = else_ {
                collect_expr_kinds(pkg, *e, out);
            }
        }
        HirExpr::Run(e) => collect_expr_kinds(pkg, *e, out),
        HirExpr::Error => {}
    }
}

fn expr_kind_name(e: &HirExpr) -> String {
    match e {
        HirExpr::Literal(_) => "Literal",
        HirExpr::Path(_) => "Path",
        HirExpr::PathGeneric { .. } => "Path",
        HirExpr::Call { .. } => "Call",
        HirExpr::MethodCall { .. } => "MethodCall",
        HirExpr::Field { .. } => "Field",
        HirExpr::Index { .. } => "Index",
        HirExpr::Binary { .. } => "Binary",
        HirExpr::Unary { .. } => "Unary",
        HirExpr::If { .. } => "If",
        HirExpr::Match { .. } => "Match",
        HirExpr::For { .. } => "For",
        HirExpr::While { .. } => "While",
        HirExpr::Loop { .. } => "Loop",
        HirExpr::Return(_) => "Return",
        HirExpr::Break(_) => "Break",
        HirExpr::Continue => "Continue",
        HirExpr::Block(_) => "Block",
        HirExpr::Tuple(_) => "Tuple",
        HirExpr::Array(_) => "Array",
        HirExpr::Struct { .. } => "Struct",
        HirExpr::Map(_) => "Map",
        HirExpr::Send { .. } => "Send",
        HirExpr::Ask { .. } => "Ask",
        HirExpr::Deadline { .. } => "Deadline",
        HirExpr::Question(_) => "Question",
        HirExpr::Move(_) => "Move",
        HirExpr::Borrow { .. } => "Borrow",
        HirExpr::Spawn { .. } => "Spawn",
        HirExpr::Detach(_) => "Detach",
        HirExpr::Join(_) => "Join",
        HirExpr::HtmlTemplate(_) => "HtmlTemplate",
        HirExpr::Unsafe(_) => "Unsafe",
        HirExpr::Arena { .. } => "Arena",
        HirExpr::TaskScope { .. } => "TaskScope",
        HirExpr::Budget { .. } => "Budget",
        HirExpr::Sandbox { .. } => "Sandbox",
        HirExpr::Cast { .. } => "Cast",
        HirExpr::Lambda { .. } => "Lambda",
        HirExpr::IfLet { .. } => "IfLet",
        HirExpr::Run(_) => "Run",
        HirExpr::Error => "Error",
    }
    .to_string()
}

// ---- Stardust-side summary extracted from the event stream --------------

fn stardust_summary(events: &[HirEvent]) -> HirSummary {
    let mut items: Vec<String> = vec![];
    let mut item_names: Vec<String> = vec![];
    let mut expr_kinds: Vec<String> = vec![];
    for e in events {
        match e {
            HirEvent::Item { kind, name } => {
                items.push(kind.clone());
                item_names.push(name.clone());
            }
            HirEvent::Expr { kind } => expr_kinds.push(kind.clone()),
            _ => {}
        }
    }
    HirSummary {
        items,
        item_names,
        expr_kinds,
    }
}

// ---- Tests --------------------------------------------------------------

#[test]
fn selfhost_hir_compiles() {
    let lower_path = workspace_root().join("selfhost/hir/lower.mty");
    let src = std::fs::read_to_string(&lower_path).expect("read lower.mty");
    let parsed = parse_source(src, "selfhost/hir/lower.mty".into());
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
        "type/borrow errors in selfhost hir: {:?}",
        tbc_errors
    );
}

#[test]
fn selfhost_hir_hello_world() {
    let input = "fn main() { log(\"hi\") }";
    let SelfhostHirRun { events, result } =
        run_selfhost_hir(input).expect("Mighty HIR lowerer should compile");
    assert!(
        matches!(result, RunResult::Ok { .. }),
        "self-hosted HIR did not terminate cleanly: {:?}",
        result
    );
    let s = stardust_summary(&events);
    let r = rust_hir(input);
    assert_eq!(
        s.items, r.items,
        "item-kind diff:\n  stardust={:?}\n  rust    ={:?}",
        s.items, r.items
    );
    assert_eq!(
        s.item_names, r.item_names,
        "item-name diff:\n  stardust={:?}\n  rust    ={:?}",
        s.item_names, r.item_names
    );
}

#[test]
fn selfhost_hir_example_01() {
    let path = workspace_root().join("examples/01_hello.mty");
    let input = std::fs::read_to_string(&path).expect("read example 01");
    let SelfhostHirRun { events, result } =
        run_selfhost_hir(&input).expect("Mighty HIR lowerer should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s = stardust_summary(&events);
    let r = rust_hir(&input);
    assert_eq!(s.items, r.items, "item-kind diff (ex01)");
    assert_eq!(s.item_names, r.item_names, "item-name diff (ex01)");
}

#[test]
fn selfhost_hir_example_02() {
    let path = workspace_root().join("examples/02_struct_enum.mty");
    let input = std::fs::read_to_string(&path).expect("read example 02");
    let SelfhostHirRun { events, result } =
        run_selfhost_hir(&input).expect("Mighty HIR lowerer should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s = stardust_summary(&events);
    let r = rust_hir(&input);
    assert_eq!(
        s.items, r.items,
        "item-kind diff (ex02):\n  stardust={:?}\n  rust    ={:?}",
        s.items, r.items
    );
    assert_eq!(
        s.item_names, r.item_names,
        "item-name diff (ex02):\n  stardust={:?}\n  rust    ={:?}",
        s.item_names, r.item_names
    );
}

#[test]
fn selfhost_hir_example_03() {
    let path = workspace_root().join("examples/03_generic_fn.mty");
    let input = std::fs::read_to_string(&path).expect("read example 03");
    let SelfhostHirRun { events, result } =
        run_selfhost_hir(&input).expect("Mighty HIR lowerer should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s = stardust_summary(&events);
    let r = rust_hir(&input);
    assert_eq!(s.items, r.items, "item-kind diff (ex03)");
    assert_eq!(s.item_names, r.item_names, "item-name diff (ex03)");
}

#[test]
fn selfhost_hir_example_04() {
    let path = workspace_root().join("examples/04_result_propagation.mty");
    let input = std::fs::read_to_string(&path).expect("read example 04");
    let SelfhostHirRun { events, result } =
        run_selfhost_hir(&input).expect("Mighty HIR lowerer should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s = stardust_summary(&events);
    let r = rust_hir(&input);
    assert_eq!(s.items, r.items, "item-kind diff (ex04)");
}

#[test]
fn selfhost_hir_example_05() {
    let path = workspace_root().join("examples/05_match_expr.mty");
    let input = std::fs::read_to_string(&path).expect("read example 05");
    let SelfhostHirRun { events, result } =
        run_selfhost_hir(&input).expect("Mighty HIR lowerer should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let s = stardust_summary(&events);
    let r = rust_hir(&input);
    assert_eq!(s.items, r.items, "item-kind diff (ex05)");
}
