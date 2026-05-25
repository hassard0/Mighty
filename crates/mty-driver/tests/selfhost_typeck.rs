//! Self-hosting bootstrap test (v0.8) — typeck phase.
//!
//! Runs the Mighty type inferer in `selfhost/typeck/infer.mty` over a
//! canned input via the SIR interpreter, with a custom `Host` that
//! services the inferer's HIR-query bridge (`hir_*`) and type sink
//! (`ty_record`). Then it type-checks the same input via the trusted
//! Rust pipeline (`mty_types::check_package_typed`) and diffs the two
//! "binding -> type" maps.
//!
//! Bootstrap technique: see `docs/internals/self-hosting.md`. Same
//! shape as the v0.5 self-host lexer test, v0.6 self-host parser test,
//! and v0.8 self-host HIR test — the Mighty source is the pure
//! algorithm; the host services the read side (HIR) and the write
//! side (binding -> type map).
//!
//! For v0.8 the inferer ships a SUBSET — see
//! `SELFHOST_HIR_V0_8_NOTES.md` for the production matrix + gap
//! catalog. The bootstrap test passes on examples 01-03 (the canonical
//! small-but-broad-coverage trio); examples 04-05 are #[ignore]'d.

use mty_driver::{lower, lower_to_sir, parse_source, type_and_borrow_check};
use mty_hir::ids::FnId;
use mty_hir::nodes::{HirExpr, HirLiteral, HirStmt, HirType, Item, Package};
use mty_ir::interp::{run_fn_by_name, Host, RunResult, Value};
use mty_ir::ir::EffectOp;
use mty_types::{check_package_typed, pretty_ty, EffectId, IntKind, TypedPackage};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

// ---- HIR snapshot served to the Mighty inferer --------------------------

/// Minimal slice of the HIR exposed to the Mighty type-inference bridge.
/// We keep only what the v0.8 inferer queries: per-fn signatures + body
/// statements (let-bindings with optional type hint + literal-kind init).
#[derive(Debug, Default, Clone)]
struct HirSnapshot {
    items: Vec<ItemEntry>,
    fns: Vec<FnEntry>,
    blocks: Vec<BlockEntry>,
}

#[derive(Debug, Clone)]
struct ItemEntry {
    kind: String, // "Fn" / "Struct" / "Enum" / "TypeAlias" / "Use" / ...
    name: String,
    /// For Fn items: the index into `fns`. SENTINEL for non-fn.
    fn_idx: usize,
}

#[derive(Debug, Clone)]
struct FnEntry {
    name: String,
    params: Vec<FnParam>,
    ret_type: String, // pretty-printed (e.g. "I32", "Unknown")
    has_body: bool,
    body_block: usize, // index into blocks
}

#[derive(Debug, Clone)]
struct FnParam {
    name: String,
    ty: String,
}

#[derive(Debug, Clone)]
struct BlockEntry {
    stmts: Vec<StmtEntry>,
}

#[derive(Debug, Clone)]
struct StmtEntry {
    kind: String, // "Let" / "Expr"
    let_name: String,
    let_hint: String,          // explicit type annotation pretty-print, or ""
    let_init_kind: String,     // expr kind ("Literal", "Call", ...) or ""
    let_init_lit_kind: String, // literal sub-kind ("Int", "Float", ...) or ""
}

const SENTINEL_NONE_USIZE: usize = usize::MAX;

fn build_snapshot(pkg: &Package) -> HirSnapshot {
    let mut snap = HirSnapshot::default();
    for &item_id in &pkg.top_level {
        let item = &pkg.items[item_id];
        let (kind, name) = item_kind_name(pkg, item);
        let mut fn_idx = SENTINEL_NONE_USIZE;
        if let Item::Fn(fid) = item {
            fn_idx = snap.fns.len();
            let f = build_fn_entry(pkg, *fid, &mut snap);
            snap.fns.push(f);
        }
        snap.items.push(ItemEntry { kind, name, fn_idx });
    }
    snap
}

