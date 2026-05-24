//! HIR-level macro expansion hook (v0.5 slice).
//!
//! Real macro expansion (token substitution + hygiene) lives in
//! [`mty_macros`]. This module is the *integration* layer: it walks
//! the parsed source, finds every call to a declared macro, runs the
//! expander to produce replacement source, and re-parses the rewritten
//! text. The expanded file is what the rest of HIR lowering sees, so
//! the macro call disappears and the inlined body is lowered as if it
//! had been written by hand.
//!
//! ## v0.5 changes
//!
//!   * Recognizes the new `MACRO_CALL` node (`name!(args)` syntax) in
//!     addition to v0.4's plain `CALL_EXPR` shape. The new node's args
//!     are stored as an opaque `TOKEN_TREE`; we split on commas at
//!     depth 0 to recover individual argument source slices.
//!   * **SD6001** finally fires for `name!(args)` calls whose name is
//!     not in the registry. v0.4 left SD6001 dormant because there was
//!     no syntactic distinction between fn calls and macro calls.
//!   * **SD6005** + **SD6006** fire for procedural macros: SD6005 at
//!     decl time if the body looks impure; SD6006 at call sites
//!     because v0.5 can't execute proc-macro bodies yet.
//!
//! Errors produced here use the SD6xxx band (see
//! `mty_macros::diag`). They are constructed with `DiagCode::new(N)`
//! rather than added to `mty_diagnostics::codes` to keep this slice
//! strictly additive — see `MACROS_V0_5_NOTES.md` for the call.

use mty_ast::{AstNode, File};
use mty_diagnostics::{
    codes::DiagCode,
    diagnostic::{Diagnostic, Label, Severity},
};
use mty_macros::{
    check_proc_macro_purity, expand_to_source, MacroContext, MacroDef, MacroKind, MacroRegistry,
    MAX_EXPANSION_DEPTH,
};
use mty_syntax::{SyntaxKind, SyntaxNode};

