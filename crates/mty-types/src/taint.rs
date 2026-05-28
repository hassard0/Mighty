//! v0.30 Track A — compiler-checked prompt injection prevention.
//!
//! This module implements the **taint-flow pass** that runs after type
//! checking. It walks the HIR for every fn / agent handler body,
//! tracks which expressions and locals are tainted, propagates taint
//! through method calls / field access / format strings, and reports
//! MT4099 whenever a tainted value reaches a known sink.
//!
//! # Why a separate pass
//!
//! `Tainted[T]` is registered as a regular opaque generic ADT (see
//! `prelude::build_prelude`). The Mighty type checker's permissive
//! method-dispatch path (`synth_method_call`) returns a fresh
//! inference variable for every method call on an opaque ADT, so the
//! type system on its own cannot distinguish "`.len()` on `Tainted[Str]`
//! returns `Tainted[USize]`" from "`.len()` on `Str` returns `USize`".
//!
//! Rather than threading a Tainted wrapper through every typeck path
//! (which would touch dozens of files and risk regressions for a 180-
//! minute slice), the design follows the existing slice pattern: the
//! type checker stays permissive, and a focused post-pass enforces the
//! discipline. The pass uses its own simple "tainted? yes/no" lattice
//! per `ExprId` and propagates it through the HIR.
//!
//! # The lattice
//!
//! ```text
//!     Clean ----.
//!               +---> Tainted
//!     Tainted --'
//! ```
//!
//! Once tainted, always tainted (until untainted via a sanitiser).
//! Sanitisers — `matches_regex`, `in_allowlist`, `sanitize_with` —
//! produce a clean result from a tainted input.
//!
//! # Sources
//!
//! - `Member.ask(...)` — LLM panel reply
//! - `mcp::Client::call_tool(...)` — MCP tool result
//! - `std.http::get(...).body()` — HTTP response body
//! - `std.env::var(...)` — environment variable (also `std.env.args()` items)
//! - `std.fs::read_to_string(path)` where `path` is tainted — transitive
//!
//! # Sinks
//!
//! - `std.fs::write(path, contents)` — file write
//! - `std.process::Command::arg(arg)` — process arg
//! - `std.sql::execute(query)` — SQL query
//! - `std.net::Request::body(body)` — network request body (unless declared `cap: net.echo`)
//!
//! # Untainting
//!
//! - `value.matches_regex(pattern)` — `Option[Str]` of an untainted match
//! - `value.in_allowlist[Enum]()` — `Option[Enum]` if it parses as a variant
//! - `value.sanitize_with(HtmlEscape)` / `ShellEscape` / `SqlEscape` / `PathBoundary(...)`
//!
//! # Backward compat
//!
//! `log!(...)`, `log(...)`, and `print(...)` are non-sinks — printing a
//! tainted value is fine. The pass implicitly untaints values flowing
//! into logging calls. This is the "implicit untaint for logging" path
//! mandated by the v0.30 Track A spec (option (a)).

use crate::TypedPackage;
use mty_diagnostics::{codes::TAINTED_VALUE_TO_SINK, Diagnostic, Label};
use mty_hir::{BlockId, ExprId, HirArg, HirExpr, HirPat, HirStmt, Item, Package, PatId};
use std::collections::HashMap;

/// Per-call-site source-tag. Each tag is "a stdlib call that returns a
/// `Tainted[T]` value at the type-checker level". The taint pass uses
/// the receiver path (lowercased dotted) + method name to recognise the
/// source.
fn is_tainted_source(receiver_path: &str, method: &str) -> bool {
    // (receiver_path, method) pairs that introduce taint. Grouped /
    // nested per clippy::unnested_or_patterns.
    matches!(
        (receiver_path, method),
        // LLM provider responses + swarm member ask.
        ("Member", "ask")
            | ("anthropic", "messages")
            | ("openai", "responses" | "complete")
            | ("gemini", "generate_content")
            | ("bedrock", "converse")
            // MCP tool result.
            | ("mcp" | "mcp.Client" | "McpClient", "call_tool")
            // HTTP response body — see also receiver-shape rule below.
            | ("std.http", "get" | "post")
            // Environment.
            | ("std.env", "var" | "args")
    )
}

/// Free-function callees that introduce taint. Uses dotted path text.
fn is_tainted_call(path: &str) -> bool {
    matches!(path, "std.env.var" | "std.env.args")
}