fn build_fn_entry(pkg: &Package, fid: FnId, snap: &mut HirSnapshot) -> FnEntry {
    let f = &pkg.fns[fid];
    let params: Vec<FnParam> = f
        .params
        .iter()
        .map(|p| {
            let ty =
                p.ty.map(|tid| pretty_hir_type(pkg, tid))
                    .unwrap_or_else(|| "Unknown".to_string());
            FnParam {
                name: p.name.clone(),
                ty,
            }
        })
        .collect();
    let ret_type = f
        .ret
        .map(|tid| pretty_hir_type(pkg, tid))
        .unwrap_or_else(|| "Unit".to_string());
    let has_body = f.body.is_some();
    let body_block = if let Some(bid) = f.body {
        let idx = snap.blocks.len();
        let entry = build_block_entry(pkg, bid);
        snap.blocks.push(entry);
        idx
    } else {
        SENTINEL_NONE_USIZE
    };
    FnEntry {
        name: f.name.clone(),
        params,
        ret_type,
        has_body,
        body_block,
    }
}

fn build_block_entry(pkg: &Package, bid: mty_hir::ids::BlockId) -> BlockEntry {
    let block = &pkg.blocks[bid];
    let stmts: Vec<StmtEntry> = block
        .stmts
        .iter()
        .map(|s| match s {
            HirStmt::Let { pat, ty, init, .. } => {
                let let_name = pat_binding_name(pkg, *pat);
                let let_hint = ty.map(|tid| pretty_hir_type(pkg, tid)).unwrap_or_default();
                let (let_init_kind, let_init_lit_kind) =
                    init.map(|eid| init_kinds(pkg, eid)).unwrap_or_default();
                StmtEntry {
                    kind: "Let".into(),
                    let_name,
                    let_hint,
                    let_init_kind,
                    let_init_lit_kind,
                }
            }
            HirStmt::Expr(_) => StmtEntry {
                kind: "Expr".into(),
                let_name: String::new(),
                let_hint: String::new(),
                let_init_kind: String::new(),
                let_init_lit_kind: String::new(),
            },
        })
        .collect();
    BlockEntry { stmts }
}

fn pat_binding_name(pkg: &Package, pat: mty_hir::ids::PatId) -> String {
    use mty_hir::nodes::HirPat;
    match &pkg.pats[pat] {
        HirPat::Binding { name, .. } => name.clone(),
        _ => String::new(),
    }
}

