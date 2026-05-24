//! HIR-level macro expansion hook (v0.4 slice).
//!
//! Real macro expansion (token substitution + hygiene) lives in
//! [`sdust_macros`]. This module is the *integration* layer: it walks
//! the parsed source, finds every call to a declared macro, runs the
//! expander to produce replacement source, and re-parses the rewritten
//! text. The expanded file is what the rest of HIR lowering sees, so
//! the macro call disappears and the inlined body is lowered as if it
//! had been written by hand.
//!
//! Errors produced here use the SD6xxx band (see
//! `sdust_macros::diag`). They are constructed with `DiagCode::new(N)`
//! rather than added to `sdust_diagnostics::codes` to keep this slice
//! strictly additive — see `MACROS_V0_4_NOTES.md` for the call.

use sdust_ast::{AstNode, File};
use sdust_diagnostics::{
    codes::DiagCode,
    diagnostic::{Diagnostic, Label, Severity},
};
use sdust_macros::{expand_to_source, MacroContext, MacroDef, MacroRegistry, MAX_EXPANSION_DEPTH};
use sdust_syntax::{SyntaxKind, SyntaxNode};

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
///   * collects every `CALL_EXPR` whose callee is a single-segment
///     path matching a registered macro (skipping calls that appear
///     *inside* a macro declaration's body),
///   * replaces each such call's source span with its expansion,
///     processed right-to-left so byte offsets stay valid,
///   * if no calls were found, returns; otherwise iterates.
///
/// The pass is conservative: any expander error (arity mismatch, bad
/// argument tokens, depth blow-up) yields a Diagnostic and the
/// originating call is left in place so downstream lowering can still
/// proceed on the surrounding source.
pub fn preprocess(source: &str) -> Preprocessed {
    let mut current = source.to_string();
    let mut diags: Vec<Diagnostic> = vec![];
    let mut ctx_counter: MacroContext = 0;

    for depth in 0..=MAX_EXPANSION_DEPTH {
        let parsed = sdust_syntax::parse(&current);
        let root = SyntaxNode::new_root(parsed.green);
        let Some(file) = File::cast(root) else {
            return Preprocessed {
                source: current,
                diagnostics: diags,
            };
        };
        let registry = MacroRegistry::from_file(&file.0);
        if registry.is_empty() {
            return Preprocessed {
                source: current,
                diagnostics: diags,
            };
        }
        let calls = collect_macro_calls(&file.0, &registry, &current);
        if calls.is_empty() {
            return Preprocessed {
                source: current,
                diagnostics: diags,
            };
        }
        if depth == MAX_EXPANSION_DEPTH {
            // We've already done MAX_EXPANSION_DEPTH passes; one more
            // round of macros remains — refuse to expand further.
            for c in &calls {
                diags.push(diag_recursion_limit(c.start, c.end, &c.name));
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
        for c in calls.iter().rev() {
            ctx_counter = ctx_counter.wrapping_add(1);
            let arg_refs: Vec<&str> = c.args.iter().map(|s| s.as_str()).collect();
            match expand_to_source(c.def, &arg_refs, ctx_counter) {
                Ok(replacement) => {
                    current.replace_range(c.start..c.end, &replacement);
                    any_progress = true;
                }
                Err(sdust_macros::ExpandError::ArityMismatch { expected, actual }) => {
                    diags.push(diag_arity_mismatch(
                        c.start, c.end, &c.name, expected, actual,
                    ));
                    current.replace_range(c.start..c.end, &macro_sentinel());
                    any_progress = true;
                }
                Err(sdust_macros::ExpandError::RecursionLimit) => {
                    diags.push(diag_recursion_limit(c.start, c.end, &c.name));
                    current.replace_range(c.start..c.end, &macro_sentinel());
                    any_progress = true;
                }
                Err(sdust_macros::ExpandError::BadArgumentTokens { index }) => {
                    diags.push(diag_body_parse(
                        c.start,
                        c.end,
                        &format!(
                            "argument #{} to macro `{}` did not lex cleanly",
                            index, c.name
                        ),
                    ));
                    current.replace_range(c.start..c.end, &macro_sentinel());
                    any_progress = true;
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
/// already-expanded CST and reports SD6001 for any call whose callee
/// path matches `macro <name>` syntax but the registry has no entry.
///
/// In v0.4 callers (HIR lowering) cannot distinguish a macro from a
/// regular function call at the source level — Stardust has no `name!`
/// punctuation. So we only check the registry membership when a call
/// site uses a name that *would have been* expanded had it been
/// declared. The realistic detection target is the post-expansion
/// pass: if the registry contains `foo` and we somehow still see a
/// call to `foo(...)`, the expander must have refused — and the
/// matching diagnostic was already emitted above. SD6001 itself is
/// reserved for future strict-mode lookups (`mac!name(...)` syntax)
/// planned in v0.5.
pub fn check_unknown_macros(_source: &str) -> Vec<Diagnostic> {
    // v0.4: no syntactic marker for macro calls; SD6001 is reserved.
    vec![]
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

/// Walk the CST and collect every CALL_EXPR whose callee resolves to
/// a registered macro. Skips calls that appear *inside* the body of a
/// MACRO_DECL — those will be expanded after substitution, on the
/// next preprocessing pass, when they appear in the inlined output.
fn collect_macro_calls<'a>(
    file: &SyntaxNode,
    reg: &'a MacroRegistry,
    source: &str,
) -> Vec<MacroCallSite<'a>> {
    let mut out: Vec<MacroCallSite<'a>> = vec![];
    let mut stack: Vec<SyntaxNode> = vec![file.clone()];
    while let Some(n) = stack.pop() {
        // Don't recurse into macro decl bodies — they're templates.
        if n.kind() == SyntaxKind::MACRO_DECL {
            continue;
        }
        if n.kind() == SyntaxKind::CALL_EXPR {
            if let Some(call) = try_macro_call(&n, reg, source) {
                out.push(call);
                // Don't recurse into the call's args — nested macro
                // calls are handled by the next outer pass after
                // substitution.
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
                // v0.4 does not support named args in macro calls.
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
/// call) is not eligible to be a macro callee in v0.4.
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
            .filter_map(sdust_ast::NameRef::cast)
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
    let nm = segs[0].descendants().find_map(sdust_ast::NameRef::cast)?;
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
        code: DiagCode::new(sdust_macros::MACRO_ARITY_MISMATCH),
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
        code: DiagCode::new(sdust_macros::RECURSIVE_MACRO_TOO_DEEP),
        severity: Severity::Error,
        primary: Label {
            start,
            end,
            message: format!("macro `{name}` expanded past depth {MAX_EXPANSION_DEPTH}; aborting"),
        },
        secondary: vec![],
        notes: vec![format!(
            "v0.4 caps declarative-macro recursion at {MAX_EXPANSION_DEPTH} levels"
        )],
        helps: vec![],
    }
}

fn diag_body_parse(start: usize, end: usize, msg: &str) -> Diagnostic {
    Diagnostic {
        code: DiagCode::new(sdust_macros::MACRO_BODY_PARSE_FAILED),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lower::LoweringCtx, HirExpr, HirStmt, Item};

    fn lower_src(src: &str) -> (crate::Package, Vec<Diagnostic>) {
        let p = sdust_syntax::parse(src);
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
                .any(|d| d.code == DiagCode::new(sdust_macros::MACRO_ARITY_MISMATCH)),
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
            DiagCode::new(sdust_macros::MACRO_ARITY_MISMATCH)
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
                .any(|d| d.code == DiagCode::new(sdust_macros::RECURSIVE_MACRO_TOO_DEEP)),
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
        // After both expansions, the main fn body contains both inner
        // pieces. The macro decls themselves still mention the names
        // — only the call site should change.
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
}
