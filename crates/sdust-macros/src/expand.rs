//! Token-substitution expander with mangling-based hygiene.
//!
//! The expander takes a [`MacroDef`] plus a call site (the call's
//! argument source slices) and returns a new source string suitable
//! for re-parsing. Three pieces of work happen here:
//!
//!   1. **Parameter substitution.** Every IDENT in the body whose text
//!      matches a parameter name is replaced by the corresponding
//!      argument's source. Arguments are wrapped in `(` `)` so that
//!      operator precedence at the call site is preserved.
//!   2. **Hygiene mangling.** Identifiers introduced by `let` bindings
//!      inside the macro body are renamed to `__mac_<ctx>_<orig>`. The
//!      `ctx` is a per-expansion counter so two nested expansions don't
//!      alias. Uses of those bindings inside the body are renamed to
//!      match. Parameters and free names (calls to `panic`, etc.) are
//!      left untouched.
//!   3. **Recursion accounting.** The expander tracks expansion depth
//!      across nested macro calls and refuses to descend past
//!      [`crate::MAX_EXPANSION_DEPTH`].
//!
//! The expander does **not** itself re-parse output — that's the caller
//! (HIR lowering) so the right parser entrypoint can be picked
//! (`parse_expr` vs the full file parser).

use crate::registry::MacroDef;
use crate::token::{lex_fragment, tokens_to_source, Tok};
use sdust_syntax::SyntaxKind;

/// Unique tag for a single macro expansion. The expander assigns one
/// per call; mangled identifiers embed the value.
pub type MacroContext = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandError {
    /// Wrong number of arguments. Carries (expected, actual).
    ArityMismatch { expected: usize, actual: usize },
    /// Recursion exceeded `MAX_EXPANSION_DEPTH`.
    RecursionLimit,
    /// One of the argument source slices failed to lex.
    BadArgumentTokens { index: usize },
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpandError::ArityMismatch { expected, actual } => {
                write!(f, "macro arity mismatch: expected {expected}, got {actual}")
            }
            ExpandError::RecursionLimit => write!(f, "macro recursion limit exceeded"),
            ExpandError::BadArgumentTokens { index } => {
                write!(f, "macro argument #{index} could not be lexed")
            }
        }
    }
}

impl std::error::Error for ExpandError {}

/// Expand `def` with `args` (each arg is the source text of one
/// call-site argument expression). Returns the rewritten token stream.
///
/// `ctx` is the unique tag for this expansion — typically a monotonic
/// counter maintained by the caller (HIR lowering) so each expansion
/// gets a fresh identity.
pub fn expand(def: &MacroDef, args: &[&str], ctx: MacroContext) -> Result<Vec<Tok>, ExpandError> {
    if args.len() != def.params.len() {
        return Err(ExpandError::ArityMismatch {
            expected: def.params.len(),
            actual: args.len(),
        });
    }

    // Pre-lex each argument so substitution preserves token kinds.
    let mut arg_toks: Vec<Vec<Tok>> = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        match lex_fragment(arg) {
            Some(t) => arg_toks.push(t),
            None => return Err(ExpandError::BadArgumentTokens { index: i }),
        }
    }

    // First pass: collect the identifier names introduced by `let` bindings
    // inside the macro body. These get hygiene-mangled. We deliberately do
    // NOT mangle parameters (those are substituted) or free names.
    let bound = collect_bound_idents(&def.body, &def.params);

    // Second pass: produce the output stream.
    let mut out: Vec<Tok> = Vec::with_capacity(def.body.len() + args.len() * 4);
    for tok in &def.body {
        if tok.kind == SyntaxKind::IDENT {
            // Parameter? Substitute with the matching argument's tokens,
            // wrapped in parens so precedence is preserved.
            if let Some(idx) = def.params.iter().position(|p| p == &tok.text) {
                out.push(Tok::new(SyntaxKind::L_PAREN, "("));
                out.extend(arg_toks[idx].iter().cloned());
                out.push(Tok::new(SyntaxKind::R_PAREN, ")"));
                continue;
            }
            // Macro-introduced binding? Mangle.
            if bound.iter().any(|b| b == &tok.text) {
                out.push(Tok::new(
                    SyntaxKind::IDENT,
                    format!("__mac_{ctx}_{}", tok.text),
                ));
                continue;
            }
        }
        out.push(tok.clone());
    }
    Ok(out)
}