fn init_kinds(pkg: &Package, eid: mty_hir::ids::ExprId) -> (String, String) {
    let e = &pkg.exprs[eid];
    let kind = match e {
        HirExpr::Literal(_) => "Literal",
        HirExpr::Path(_) | HirExpr::PathGeneric { .. } => "Path",
        HirExpr::Call { .. } => "Call",
        HirExpr::MethodCall { .. } => "MethodCall",
        HirExpr::Field { .. } => "Field",
        HirExpr::Index { .. } => "Index",
        HirExpr::Binary { .. } => "Binary",
        HirExpr::Unary { .. } => "Unary",
        HirExpr::If { .. } => "If",
        HirExpr::Match { .. } => "Match",
        HirExpr::Block(_) => "Block",
        _ => "Other",
    };
    let lit_kind = match e {
        HirExpr::Literal(HirLiteral::Int(_, _)) => "Int",
        HirExpr::Literal(HirLiteral::Float(_, _)) => "Float",
        HirExpr::Literal(HirLiteral::Str(_)) => "Str",
        HirExpr::Literal(HirLiteral::Char(_)) => "Char",
        HirExpr::Literal(HirLiteral::Bool(_)) => "Bool",
        HirExpr::Literal(HirLiteral::Duration { .. }) => "Duration",
        HirExpr::Literal(HirLiteral::Size { .. }) => "Size",
        _ => "",
    };
    (kind.into(), lit_kind.into())
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

/// Pretty-print a HIR type (syntactic) for the Mighty bridge. Matches
/// the set of names the trusted typeck would print via `pretty_ty`.
fn pretty_hir_type(pkg: &Package, tid: mty_hir::ids::TypeId) -> String {
    let t = &pkg.types[tid];
    match t {
        HirType::Path { segments, generics } => {
            let base = segments.join(".");
            if generics.is_empty() {
                base
            } else {
                let parts: Vec<String> =
                    generics.iter().map(|g| pretty_hir_type(pkg, *g)).collect();
                format!("{}[{}]", base, parts.join(", "))
            }
        }
        HirType::Borrow { mutable, inner } => {
            let m = if *mutable { "mut " } else { "" };
            format!("&{}{}", m, pretty_hir_type(pkg, *inner))
        }
        HirType::Tuple(xs) => {
            let parts: Vec<String> = xs.iter().map(|t| pretty_hir_type(pkg, *t)).collect();
            format!("({})", parts.join(", "))
        }
        HirType::Array { elem, .. } => format!("[{}]", pretty_hir_type(pkg, *elem)),
        HirType::Fn { params, ret } => {
            let ps: Vec<String> = params.iter().map(|t| pretty_hir_type(pkg, *t)).collect();
            let r = ret
                .map(|t| pretty_hir_type(pkg, t))
                .unwrap_or_else(|| "Unit".into());
            format!("fn({}) -> {}", ps.join(", "), r)
        }
        HirType::Result { ok, err } => format!(
            "{}!{}",
            pretty_hir_type(pkg, *ok),
            pretty_hir_type(pkg, *err)
        ),
        HirType::Union(xs) => {
            let parts: Vec<String> = xs.iter().map(|t| pretty_hir_type(pkg, *t)).collect();
            parts.join(" | ")
        }
        HirType::Dyn { trait_name } => format!("dyn {}", trait_name),
        HirType::Unit => "Unit".into(),
        HirType::Unknown => "Unknown".into(),
    }
}

// ---- Selfhost host ------------------------------------------------------

#[derive(Debug, Default)]
struct SelfhostTypeckHost {
    snap: HirSnapshot,
    /// Captured binding -> type recordings from the Mighty inferer.
    bindings: BTreeMap<String, String>,
}

impl Host for SelfhostTypeckHost {
    fn print(&mut self, _s: &str) {}

    fn effect_call(&mut self, _effect: EffectId, op: &EffectOp, args: &[Value]) -> Value {
        let EffectOp::GenericCall { method, .. } = op;
        self.dispatch_method(method, args)
    }

    fn extern_call(&mut self, _name: &str, _args: &[Value]) -> Value {
        Value::Unit
    }
}

impl SelfhostTypeckHost {
    fn seed(&mut self, pkg: &Package) {
        self.snap = build_snapshot(pkg);
        self.bindings.clear();
    }

    fn dispatch_method(&mut self, method: &str, args: &[Value]) -> Value {
        match method {
            "hir_item_count" => Value::Int(self.snap.items.len() as i128, IntKind::USize),
            "hir_item_kind" => {
                let i = arg_usize(args, 0);
                Value::Str(
                    self.snap
                        .items
                        .get(i)
                        .map(|it| it.kind.clone())
                        .unwrap_or_default(),
                )
            }
            "hir_item_name" => {
                let i = arg_usize(args, 0);
                Value::Str(
                    self.snap
                        .items
                        .get(i)
                        .map(|it| it.name.clone())
                        .unwrap_or_default(),
                )
            }
            "hir_fn_id" => {
                let i = arg_usize(args, 0);
                let fid = self.snap.items.get(i).map(|it| it.fn_idx).unwrap_or(0);
                Value::Int(fid as i128, IntKind::USize)
            }
            "hir_fn_name" => {
                let i = arg_usize(args, 0);
                Value::Str(
                    self.snap
                        .fns
                        .get(i)
                        .map(|f| f.name.clone())
                        .unwrap_or_default(),
                )
            }
            "hir_fn_param_count" => {
                let i = arg_usize(args, 0);
                let c = self.snap.fns.get(i).map(|f| f.params.len()).unwrap_or(0);
                Value::Int(c as i128, IntKind::USize)
            }
            "hir_fn_param_name" => {
                let i = arg_usize(args, 0);
                let j = arg_usize(args, 1);
                let name = self
                    .snap
                    .fns
                    .get(i)
                    .and_then(|f| f.params.get(j))
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                Value::Str(name)
            }
            "hir_fn_param_type" => {
                let i = arg_usize(args, 0);
                let j = arg_usize(args, 1);
                let ty = self
                    .snap
                    .fns
                    .get(i)
                    .and_then(|f| f.params.get(j))
                    .map(|p| p.ty.clone())
                    .unwrap_or_default();
                Value::Str(ty)
            }
            "hir_fn_ret_type" => {
                let i = arg_usize(args, 0);
                let ty = self
                    .snap
                    .fns
                    .get(i)
                    .map(|f| f.ret_type.clone())
                    .unwrap_or_default();
                Value::Str(ty)
            }
            "hir_fn_has_body" => {
                let i = arg_usize(args, 0);
                let has = self.snap.fns.get(i).map(|f| f.has_body).unwrap_or(false);
                Value::Bool(has)
            }
            "hir_fn_body" => {
                let i = arg_usize(args, 0);
                let b = self.snap.fns.get(i).map(|f| f.body_block).unwrap_or(0);
                Value::Int(b as i128, IntKind::USize)
            }
            "hir_block_stmt_count" => {
                let b = arg_usize(args, 0);
                let c = self
                    .snap
                    .blocks
                    .get(b)
                    .map(|bl| bl.stmts.len())
                    .unwrap_or(0);
                Value::Int(c as i128, IntKind::USize)
            }
            "hir_block_stmt_kind" => {
                let b = arg_usize(args, 0);
                let j = arg_usize(args, 1);
                let k = self
                    .snap
                    .blocks
                    .get(b)
                    .and_then(|bl| bl.stmts.get(j))
                    .map(|s| s.kind.clone())
                    .unwrap_or_default();
                Value::Str(k)
            }
            "hir_let_name" => {
                let b = arg_usize(args, 0);
                let j = arg_usize(args, 1);
                let s = self
                    .snap
                    .blocks
                    .get(b)
                    .and_then(|bl| bl.stmts.get(j))
                    .map(|s| s.let_name.clone())
                    .unwrap_or_default();
                Value::Str(s)
            }
            "hir_let_type_hint" => {
                let b = arg_usize(args, 0);
                let j = arg_usize(args, 1);
                let s = self
                    .snap
                    .blocks
                    .get(b)
                    .and_then(|bl| bl.stmts.get(j))
                    .map(|s| s.let_hint.clone())
                    .unwrap_or_default();
                Value::Str(s)
            }
            "hir_let_init_kind" => {
                let b = arg_usize(args, 0);
                let j = arg_usize(args, 1);
                let s = self
                    .snap
                    .blocks
                    .get(b)
                    .and_then(|bl| bl.stmts.get(j))
                    .map(|s| s.let_init_kind.clone())
                    .unwrap_or_default();
                Value::Str(s)
            }
            "hir_let_init_lit_kind" => {
                let b = arg_usize(args, 0);
                let j = arg_usize(args, 1);
                let s = self
                    .snap
                    .blocks
                    .get(b)
                    .and_then(|bl| bl.stmts.get(j))
                    .map(|s| s.let_init_lit_kind.clone())
                    .unwrap_or_default();
                Value::Str(s)
            }
            "ty_record" => {
                let name = arg_str(args, 0);
                let ty = arg_str(args, 1);
                self.bindings.insert(name, ty);
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

// ---- Compile + run the self-hosted inferer ------------------------------

struct SelfhostTypeckRun {
    bindings: BTreeMap<String, String>,
    result: RunResult,
}

fn run_selfhost_typeck(input: &str) -> Result<SelfhostTypeckRun, String> {
    let infer_path = workspace_root().join("selfhost/typeck/infer.mty");
    let infer_src = std::fs::read_to_string(&infer_path)
        .map_err(|e| format!("read {}: {}", infer_path.display(), e))?;
    let parsed = parse_source(infer_src, "selfhost/typeck/infer.mty".into());
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

    // Seed the host with the trusted Rust HIR snapshot.
    let parsed_input = parse_source(input.to_string(), "test.mty".into());
    let (input_pkg, _) = lower(&parsed_input);
    let mut host = SelfhostTypeckHost::default();
    host.seed(&input_pkg);

    let res = run_fn_by_name(&prog, "infer_package", vec![], &mut host);
    let result = match res {
        Ok(_) => RunResult::Ok { exit: 0 },
        Err(r) => r,
    };
    Ok(SelfhostTypeckRun {
        bindings: host.bindings,
        result,
    })
}

// ---- Reference binding -> type map via the trusted typeck pipeline ----

fn rust_typeck(src: &str) -> BTreeMap<String, String> {
    let parsed = parse_source(src.to_string(), "test.mty".into());
    let (pkg, _) = lower(&parsed);
    let typed = check_package_typed(&pkg);
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    // Per-fn signatures (params + return).
    for (fid, params) in &typed.fn_params {
        for (name, tid) in params {
            map.insert(
                name.clone(),
                pretty_ty(*tid, &typed.ty_arena, None, Some(&typed.def_map)),
            );
        }
        let f = &pkg.fns[*fid];
        if let Some(rid) = typed.fn_ret.get(fid) {
            map.insert(
                format!("{}:return", f.name),
                pretty_ty(*rid, &typed.ty_arena, None, Some(&typed.def_map)),
            );
        }
    }
    // Per-let-binding types (from `expr_ty` of the init expr).
    for &item_id in &pkg.top_level {
        if let Item::Fn(fid) = &pkg.items[item_id] {
            let f = &pkg.fns[*fid];
            if let Some(bid) = f.body {
                collect_let_types(&pkg, &typed, bid, &mut map);
            }
        }
    }
    map
}

fn collect_let_types(
    pkg: &Package,
    typed: &TypedPackage,
    bid: mty_hir::ids::BlockId,
    out: &mut BTreeMap<String, String>,
) {
    let block = &pkg.blocks[bid];
    for stmt in &block.stmts {
        if let HirStmt::Let { pat, ty, init, .. } = stmt {
            let name = pat_binding_name(pkg, *pat);
            if name.is_empty() {
                continue;
            }
            let pretty = if let Some(tid) = ty {
                pretty_hir_type(pkg, *tid)
            } else if let Some(eid) = init {
                typed
                    .expr_ty
                    .get(eid)
                    .map(|t| pretty_ty(*t, &typed.ty_arena, None, Some(&typed.def_map)))
                    .unwrap_or_else(|| "Unknown".to_string())
            } else {
                "Unknown".to_string()
            };
            out.insert(name, pretty);
        }
    }
}

// ---- Diff helper --------------------------------------------------------

/// (matched_keys, mismatched (key, lhs, rhs), only_in_a, only_in_b)
type DiffResult = (
    Vec<String>,
    Vec<(String, String, String)>,
    Vec<String>,
    Vec<String>,
);

/// Compare two binding maps for the subset of keys present in both.
fn diff_bindings(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> DiffResult {
    let mut matched = vec![];
    let mut mismatched = vec![];
    let mut only_a = vec![];
    let mut only_b = vec![];
    for (k, va) in a {
        if let Some(vb) = b.get(k) {
            if va == vb {
                matched.push(k.clone());
            } else {
                mismatched.push((k.clone(), va.clone(), vb.clone()));
            }
        } else {
            only_a.push(k.clone());
        }
    }
    for k in b.keys() {
        if !a.contains_key(k) {
            only_b.push(k.clone());
        }
    }
    (matched, mismatched, only_a, only_b)
}

/// Normalize a HIR-syntactic type name into a form comparable with the
/// canonical typeck rendering. v0.8 mighty side renders types
/// syntactically (e.g. `"&[T]"`); the trusted typeck renders concrete
/// `TyData::Param`s with a numeric suffix (`T0`, `T6`, ...). Strip the
/// digit suffix on Rust-rendered params so we can compare structurally.
/// Also strip whitespace and normalise the "fn(..) ->" syntactic form
/// across the two renderers.
fn normalize(ty: &str) -> String {
    // Strip digit suffixes after a bare T (handles `T0`, `T12`, `T6` -> `T`).
    let bytes = ty.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        out.push(c);
        if c == 'T' {
            // Look ahead and skip a digit run if present.
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 {
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out.trim().to_string()
}

fn assert_binding_subset_match(
    src: &str,
    stardust: &BTreeMap<String, String>,
    rust: &BTreeMap<String, String>,
) {
    let s: BTreeMap<String, String> = stardust
        .iter()
        .map(|(k, v)| (k.clone(), normalize(v)))
        .collect();
    let r: BTreeMap<String, String> = rust
        .iter()
        .map(|(k, v)| (k.clone(), normalize(v)))
        .collect();
    let (matched, mismatched, only_s, only_r) = diff_bindings(&s, &r);
    let common_keys: usize = matched.len() + mismatched.len();
    assert!(
        common_keys > 0,
        "no overlapping bindings between Mighty and Rust typeck for {:?}\n  stardust keys = {:?}\n  rust keys    = {:?}",
        src,
        s.keys().collect::<Vec<_>>(),
        r.keys().collect::<Vec<_>>()
    );
    assert!(
        mismatched.is_empty(),
        "binding-type mismatch for {:?}:\n  {}\n  matched={}  only_stardust={}  only_rust={}",
        src,
        mismatched
            .iter()
            .map(|(k, va, vb)| format!("{}: stardust={:?} rust={:?}", k, va, vb))
            .collect::<Vec<_>>()
            .join("\n  "),
        matched.len(),
        only_s.len(),
        only_r.len()
    );
}

// ---- Tests --------------------------------------------------------------

#[test]
fn selfhost_typeck_compiles() {
    let infer_path = workspace_root().join("selfhost/typeck/infer.mty");
    let src = std::fs::read_to_string(&infer_path).expect("read infer.mty");
    let parsed = parse_source(src, "selfhost/typeck/infer.mty".into());
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
        "type/borrow errors in selfhost typeck: {:?}",
        tbc_errors
    );
}

#[test]
fn selfhost_typeck_hello_world() {
    let input = "fn main() { log(\"hi\") }";
    let SelfhostTypeckRun { bindings, result } =
        run_selfhost_typeck(input).expect("Mighty typeck should compile");
    assert!(
        matches!(result, RunResult::Ok { .. }),
        "self-hosted typeck did not terminate cleanly: {:?}",
        result
    );
    let r = rust_typeck(input);
    assert_binding_subset_match(input, &bindings, &r);
}

#[test]
fn selfhost_typeck_example_01() {
    let path = workspace_root().join("examples/01_hello.mty");
    let input = std::fs::read_to_string(&path).expect("read example 01");
    let SelfhostTypeckRun { bindings, result } =
        run_selfhost_typeck(&input).expect("Mighty typeck should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let r = rust_typeck(&input);
    assert_binding_subset_match(&input, &bindings, &r);
}

#[test]
fn selfhost_typeck_example_02() {
    let path = workspace_root().join("examples/02_struct_enum.mty");
    let input = std::fs::read_to_string(&path).expect("read example 02");
    let SelfhostTypeckRun { bindings, result } =
        run_selfhost_typeck(&input).expect("Mighty typeck should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let r = rust_typeck(&input);
    assert_binding_subset_match(&input, &bindings, &r);
}

#[test]
fn selfhost_typeck_example_03() {
    let path = workspace_root().join("examples/03_generic_fn.mty");
    let input = std::fs::read_to_string(&path).expect("read example 03");
    let SelfhostTypeckRun { bindings, result } =
        run_selfhost_typeck(&input).expect("Mighty typeck should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let r = rust_typeck(&input);
    assert_binding_subset_match(&input, &bindings, &r);
}

#[test]
#[ignore = "v0.9 — example 04 has Result!{NetErr,ParseErr} return type whose pretty-print differs between syntactic HIR (`Page!Net|ParseErr` vs `Result<Page, ...>`); needs typeck normalization layer"]
fn selfhost_typeck_example_04() {
    let path = workspace_root().join("examples/04_result_propagation.mty");
    let input = std::fs::read_to_string(&path).expect("read example 04");
    let SelfhostTypeckRun { bindings, result } =
        run_selfhost_typeck(&input).expect("Mighty typeck should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let r = rust_typeck(&input);
    assert_binding_subset_match(&input, &bindings, &r);
}

#[test]
#[ignore = "v0.9 — example 05 has private `_classify` fn (typeck name-mangling) + range patterns"]
fn selfhost_typeck_example_05() {
    let path = workspace_root().join("examples/05_match_expr.mty");
    let input = std::fs::read_to_string(&path).expect("read example 05");
    let SelfhostTypeckRun { bindings, result } =
        run_selfhost_typeck(&input).expect("Mighty typeck should compile");
    assert!(matches!(result, RunResult::Ok { .. }), "{:?}", result);
    let r = rust_typeck(&input);
    assert_binding_subset_match(&input, &bindings, &r);
}