/// Output of [`preprocess`]: the (possibly rewritten) source plus any
/// diagnostics produced by the expander.
pub struct Preprocessed {
    pub source: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Iterate `source` to a fixed point or until the recursion cap is
/// hit. Each pass:
///   * parses the source into a CST,
///   * builds a [`MacroRegistry`] from every top-level `macro` decl,
///   * collects every macro call site (the new MACRO_CALL node OR a
///     CALL_EXPR whose callee is a single-segment path matching a
///     registered macro — v0.4 backwards-compat), skipping calls that
///     appear *inside* a macro declaration's body,
///   * also collects every MACRO_CALL whose name is **not** in the
///     registry — those raise SD6001 immediately,
///   * also collects every PROC_MACRO_DECL and checks purity
///     (SD6005), and raises SD6006 for any call to one,
///   * replaces each resolvable call's source span with its expansion,
///     processed right-to-left so byte offsets stay valid,
///   * if no calls were found, returns; otherwise iterates.
pub fn preprocess(source: &str) -> Preprocessed {
    let mut current = source.to_string();
    let mut diags: Vec<Diagnostic> = vec![];
    let mut ctx_counter: MacroContext = 0;

    // SD6005: check every proc-macro decl's purity once, before expansion.
    // This is a static check so we only need to run it on the original
    // source (subsequent expansion passes don't change decls).
    diags.extend(check_proc_macros(source));

    for depth in 0..=MAX_EXPANSION_DEPTH {
        let parsed = mty_syntax::parse(&current);
        let root = SyntaxNode::new_root(parsed.green);
        let Some(file) = File::cast(root) else {
            return Preprocessed {
                source: current,
                diagnostics: diags,
            };
        };
        let registry = MacroRegistry::from_file(&file.0);

        // SD6001 for explicit Name!(args) calls whose name isn't in the
        // registry. This fires even when the registry is empty.
        let unknown = collect_unknown_macro_calls(&file.0, &registry);
        let unknown_was_nonempty = !unknown.is_empty();
        for u in &unknown {
            diags.push(diag_unknown_macro(u.start, u.end, &u.name));
        }

        if registry.is_empty() {
            // Even with no decls we still rewrite unknown-macro call sites to
            // a sentinel so downstream lowering doesn't choke on `Path!(args)`
            // that the post-macro parse won't understand.
            if unknown_was_nonempty {
                rewrite_unknown_to_sentinel(&mut current, &unknown);
            }
            return Preprocessed {
                source: current,
                diagnostics: diags,
            };
        }
        let calls = collect_macro_calls(&file.0, &registry, &current);
        if calls.is_empty() && !unknown_was_nonempty {
            return Preprocessed {
                source: current,
                diagnostics: diags,
            };
        }
        // Stop-on-cap rule: if we've already done MAX iterations *and* there
        // are still calls left, refuse one more round.
        if depth == MAX_EXPANSION_DEPTH && !calls.is_empty() {
            for c in &calls {
                diags.push(diag_recursion_limit(c.start, c.end, &c.name));
            }
            if unknown_was_nonempty {
                rewrite_unknown_to_sentinel(&mut current, &unknown);
            }
            return Preprocessed {
                source: current,
                diagnostics: diags,
            };
        }

        // Apply expansions right-to-left so earlier byte offsets are
        // unaffected. Failed expansions replace the call with an inert
        // sentinel so the next iteration doesn't observe (and re-report)
        // the same site.
        let mut any_progress = false;
        // Merge unknown-macro sentinel writes with successful expansions,
        // sort by offset, apply right-to-left in one sweep.
        #[derive(Clone)]
        enum Rewrite {
            Replace {
                start: usize,
                end: usize,
                with: String,
            },
        }
        let mut rewrites: Vec<Rewrite> = vec![];
        for c in &calls {
            ctx_counter = ctx_counter.wrapping_add(1);
            // Procedural macro: emit SD6006 and replace with sentinel.
            // (Real expansion is v0.6.)
            if c.def.kind == MacroKind::Procedural {
                diags.push(diag_proc_macro_unsupported(c.start, c.end, &c.name));
                rewrites.push(Rewrite::Replace {
                    start: c.start,
                    end: c.end,
                    with: macro_sentinel(),
                });
                any_progress = true;
                continue;
            }
            let arg_refs: Vec<&str> = c.args.iter().map(|s| s.as_str()).collect();
            match expand_to_source(c.def, &arg_refs, ctx_counter) {
                Ok(replacement) => {
                    rewrites.push(Rewrite::Replace {
                        start: c.start,
                        end: c.end,
                        with: replacement,
                    });
                    any_progress = true;
                }
                Err(mty_macros::ExpandError::ArityMismatch { expected, actual }) => {
                    diags.push(diag_arity_mismatch(
                        c.start, c.end, &c.name, expected, actual,
                    ));
                    rewrites.push(Rewrite::Replace {
                        start: c.start,
                        end: c.end,
                        with: macro_sentinel(),
                    });
                    any_progress = true;
                }
                Err(mty_macros::ExpandError::RecursionLimit) => {
                    diags.push(diag_recursion_limit(c.start, c.end, &c.name));
                    rewrites.push(Rewrite::Replace {
                        start: c.start,
                        end: c.end,
                        with: macro_sentinel(),
                    });
                    any_progress = true;
                }
                Err(mty_macros::ExpandError::BadArgumentTokens { index }) => {
                    diags.push(diag_body_parse(
                        c.start,
                        c.end,
                        &format!(
                            "argument #{} to macro `{}` did not lex cleanly",
                            index, c.name
                        ),
                    ));
                    rewrites.push(Rewrite::Replace {
                        start: c.start,
                        end: c.end,
                        with: macro_sentinel(),
                    });
                    any_progress = true;
                }
            }
        }
        for u in &unknown {
            rewrites.push(Rewrite::Replace {
                start: u.start,
                end: u.end,
                with: macro_sentinel(),
            });
            any_progress = true;
        }
        rewrites.sort_by_key(|r| match r {
            Rewrite::Replace { start, .. } => *start,
        });
        for r in rewrites.into_iter().rev() {
            match r {
                Rewrite::Replace { start, end, with } => {
                    current.replace_range(start..end, &with);
                }
            }
        }
        if !any_progress {
            return Preprocessed {
                source: current,
                diagnostics: diags,
            };
        }
    }

    Preprocessed {
        source: current,
        diagnostics: diags,
    }
}

/// Validate that a referenced macro name actually exists. Walks the
/// CST and reports SD6001 for any `name!(args)` call whose name is
/// not in `registry`.
pub fn check_unknown_macros(source: &str) -> Vec<Diagnostic> {
    let p = mty_syntax::parse(source);
    let root = SyntaxNode::new_root(p.green);
    let Some(file) = File::cast(root) else {
        return vec![];
    };
    let registry = MacroRegistry::from_file(&file.0);
    collect_unknown_macro_calls(&file.0, &registry)
        .into_iter()
        .map(|u| diag_unknown_macro(u.start, u.end, &u.name))
        .collect()
}

/// Scan proc-macro decls in `source` and emit SD6005 for any whose body
/// looks impure (effect calls, bare impure surface). Pure proc macros
/// produce no diagnostic here — their call sites raise SD6006 later.
pub fn check_proc_macros(source: &str) -> Vec<Diagnostic> {
    let p = mty_syntax::parse(source);
    let root = SyntaxNode::new_root(p.green);
    let Some(file) = File::cast(root) else {
        return vec![];
    };
    let mut out = vec![];
    for child in file.0.children() {
        if child.kind() != SyntaxKind::PROC_MACRO_DECL {
            continue;
        }
        let Some(def) = mty_macros::registry::lower_proc_macro_decl(&child) else {
            continue;
        };
        if let Some(reason) = check_proc_macro_purity(&def.body) {
            let range = child.text_range();
            out.push(diag_proc_macro_impure(
                usize::from(range.start()),
                usize::from(range.end()),
                &def.name,
                &reason.to_string(),
            ));
        }
    }
    out
}

/// A macro call site located in the source.
struct MacroCallSite<'a> {
    name: String,
    def: &'a MacroDef,
    args: Vec<String>,
    /// Inclusive start byte offset of the call expression in `source`.
    start: usize,
    /// Exclusive end byte offset.
    end: usize,
}