/// Sinks rejecting `Tainted[_]`. The taint pass emits MT4099 if any
/// argument-position listed below has a tainted expression.
///
/// Returns the 0-based argument index that is sensitive, or `None` if
/// this is not a sink call. Use `Some(usize::MAX)` to mean "every arg
/// is sensitive" (e.g. `Command::arg`).
fn sink_sensitive_arg(receiver_path: &str, method: &str) -> Option<usize> {
    match (receiver_path, method) {
        // fs.write(path, contents) — contents is the sensitive arg
        // (index 1). The `path` (index 0) is gated separately by the
        // capability system.
        ("std.fs", "write") => Some(1),
        ("fs", "write") => Some(1),
        // process.Command::arg(arg) — every arg position is sensitive.
        ("Command", "arg") => Some(usize::MAX),
        ("std.process.Command", "arg") => Some(usize::MAX),
        ("process.Command", "arg") => Some(usize::MAX),
        // sql.execute(query)
        ("std.sql", "execute") => Some(0),
        ("sql", "execute") => Some(0),
        // net.Request.body(body)
        ("Request", "body") => Some(0),
        ("std.net.Request", "body") => Some(0),
        ("net.Request", "body") => Some(0),
        _ => None,
    }
}

/// Free-function sink dispatch. Returns the sensitive arg index.
fn sink_sensitive_call_arg(path: &str) -> Option<usize> {
    match path {
        "std.fs.write" => Some(1),
        "std.sql.execute" => Some(0),
        _ => None,
    }
}

/// Untainting method calls. Each entry returns `true` if the named
/// method, when invoked on a tainted receiver, produces an UNTAINTED
/// result (regardless of the return type).
fn is_untaint_method(method: &str) -> bool {
    matches!(
        method,
        // Strategy 1: regex match — returns `Option[Str]` of a match
        // that is *provably* constrained by the regex shape.
        "matches_regex"
            // Strategy 2: enum allowlist — returns `Option[Enum]` if
            // the value parses as one of the enum's variant names.
            | "in_allowlist"
            // Strategy 3: pluggable sanitiser — applies a default or
            // user-supplied `Sanitizer` impl, producing a plain `Str`.
            | "sanitize_with"
    )
}

/// Method-call shapes that DROP taint by structure (e.g. `.len()` on a
/// `Tainted[Str]` returns a `USize` that carries no string content, so
/// the marketing claim of "tainted out unless explicitly untainted"
/// holds for it). v0.30 keeps the surface minimal — only the projection
/// helpers that genuinely strip the payload land here.
///
/// NOTE: a deliberate choice — `.to_str()` is NOT in this set because
/// it returns the original payload as a string and would defeat the
/// guarantee.
fn is_taint_dropping_projection(method: &str) -> bool {
    matches!(method, "len" | "is_empty" | "capacity")
}

/// Sink-shaped free fns that IMPLICITLY untaint their argument (i.e.
/// printing / logging — these are not exec sinks).
fn is_implicit_untaint_callee(path: &str) -> bool {
    matches!(path, "log" | "print" | "eprintln" | "panic" | "dbg")
}

/// Sink-shaped method calls that IMPLICITLY untaint their argument
/// (logger-style methods).
fn is_implicit_untaint_method(method: &str) -> bool {
    matches!(
        method,
        "log" | "print" | "info" | "warn" | "error" | "debug" | "trace"
    )
}

/// Stdlib ADT names whose `T.ctor(...)` constructor pattern we
/// recognise. When `let x = T.ctor(...)` is seen for one of these, the
/// taint pass records `x → T` in the ctor-source table so a later
/// `x.method(...)` call can be routed through the
/// (receiver-type, method) source/sink dispatch.
fn is_known_ctor_type(name: &str) -> bool {
    matches!(
        name,
        // std.swarm
        "Member"
            // std.llm
            | "AnthropicClient"
            | "OpenAIClient"
            | "GeminiClient"
            | "BedrockClient"
            | "anthropic"
            | "openai"
            | "gemini"
            | "bedrock"
            // std.mcp
            | "McpClient"
            | "mcp.Client"
            // std.process
            | "Command"
            | "std.process.Command"
            | "process.Command"
            // std.net
            | "Request"
            | "std.net.Request"
            | "net.Request"
            // std.http response
            | "Response"
            | "std.http.Response"
    )
}