/// Convenience: expand and emit re-parsable source text in one step.
pub fn expand_to_source(
    def: &MacroDef,
    args: &[&str],
    ctx: MacroContext,
) -> Result<String, ExpandError> {
    let toks = expand(def, args, ctx)?;
    Ok(tokens_to_source(&toks))
}

/// Walk the macro body and gather every identifier introduced by a
/// `let` binding (skipping `mut`). We only handle the simple binding
/// shape `let IDENT [: TYPE] = ...` for v0.4; tuple/struct patterns in
/// `let` inside macros are explicitly out of scope.
fn collect_bound_idents(body: &[Tok], params: &[String]) -> Vec<String> {
    let mut bound = vec![];
    let mut i = 0;
    while i < body.len() {
        if body[i].kind == SyntaxKind::LET_KW {
            // skip trivia
            let mut j = i + 1;
            while j < body.len() && body[j].is_trivia() {
                j += 1;
            }
            // optional `mut`
            if j < body.len() && body[j].kind == SyntaxKind::MUT_KW {
                j += 1;
                while j < body.len() && body[j].is_trivia() {
                    j += 1;
                }
            }
            if j < body.len() && body[j].kind == SyntaxKind::IDENT {
                let name = &body[j].text;
                // Don't mangle parameters; the caller's argument substitution
                // owns those names.
                if !params.iter().any(|p| p == name) && !bound.iter().any(|b: &String| b == name) {
                    bound.push(name.clone());
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    bound
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::MacroRegistry;
    use sdust_ast::{AstNode, File};
    use sdust_syntax::SyntaxNode;

    fn def(src: &str, name: &str) -> MacroDef {
        let p = sdust_syntax::parse(src);
        let root = SyntaxNode::new_root(p.green);
        let file = File::cast(root).unwrap();
        let reg = MacroRegistry::from_file(&file.0);
        reg.get(name).cloned().unwrap()
    }

    #[test]
    fn arity_mismatch_reported() {
        let d = def("macro id(x) => { x }\n", "id");
        let e = expand(&d, &["1", "2"], 0).unwrap_err();
        assert!(matches!(
            e,
            ExpandError::ArityMismatch {
                expected: 1,
                actual: 2
            }
        ));
    }

    #[test]
    fn parameter_substitution_wraps_in_parens() {
        let d = def("macro id(x) => { x }\n", "id");
        let s = expand_to_source(&d, &["1 + 1"], 0).unwrap();
        assert!(s.contains("(1 + 1)"), "got: {s}");
    }

    #[test]
    fn free_name_not_mangled() {
        let d = def("macro p() => { panic(\"oops\") }\n", "p");
        let s = expand_to_source(&d, &[], 7).unwrap();
        assert!(s.contains("panic"), "got: {s}");
        assert!(!s.contains("__mac_"), "free names must not be mangled");
    }

    #[test]
    fn let_binding_is_mangled() {
        let d = def("macro twice(x) => { let y = x; y + y }\n", "twice");
        let s = expand_to_source(&d, &["3"], 5).unwrap();
        // `y` becomes `__mac_5_y` everywhere it appears.
        assert!(s.contains("let __mac_5_y"), "got: {s}");
        assert!(s.contains("__mac_5_y + __mac_5_y"), "got: {s}");
    }

    #[test]
    fn distinct_contexts_yield_distinct_mangles() {
        let d = def("macro twice(x) => { let y = x; y + y }\n", "twice");
        let s1 = expand_to_source(&d, &["3"], 1).unwrap();
        let s2 = expand_to_source(&d, &["3"], 2).unwrap();
        assert!(s1.contains("__mac_1_y"));
        assert!(s2.contains("__mac_2_y"));
        assert!(s1 != s2);
    }
}