/// A `name!(args)` call site whose name is NOT in the registry.
struct UnknownMacroSite {
    name: String,
    start: usize,
    end: usize,
}

/// Walk the CST and collect every call (MACRO_CALL node *or* CALL_EXPR)
/// whose callee resolves to a registered macro. Skips calls that
/// appear *inside* the body of a MACRO_DECL or PROC_MACRO_DECL —
/// those will be expanded after substitution, on the next
/// preprocessing pass, when they appear in the inlined output.
fn collect_macro_calls<'a>(
    file: &SyntaxNode,
    reg: &'a MacroRegistry,
    source: &str,
) -> Vec<MacroCallSite<'a>> {
    let mut out: Vec<MacroCallSite<'a>> = vec![];
    let mut stack: Vec<SyntaxNode> = vec![file.clone()];
    while let Some(n) = stack.pop() {
        // Don't recurse into macro decl bodies — they're templates.
        if matches!(
            n.kind(),
            SyntaxKind::MACRO_DECL | SyntaxKind::PROC_MACRO_DECL
        ) {
            continue;
        }
        if n.kind() == SyntaxKind::MACRO_CALL {
            if let Some(call) = try_macro_call_node(&n, reg, source) {
                out.push(call);
                // Don't recurse into the call's TOKEN_TREE — nested macro
                // calls inside the arg tree are handled by the next outer
                // pass after substitution.
                continue;
            }
        }
        if n.kind() == SyntaxKind::CALL_EXPR {
            if let Some(call) = try_macro_call(&n, reg, source) {
                out.push(call);
                // Don't recurse into the call's args.
                continue;
            }
        }
        for child in n.children() {
            stack.push(child);
        }
    }
    // Sort by start offset so the right-to-left rewrite order is stable.
    out.sort_by_key(|c| c.start);
    out
}