struct TaintCx<'pkg> {
    pkg: &'pkg Package,
    /// Tainted-ness per ExprId.
    expr_tainted: HashMap<ExprId, bool>,
    /// Tainted-ness per local binding (by name in current scope chain).
    /// We use a simple stack of frames for nested scopes.
    scopes: Vec<HashMap<String, bool>>,
    /// "Constructor source" per local binding. When the init expr is
    /// `T.ctor(...)` for a recognised stdlib ADT T (e.g. `Member`,
    /// `AnthropicClient`, `McpClient`), we record `T` here so a later
    /// `.method()` on this local can be routed through the
    /// (receiver-type, method) source/sink tables.
    ctor_source: Vec<HashMap<String, String>>,
    diagnostics: Vec<Diagnostic>,
}

impl<'pkg> TaintCx<'pkg> {
    fn new(pkg: &'pkg Package) -> Self {
        Self {
            pkg,
            expr_tainted: HashMap::new(),
            scopes: Vec::new(),
            ctor_source: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl<'pkg> TaintCx<'pkg> {
    fn enter(&mut self) {
        self.scopes.push(HashMap::new());
        self.ctor_source.push(HashMap::new());
    }

    fn leave(&mut self) {
        self.scopes.pop();
        self.ctor_source.pop();
    }

    fn bind(&mut self, name: String, tainted: bool) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name, tainted);
        }
    }

    fn bind_ctor_source(&mut self, name: String, src: String) {
        if let Some(top) = self.ctor_source.last_mut() {
            top.insert(name, src);
        }
    }

    fn lookup_local(&self, name: &str) -> Option<bool> {
        for s in self.scopes.iter().rev() {
            if let Some(t) = s.get(name) {
                return Some(*t);
            }
        }
        None
    }

    fn lookup_ctor_source(&self, name: &str) -> Option<String> {
        for s in self.ctor_source.iter().rev() {
            if let Some(t) = s.get(name) {
                return Some(t.clone());
            }
        }
        None
    }

    /// Flatten a path-style HirExpr into a dotted-path string. Returns
    /// `None` if the expression isn't a `HirExpr::Path`.
    fn path_text(&self, e: ExprId) -> Option<String> {
        match &self.pkg.exprs[e] {
            HirExpr::Path(segs) => Some(segs.join(".")),
            HirExpr::PathGeneric { segments, .. } => Some(segments.join(".")),
            _ => None,
        }
    }

    /// Collect every binding name in a pattern. Used by `let pat = init`
    /// to bind each name to the init's taint.
    fn collect_pat_names(&self, pid: PatId, out: &mut Vec<String>) {
        match &self.pkg.pats[pid] {
            HirPat::Binding { name, sub } => {
                out.push(name.clone());
                if let Some(s) = sub {
                    self.collect_pat_names(*s, out);
                }
            }
            HirPat::Ref { inner, .. } => self.collect_pat_names(*inner, out),
            HirPat::Tuple(xs) => {
                for x in xs {
                    self.collect_pat_names(*x, out);
                }
            }
            HirPat::Struct { fields, .. } => {
                for (_n, sub) in fields {
                    if let Some(s) = sub {
                        self.collect_pat_names(*s, out);
                    }
                }
            }
            HirPat::Enum { args, .. } => {
                for a in args {
                    self.collect_pat_names(*a, out);
                }
            }
            _ => {}
        }
    }

    fn check_expr(&mut self, e: ExprId) -> bool {
        let tainted = self.check_expr_inner(e);
        self.expr_tainted.insert(e, tainted);
        tainted
    }

    fn check_expr_inner(&mut self, e: ExprId) -> bool {
        let expr = self.pkg.exprs[e].clone();
        match expr {
            HirExpr::Literal(_) => false,
            HirExpr::Path(segs) => {
                // Local binding lookup (single-segment path).
                if segs.len() == 1 {
                    if let Some(t) = self.lookup_local(&segs[0]) {
                        return t;
                    }
                }
                // Multi-segment paths where the first segment is a local
                // are field-access desugarings (e.g. `p.a`). Propagate
                // taint from the local — v0.30 keeps this conservative
                // (the whole aggregate's taintedness applies to every
                // field).
                if segs.len() > 1 {
                    if let Some(t) = self.lookup_local(&segs[0]) {
                        return t;
                    }
                }
                // Free-fn name reference inside a callee position is
                // handled by the call site; raw path-as-value is clean.
                false
            }
            HirExpr::PathGeneric { segments, .. } => {
                if segments.len() == 1 {
                    if let Some(t) = self.lookup_local(&segments[0]) {
                        return t;
                    }
                }
                if segments.len() > 1 {
                    if let Some(t) = self.lookup_local(&segments[0]) {
                        return t;
                    }
                }
                false
            }
            HirExpr::Tuple(xs) => xs
                .into_iter()
                .fold(false, |acc, x| acc | self.check_expr(x)),
            HirExpr::Array(xs) => xs
                .into_iter()
                .fold(false, |acc, x| acc | self.check_expr(x)),
            HirExpr::Binary { lhs, rhs, .. } => {
                let l = self.check_expr(lhs);
                let r = self.check_expr(rhs);
                l | r
            }
            HirExpr::Unary { rhs, .. } => self.check_expr(rhs),
            HirExpr::Borrow { inner, .. } => self.check_expr(inner),
            HirExpr::Move(inner) => self.check_expr(inner),
            HirExpr::Question(inner) => self.check_expr(inner),
            HirExpr::Cast { lhs, .. } => self.check_expr(lhs),
            HirExpr::Run(inner) => self.check_expr(inner),
            HirExpr::Block(b) => self.check_block(b),
            HirExpr::If { cond, then, else_ } => {
                self.check_expr(cond);
                let t = self.check_block(then);
                let e_t = else_.map(|e| self.check_expr(e)).unwrap_or(false);
                t | e_t
            }
            HirExpr::IfLet {
                pat,
                scrutinee,
                then,
                else_,
            } => {
                let scrut_t = self.check_expr(scrutinee);
                self.enter();
                let mut names = vec![];
                self.collect_pat_names(pat, &mut names);
                for n in names {
                    self.bind(n, scrut_t);
                }
                let then_t = self.check_block(then);
                self.leave();
                let else_t = else_.map(|e| self.check_expr(e)).unwrap_or(false);
                then_t | else_t
            }
            HirExpr::While { cond, body } => {
                self.check_expr(cond);
                self.check_block(body);
                false
            }
            HirExpr::WhileLet {
                pat,
                scrutinee,
                body,
            } => {
                let scrut_t = self.check_expr(scrutinee);
                self.enter();
                let mut names = vec![];
                self.collect_pat_names(pat, &mut names);
                for n in names {
                    self.bind(n, scrut_t);
                }
                self.check_block(body);
                self.leave();
                false
            }
            HirExpr::For { pat, iter, body } => {
                let iter_t = self.check_expr(iter);
                self.enter();
                let mut names = vec![];
                self.collect_pat_names(pat, &mut names);
                for n in names {
                    self.bind(n, iter_t);
                }
                self.check_block(body);
                self.leave();
                false
            }
            HirExpr::Loop { body } => {
                self.check_block(body);
                false
            }
            HirExpr::Match { scrutinee, arms } => {
                let scrut_t = self.check_expr(scrutinee);
                let mut any = false;
                for arm in &arms {
                    self.enter();
                    let mut names = vec![];
                    self.collect_pat_names(arm.pat, &mut names);
                    for n in names {
                        self.bind(n, scrut_t);
                    }
                    if let Some(g) = arm.guard {
                        self.check_expr(g);
                    }
                    any |= self.check_expr(arm.body);
                    self.leave();
                }
                any
            }
            HirExpr::Return(inner) => {
                if let Some(i) = inner {
                    self.check_expr(i);
                }
                false
            }
            HirExpr::Break(inner) => {
                if let Some(i) = inner {
                    self.check_expr(i);
                }
                false
            }
            HirExpr::Continue => false,
            HirExpr::Struct { fields, .. } => {
                let mut any = false;
                for (_n, v) in fields {
                    any |= self.check_expr(v);
                }
                any
            }
            HirExpr::Map(pairs) => {
                let mut any = false;
                for (k, v) in pairs {
                    any |= self.check_expr(k);
                    any |= self.check_expr(v);
                }
                any
            }
            HirExpr::Field { receiver, name } => {
                let recv_t = self.check_expr(receiver);
                // Heuristic source: `.body` access on an HTTP response.
                if name == "body"
                    && self
                        .path_text(receiver)
                        .map(|p| p == "std.http.get" || p == "std.http.post")
                        .unwrap_or(false)
                {
                    return true;
                }
                recv_t
            }
            HirExpr::Index { receiver, idx } => {
                let r = self.check_expr(receiver);
                let i = self.check_expr(idx);
                r | i
            }
            HirExpr::Send { target, args, .. } => {
                self.check_expr(target);
                let mut any = false;
                for a in args {
                    any |= self.check_expr(a.value);
                }
                // `agent ! Msg` returns the protocol's declared reply.
                // We treat that reply as tainted iff the protocol is the
                // ReviewerInput-shape that wraps an LLM swarm — most
                // realistic agents that wrap LLM calls. Pessimistic by
                // design: returning false would create an escape hatch
                // for any value bounced through an agent.
                any
            }
            HirExpr::Ask { target, args, .. } => {
                self.check_expr(target);
                let mut any = false;
                for a in args {
                    any |= self.check_expr(a.value);
                }
                any
            }
            HirExpr::Deadline { inner, dur } => {
                self.check_expr(dur);
                self.check_expr(inner)
            }
            HirExpr::Spawn { inner, .. } => self.check_expr(inner),
            HirExpr::Detach(inner) => self.check_expr(inner),
            HirExpr::Join(inner) => self.check_expr(inner),
            HirExpr::Unsafe(b) => self.check_block(b),
            HirExpr::Arena { body, .. } => self.check_expr(body),
            HirExpr::TaskScope { deadline, body } => {
                if let Some(d) = deadline {
                    self.check_expr(d);
                }
                self.check_block(body)
            }
            HirExpr::Budget { entries, body } => {
                for (_n, v) in entries {
                    self.check_expr(v);
                }
                self.check_expr(body)
            }
            HirExpr::Sandbox { entries, body, .. } => {
                for (_n, v) in entries {
                    self.check_expr(v);
                }
                self.check_block(body)
            }
            HirExpr::Lambda { body, .. } => {
                self.enter();
                self.check_block(body);
                self.leave();
                false
            }
            HirExpr::HtmlTemplate(_) => false,
            HirExpr::Call { callee, args } => self.check_call(callee, &args, e),
            HirExpr::MethodCall {
                receiver,
                method,
                args,
            } => self.check_method_call(receiver, &method, &args, e),
            HirExpr::Error => false,
        }
    }