/// Walk the CST and collect every MACRO_CALL whose name is NOT in
/// the registry — those raise SD6001.
fn collect_unknown_macro_calls(file: &SyntaxNode, reg: &MacroRegistry) -> Vec<UnknownMacroSite> {
    let mut out: Vec<UnknownMacroSite> = vec![];
    let mut stack: Vec<SyntaxNode> = vec![file.clone()];
    while let Some(n) = stack.pop() {
        if matches!(
            n.kind(),
            SyntaxKind::MACRO_DECL | SyntaxKind::PROC_MACRO_DECL
        ) {
            continue;
        }
        if n.kind() == SyntaxKind::MACRO_CALL {
            if let Some(name) = macro_call_name(&n) {
                if reg.get(&name).is_none() {
                    let range = n.text_range();
                    out.push(UnknownMacroSite {
                        name,
                        start: usize::from(range.start()),
                        end: usize::from(range.end()),
                    });
                    continue;
                }
            }
        }
        for child in n.children() {
            stack.push(child);
        }
    }
    out.sort_by_key(|u| u.start);
    out
}

fn rewrite_unknown_to_sentinel(source: &mut String, unknown: &[UnknownMacroSite]) {
    // Right-to-left so byte offsets stay valid.
    let mut sites: Vec<&UnknownMacroSite> = unknown.iter().collect();
    sites.sort_by_key(|u| u.start);
    for u in sites.into_iter().rev() {
        source.replace_range(u.start..u.end, &macro_sentinel());
    }
}

fn try_macro_call_node<'a>(
    call: &SyntaxNode,
    reg: &'a MacroRegistry,
    source: &str,
) -> Option<MacroCallSite<'a>> {
    debug_assert_eq!(call.kind(), SyntaxKind::MACRO_CALL);
    let name = macro_call_name(call)?;
    let def = reg.get(&name)?;
    let args = parse_macro_call_token_tree(call)?;
    let range = call.text_range();
    let start = usize::from(range.start());
    let end = usize::from(range.end());
    debug_assert!(end <= source.len());
    Some(MacroCallSite {
        name,
        def,
        args,
        start,
        end,
    })
}

/// Extract the macro's name from a MACRO_CALL node. The path is the
/// first child (PATH_EXPR); we want its single-segment name.
fn macro_call_name(call: &SyntaxNode) -> Option<String> {
    debug_assert_eq!(call.kind(), SyntaxKind::MACRO_CALL);
    let path_expr = call
        .children()
        .find(|c| c.kind() == SyntaxKind::PATH_EXPR)?;
    callee_single_name(&path_expr)
}