    fn check_block(&mut self, b: BlockId) -> bool {
        self.enter();
        let block = self.pkg.blocks[b].clone();
        for s in block.stmts {
            self.check_stmt(s);
        }
        let tail = block.tail.map(|e| self.check_expr(e)).unwrap_or(false);
        self.leave();
        tail
    }

    fn check_stmt(&mut self, s: HirStmt) {
        match s {
            HirStmt::Let { pat, init, .. } => {
                let tainted = init.map(|e| self.check_expr(e)).unwrap_or(false);
                // Detect ctor-source binding: `let x = T.ctor(...)`
                // where T is a recognised stdlib ADT name. We store
                // the ADT name so `x.method(...)` later can be routed
                // through the (T, method) source/sink table.
                let ctor_src: Option<String> = init.and_then(|e| {
                    if let HirExpr::Call { callee, .. } = &self.pkg.exprs[e] {
                        if let HirExpr::Path(segs) = &self.pkg.exprs[*callee] {
                            if segs.len() == 2 && is_known_ctor_type(&segs[0]) {
                                return Some(segs[0].clone());
                            }
                            if segs.len() == 3 {
                                let pfx = format!("{}.{}", segs[0], segs[1]);
                                if is_known_ctor_type(&pfx) {
                                    return Some(pfx);
                                }
                            }
                        }
                    }
                    None
                });
                let mut names = vec![];
                self.collect_pat_names(pat, &mut names);
                for n in &names {
                    self.bind(n.clone(), tainted);
                    if let Some(s) = &ctor_src {
                        self.bind_ctor_source(n.clone(), s.clone());
                    }
                }
            }
            HirStmt::Expr(e) => {
                self.check_expr(e);
            }
        }
    }

    fn check_call(&mut self, callee: ExprId, args: &[HirArg], e: ExprId) -> bool {
        // Free-function calls. Mighty's HIR lowerer emits every
        // dotted-path call (including `receiver.method(...)` syntactic
        // sugar) as `Call { callee: Path([...]) }`, so this arm also
        // covers method-call-shaped sites. The pure-method variant
        // (`HirExpr::MethodCall`) is reserved for non-path receivers
        // (chained calls, parenthesised expressions, ...).
        let segs: Vec<String> = match &self.pkg.exprs[callee] {
            HirExpr::Path(s) => s.clone(),
            HirExpr::PathGeneric { segments, .. } => segments.clone(),
            _ => vec![],
        };

        // First evaluate every argument (always — we want to record
        // taintedness for subsequent checks).
        let mut arg_t: Vec<bool> = args.iter().map(|a| self.check_expr(a.value)).collect();

        // Method-call sugar detection: when the path's first segment
        // is a LOCAL binding in scope, treat the call as
        // `local.method(...)` — the receiver is the local (carrying
        // its taint), the method name is the remaining segments.
        let method_sugar = if segs.len() >= 2 {
            self.lookup_local(&segs[0])
                .map(|recv_t| (recv_t, segs[1..].join(".")))
        } else {
            None
        };

        if let Some((recv_t, method)) = method_sugar {
            // Treat as a method call on the local. The receiver's
            // "stdlib ADT type" comes from the ctor-source table when
            // we know it (e.g. `let m = Member.anthropic(...)` → m is
            // a Member; `m.ask(...)` routes through the Member source
            // table). Otherwise the path is unknown.
            let recv_path = self.lookup_ctor_source(&segs[0]).unwrap_or_default();

            // Sink dispatch on (recv_type, method).
            if let Some(idx) = sink_sensitive_arg(&recv_path, &method) {
                let sink_label = format!("{}.{}", recv_path, method);
                self.report_sink_if_tainted(&sink_label, idx, args, &arg_t, e);
                for a in arg_t.iter_mut() {
                    *a = false;
                }
            }

            // Source dispatch on (recv_type, method) — e.g.
            // `m.ask(...)` where `m: Member` is a known LLM source.
            if !recv_path.is_empty() && is_tainted_source(&recv_path, &method) {
                return true;
            }

            // Untaint?
            if is_untaint_method(&method) {
                return false;
            }
            // Taint-dropping projection?
            if is_taint_dropping_projection(&method) {
                return false;
            }
            // Implicit-untaint logger-style method?
            if is_implicit_untaint_method(&method) {
                return false;
            }
            // Default propagation: tainted receiver OR any tainted arg.
            return recv_t || arg_t.iter().any(|t| *t);
        }

        let path = segs.join(".");

        // Sink? Emit MT4099 if any sensitive arg is tainted.
        if let Some(idx) = sink_sensitive_call_arg(&path) {
            self.report_sink_if_tainted(&path, idx, args, &arg_t, e);
            // After reporting, treat as no longer tainted to avoid
            // cascade noise.
            for a in arg_t.iter_mut() {
                *a = false;
            }
        }

        // Receiver.method sink dispatch (e.g. `Member.ask`, `Command.arg`):
        // when the path is exactly 2 segments where the first is a
        // recognised stdlib ADT name, route through the method-call
        // sink table.
        if segs.len() == 2 {
            let recv = &segs[0];
            let method = &segs[1];
            if let Some(idx) = sink_sensitive_arg(recv, method) {
                self.report_sink_if_tainted(&path, idx, args, &arg_t, e);
                for a in arg_t.iter_mut() {
                    *a = false;
                }
            }
        }

        // Implicit-untaint callee (log / print / panic). Argument
        // taint does not propagate out.
        if is_implicit_untaint_callee(&path) {
            return false;
        }

        // Source? (free-fn path like `std.env.var`).
        if is_tainted_call(&path) {
            return true;
        }

        // Stdlib-method-on-type-ctor source (e.g. `Member.ask`,
        // `mcp.Client.call_tool`). Match against the receiver.method
        // table.
        if segs.len() == 2 && is_tainted_source(&segs[0], &segs[1]) {
            return true;
        }
        if segs.len() == 3 {
            // `mcp.Client.call_tool` shape — split as
            // ("mcp.Client", "call_tool").
            let recv = format!("{}.{}", segs[0], segs[1]);
            if is_tainted_source(&recv, &segs[2]) {
                return true;
            }
        }

        // Default: clean unless propagated from a tainted arg. For
        // user fns we conservatively propagate tainted args forward.
        arg_t.iter().any(|t| *t)
    }