/// Walk a MACRO_CALL's TOKEN_TREE and split on commas at depth 0.
/// Returns one source slice per argument. An empty token tree (just
/// `()`) returns an empty vec; a tree with content but no commas
/// returns a single arg.
fn parse_macro_call_token_tree(call: &SyntaxNode) -> Option<Vec<String>> {
    let tree = call
        .children()
        .find(|c| c.kind() == SyntaxKind::TOKEN_TREE)?;

    // Collect leaf tokens excluding the outer L_PAREN and R_PAREN.
    let mut tokens: Vec<(SyntaxKind, String)> = vec![];
    let mut started = false;
    let mut depth = 0i32;
    for elem in tree.descendants_with_tokens() {
        let Some(t) = elem.into_token() else { continue };
        match t.kind() {
            SyntaxKind::L_PAREN => {
                if !started {
                    started = true;
                    depth = 1;
                    continue;
                }
                depth += 1;
                tokens.push((t.kind(), t.text().to_string()));
            }
            SyntaxKind::R_PAREN => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                tokens.push((t.kind(), t.text().to_string()));
            }
            _ => {
                if started {
                    tokens.push((t.kind(), t.text().to_string()));
                }
            }
        }
    }

    // Now split on COMMA at depth 0 (depth here is paren/brace/bracket
    // nesting *inside* the outer parens we already removed).
    let mut args: Vec<String> = vec![];
    let mut cur = String::new();
    let mut inner_depth = 0i32;
    let mut any_non_trivia = false;
    for (k, text) in &tokens {
        match k {
            SyntaxKind::L_PAREN | SyntaxKind::L_BRACE | SyntaxKind::L_BRACK => {
                inner_depth += 1;
                cur.push_str(text);
                any_non_trivia = true;
            }
            SyntaxKind::R_PAREN | SyntaxKind::R_BRACE | SyntaxKind::R_BRACK => {
                inner_depth -= 1;
                cur.push_str(text);
            }
            SyntaxKind::COMMA if inner_depth == 0 => {
                args.push(cur.trim().to_string());
                cur = String::new();
            }
            _ => {
                if !k.is_trivia() {
                    any_non_trivia = true;
                }
                cur.push_str(text);
            }
        }
    }
    if any_non_trivia || !args.is_empty() {
        args.push(cur.trim().to_string());
    }
    // Drop a trailing empty arg from a trailing comma.
    if args.last().map(|s| s.is_empty()).unwrap_or(false) {
        args.pop();
    }
    Some(args)
}

fn try_macro_call<'a>(
    call: &SyntaxNode,
    reg: &'a MacroRegistry,
    source: &str,
) -> Option<MacroCallSite<'a>> {
    debug_assert_eq!(call.kind(), SyntaxKind::CALL_EXPR);
    // The callee is the first non-ARG_LIST child.
    let callee = call.children().find(|c| c.kind() != SyntaxKind::ARG_LIST)?;
    let name = callee_single_name(&callee)?;
    let def = reg.get(&name)?;
    // Only declarative macros expand via the v0.4 backwards-compat path —
    // procedural macros require explicit `name!(...)` invocation, so we
    // don't silently turn a plain `name(args)` call into a proc-macro
    // expansion. (This avoids breaking existing fn-call call sites.)
    if def.kind != MacroKind::Declarative {
        return None;
    }
    // Collect argument source slices in declaration order.
    let arg_list = call.children().find(|c| c.kind() == SyntaxKind::ARG_LIST)?;
    let mut args: Vec<String> = vec![];
    for child in arg_list.children() {
        match child.kind() {
            SyntaxKind::ARG => {
                // Take the inner expression's text.
                let inner = child.children().next();
                let text = inner.map(|n| n.text().to_string()).unwrap_or_default();
                args.push(text);
            }
            SyntaxKind::NAMED_ARG => {
                // v0.5 still does not support named args in macro calls.
                let inner = child
                    .children()
                    .find(|c| c.kind() != SyntaxKind::NAME)
                    .map(|n| n.text().to_string())
                    .unwrap_or_default();
                args.push(inner);
            }
            _ => {}
        }
    }
    let range = call.text_range();
    let start = usize::from(range.start());
    let end = usize::from(range.end());
    debug_assert!(end <= source.len());
    Some(MacroCallSite {
        name,
        def,
        args,
        start,
        end,
    })
}

/// If `callee` is a `PATH_EXPR` with a single name segment, return the
/// name text. Anything more complex (qualified path, generics, method
/// call) is not eligible to be a macro callee in v0.5.
fn callee_single_name(callee: &SyntaxNode) -> Option<String> {
    if callee.kind() != SyntaxKind::PATH_EXPR {
        return None;
    }
    let segs: Vec<SyntaxNode> = callee
        .descendants()
        .filter(|d| d.kind() == SyntaxKind::PATH_SEGMENT)
        .collect();
    if segs.is_empty() {
        // Fallback: direct NAME_REF child of PATH_EXPR.
        let names: Vec<String> = callee
            .descendants()
            .filter_map(mty_ast::NameRef::cast)
            .map(|n| {
                n.0.first_token()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default()
            })
            .collect();
        if names.len() == 1 {
            return Some(names.into_iter().next().unwrap());
        }
        return None;
    }
    if segs.len() != 1 {
        return None;
    }
    let nm = segs[0].descendants().find_map(mty_ast::NameRef::cast)?;
    let txt = nm.0.first_token()?.text().to_string();
    Some(txt)
}

/// Source replacement used when an expansion failed. Parses as an
/// expression (literal `0`) so the surrounding parse stays well-formed
/// and downstream lowering can keep going to report further errors.
fn macro_sentinel() -> String {
    "0".to_string()
}

fn diag_arity_mismatch(
    start: usize,
    end: usize,
    name: &str,
    expected: usize,
    actual: usize,
) -> Diagnostic {
    Diagnostic {
        code: DiagCode::new(mty_macros::MACRO_ARITY_MISMATCH),
        severity: Severity::Error,
        primary: Label {
            start,
            end,
            message: format!("macro `{name}` expects {expected} argument(s), got {actual}"),
        },
        secondary: vec![],
        notes: vec![],
        helps: vec![],
    }
}

fn diag_recursion_limit(start: usize, end: usize, name: &str) -> Diagnostic {
    Diagnostic {
        code: DiagCode::new(mty_macros::RECURSIVE_MACRO_TOO_DEEP),
        severity: Severity::Error,
        primary: Label {
            start,
            end,
            message: format!("macro `{name}` expanded past depth {MAX_EXPANSION_DEPTH}; aborting"),
        },
        secondary: vec![],
        notes: vec![format!(
            "v0.5 caps declarative-macro recursion at {MAX_EXPANSION_DEPTH} levels"
        )],
        helps: vec![],
    }
}

fn diag_body_parse(start: usize, end: usize, msg: &str) -> Diagnostic {
    Diagnostic {
        code: DiagCode::new(mty_macros::MACRO_BODY_PARSE_FAILED),
        severity: Severity::Error,
        primary: Label {
            start,
            end,
            message: msg.to_string(),
        },
        secondary: vec![],
        notes: vec![],
        helps: vec![],
    }
}

fn diag_unknown_macro(start: usize, end: usize, name: &str) -> Diagnostic {
    Diagnostic {
        code: DiagCode::new(mty_macros::UNKNOWN_MACRO),
        severity: Severity::Error,
        primary: Label {
            start,
            end,
            message: format!("unknown macro `{name}!`"),
        },
        secondary: vec![],
        notes: vec![
            "macro must be declared with `macro Name(...) => {{ ... }}` before use".to_string(),
            "or imported from another package with `use otherpkg.name`".to_string(),
        ],
        helps: vec![],
    }
}

fn diag_proc_macro_unsupported(start: usize, end: usize, name: &str) -> Diagnostic {
    Diagnostic {
        code: DiagCode::new(mty_macros::PROC_MACRO_UNSUPPORTED_V0_5),
        severity: Severity::Error,
        primary: Label {
            start,
            end,
            message: format!(
                "procedural macro `{name}!` cannot be executed in v0.5 (parsed + stored only)"
            ),
        },
        secondary: vec![],
        notes: vec![
            "v0.5 ships proc-macro parsing + storage; execution lands in v0.6".to_string(),
            "the macro declaration is preserved so call-site source can stay stable".to_string(),
        ],
        helps: vec![],
    }
}