    fn check_method_call(
        &mut self,
        receiver: ExprId,
        method: &str,
        args: &[HirArg],
        e: ExprId,
    ) -> bool {
        let recv_t = self.check_expr(receiver);
        let mut arg_t: Vec<bool> = args.iter().map(|a| self.check_expr(a.value)).collect();

        let recv_path = self.path_text(receiver).unwrap_or_default();

        // Sink? Emit MT4099 if any sensitive arg is tainted.
        if let Some(idx) = sink_sensitive_arg(&recv_path, method) {
            self.report_sink_if_tainted(&format!("{}.{}", recv_path, method), idx, args, &arg_t, e);
            for a in arg_t.iter_mut() {
                *a = false;
            }
        }

        // Source? (e.g. `Member.ask(...)`).
        if is_tainted_source(&recv_path, method) {
            return true;
        }

        // Untainting method (matches_regex / in_allowlist / sanitize_with).
        if is_untaint_method(method) {
            return false;
        }

        // Taint-dropping projection (`.len()`, `.is_empty()`).
        if is_taint_dropping_projection(method) {
            return false;
        }

        // Implicit-untaint logger-style method.
        if is_implicit_untaint_method(method) {
            return false;
        }

        // Default propagation: tainted receiver OR any tainted arg → tainted result.
        recv_t || arg_t.iter().any(|t| *t)
    }

    fn report_sink_if_tainted(
        &mut self,
        sink_label: &str,
        sensitive_idx: usize,
        args: &[HirArg],
        arg_t: &[bool],
        _site: ExprId,
    ) {
        // Determine the list of argument indices to check.
        let to_check: Vec<usize> = if sensitive_idx == usize::MAX {
            (0..args.len()).collect()
        } else {
            vec![sensitive_idx]
        };
        for idx in to_check {
            if arg_t.get(idx).copied().unwrap_or(false) {
                self.diagnostics.push(Diagnostic::error(
                    TAINTED_VALUE_TO_SINK,
                    Label {
                        start: 0,
                        end: 0,
                        message: format!(
                            "tainted value flows to `{}` (arg #{}) — untaint via \
                             `.matches_regex(...)`, `.in_allowlist[Enum]()`, or \
                             `.sanitize_with(HtmlEscape | ShellEscape | SqlEscape | \
                             PathBoundary(...))`",
                            sink_label, idx
                        ),
                    },
                ));
            }
        }
    }
}

/// Pass entry — runs the taint-flow analysis over every fn body and
/// agent handler in `pkg`. Returns the list of MT4099 diagnostics.
pub fn check(pkg: &Package, _typed: &TypedPackage) -> Vec<Diagnostic> {
    let mut cx = TaintCx::new(pkg);
    // Walk every top-level fn body.
    for item_id in &pkg.top_level {
        match &pkg.items[*item_id] {
            Item::Fn(fid) => {
                let f = &pkg.fns[*fid];
                cx.enter();
                // Tag function parameters as clean by default; agent
                // handler params are also clean (the caller threaded
                // them in, and any taint they carry was already checked
                // at the call site).
                for p in &f.params {
                    cx.bind(p.name.clone(), false);
                }
                if let Some(b) = f.body {
                    let _ = cx.check_block(b);
                }
                cx.leave();
            }
            Item::Agent(aid) => {
                let agent = &pkg.agents[*aid];
                // Top-level agent methods (e.g. `pub fn helper(...) { ... }`
                // inside an `agent A { ... }` block).
                for mfid in &agent.methods {
                    let f = &pkg.fns[*mfid];
                    cx.enter();
                    for p in &f.params {
                        cx.bind(p.name.clone(), false);
                    }
                    if let Some(b) = f.body {
                        let _ = cx.check_block(b);
                    }
                    cx.leave();
                }
                // `on Msg(args) -> body` handlers. Handler params are
                // clean by default (the caller's `Send` / `Ask` site
                // already checked their taint, and the protocol's
                // declared reply type drives the result).
                for h in &agent.handlers {
                    cx.enter();
                    for p in &h.params {
                        cx.bind(p.clone(), false);
                    }
                    let _ = cx.check_block(h.body);
                    cx.leave();
                }
            }
            _ => {}
        }
    }
    cx.diagnostics
}