fn diag_proc_macro_impure(start: usize, end: usize, name: &str, reason: &str) -> Diagnostic {
    Diagnostic {
        code: DiagCode::new(mty_macros::PROC_MACRO_IMPURE),
        severity: Severity::Error,
        primary: Label {
            start,
            end,
            message: format!("procedural macro `{name}` is not pure: {reason}"),
        },
        secondary: vec![],
        notes: vec![
            "proc macros must be pure functions over TokenStream".to_string(),
            "effects (I/O, time, env, model, rand) are forbidden in proc-macro bodies".to_string(),
        ],
        helps: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lower::LoweringCtx, HirExpr, HirStmt, Item};

    fn lower_src(src: &str) -> (crate::Package, Vec<Diagnostic>) {
        let p = mty_syntax::parse(src);
        let root = SyntaxNode::new_root(p.green);
        let file = File::cast(root).expect("FILE root");
        LoweringCtx::new().lower_file(file)
    }

    #[test]
    fn end_to_end_assert_eq_expands_inline_in_hir() {
        let src = concat!(
            "macro assert_eq(a, b) => { if a != b { panic(\"assert_eq failed\") } }\n",
            "fn main() -> i32 {\n",
            "  assert_eq(1 + 1, 2)\n",
            "  0\n",
            "}\n",
        );
        let (pkg, _diags) = lower_src(src);
        let main_fn_id = pkg
            .top_level
            .iter()
            .find_map(|id| match &pkg.items[*id] {
                Item::Fn(fid) if pkg.fns[*fid].name == "main" => Some(*fid),
                _ => None,
            })
            .expect("main fn");
        let main_fn = &pkg.fns[main_fn_id];
        let body = &pkg.blocks[main_fn.body.expect("body")];
        let first_stmt = body.stmts.first().expect("first stmt");
        let expr_id = match first_stmt {
            HirStmt::Expr(e) => *e,
            HirStmt::Let { .. } => panic!("expected Expr stmt, got Let"),
        };
        match &pkg.exprs[expr_id] {
            HirExpr::If { .. } => {}
            other => panic!(
                "expected HirExpr::If after macro expansion, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn end_to_end_arity_error_surfaces() {
        let src = concat!(
            "macro one(a) => { a + 1 }\n",
            "fn main() -> i32 { one(1, 2); 0 }\n",
        );
        let (_pkg, diags) = lower_src(src);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagCode::new(mty_macros::MACRO_ARITY_MISMATCH)),
            "missing SD6002, got: {:?}",
            diags
        );
    }

    #[test]
    fn no_macros_is_identity() {
        let src = "fn main() -> i32 { 42 }\n";
        let pp = preprocess(src);
        assert_eq!(pp.source, src);
        assert!(pp.diagnostics.is_empty());
    }

    #[test]
    fn assert_eq_call_is_expanded_inline() {
        let src = concat!(
            "macro assert_eq(a, b) => { if a != b { panic(\"x\") } }\n",
            "fn main() -> i32 { assert_eq(1 + 1, 2); 0 }\n",
        );
        let pp = preprocess(src);
        assert!(pp.diagnostics.is_empty(), "diags: {:?}", pp.diagnostics);
        // After expansion, the call site should be replaced with the
        // expanded body. The substituted args are wrapped in parens.
        assert!(
            pp.source.contains("if (1 + 1) != (2)"),
            "expansion missing: {}",
            pp.source
        );
    }

    #[test]
    fn arity_mismatch_emits_sd6002() {
        let src = concat!(
            "macro one(a) => { a + 1 }\n",
            "fn main() -> i32 { one(1, 2); 0 }\n",
        );
        let pp = preprocess(src);
        assert_eq!(pp.diagnostics.len(), 1);
        assert_eq!(
            pp.diagnostics[0].code,
            DiagCode::new(mty_macros::MACRO_ARITY_MISMATCH)
        );
    }

    #[test]
    fn recursive_macro_caps_out_with_sd6004() {
        let src = concat!(
            "macro r(x) => { r(x) + 1 }\n",
            "fn main() -> i32 { r(0); 0 }\n",
        );
        let pp = preprocess(src);
        assert!(
            pp.diagnostics
                .iter()
                .any(|d| d.code == DiagCode::new(mty_macros::RECURSIVE_MACRO_TOO_DEEP)),
            "diags: {:?}",
            pp.diagnostics
        );
    }

    #[test]
    fn nested_macros_expand_inner_then_outer() {
        let src = concat!(
            "macro inc(x) => { x + 1 }\n",
            "macro double(x) => { x + x }\n",
            "fn main() -> i32 { double(inc(2)) }\n",
        );
        let pp = preprocess(src);
        assert!(pp.diagnostics.is_empty(), "diags: {:?}", pp.diagnostics);
        let main_body = pp
            .source
            .split("fn main() -> i32 {")
            .nth(1)
            .expect("split fn main")
            .to_string();
        assert!(
            main_body.contains("(2) + 1"),
            "inner not expanded inside main: {}",
            pp.source
        );
        assert!(
            !main_body.contains("inc("),
            "leftover inc in main: {}",
            pp.source
        );
        assert!(
            !main_body.contains("double("),
            "leftover double in main: {}",
            pp.source
        );
    }

    // v0.5 tests:

    #[test]
    fn bang_call_syntax_expands() {
        let src = concat!(
            "macro assert_eq(a, b) => { if a != b { panic(\"x\") } }\n",
            "fn main() -> i32 { assert_eq!(1 + 1, 2); 0 }\n",
        );
        let pp = preprocess(src);
        assert!(pp.diagnostics.is_empty(), "diags: {:?}", pp.diagnostics);
        assert!(
            pp.source.contains("if (1 + 1) != (2)"),
            "expansion missing: {}",
            pp.source
        );
    }

    #[test]
    fn unknown_bang_macro_emits_sd6001() {
        let src = "fn main() -> i32 { nonexistent!(x); 0 }\n";
        let pp = preprocess(src);
        assert!(
            pp.diagnostics
                .iter()
                .any(|d| d.code == DiagCode::new(mty_macros::UNKNOWN_MACRO)),
            "missing SD6001, diags: {:?}",
            pp.diagnostics
        );
    }

    #[test]
    fn plain_call_to_unknown_does_not_emit_sd6001() {
        // Without the `!` marker, an unknown name is a regular fn call
        // — let normal name resolution handle it. SD6001 fires only
        // for the explicit `name!(...)` shape.
        let src = "fn main() -> i32 { nonexistent(x); 0 }\n";
        let pp = preprocess(src);
        assert!(
            !pp.diagnostics
                .iter()
                .any(|d| d.code == DiagCode::new(mty_macros::UNKNOWN_MACRO)),
            "SD6001 should not fire for plain calls, diags: {:?}",
            pp.diagnostics
        );
    }

    #[test]
    fn proc_macro_call_emits_sd6006() {
        let src = concat!(
            "proc macro id(input: TokenStream) -> TokenStream { input }\n",
            "fn main() -> i32 { id!(42); 0 }\n",
        );
        let pp = preprocess(src);
        assert!(
            pp.diagnostics
                .iter()
                .any(|d| d.code == DiagCode::new(mty_macros::PROC_MACRO_UNSUPPORTED_V0_5)),
            "missing SD6006, diags: {:?}",
            pp.diagnostics
        );
    }

    #[test]
    fn impure_proc_macro_emits_sd6005() {
        let src = concat!(
            "proc macro leak(input: TokenStream) -> TokenStream {\n",
            "  effect.io(\"oops\")\n",
            "  input\n",
            "}\n",
        );
        let pp = preprocess(src);
        assert!(
            pp.diagnostics
                .iter()
                .any(|d| d.code == DiagCode::new(mty_macros::PROC_MACRO_IMPURE)),
            "missing SD6005, diags: {:?}",
            pp.diagnostics
        );
    }
}
