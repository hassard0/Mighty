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
//!      inside the macro body are renamed to `__mac_<intro>_<orig>`,
//!      where `<intro>` is the fresh [`crate::scopes::ScopeId`] minted
//!      for the call. Two nested expansions get distinct IDs off the
//!      shared [`crate::scopes::ScopeGen`] so the mangled names never
//!      alias. v0.5 extends this to tuple patterns `let (a, b) = ...`,
//!      struct patterns `let User { id, name } = ...`, ref patterns
//!      `let &x = ...`, and binding patterns. Parameters and free names
//!      (calls to `panic`, etc.) are left untouched.
//!   3. **Recursion accounting.** The expander tracks expansion depth
//!      across nested macro calls and refuses to descend past
//!      [`crate::MAX_EXPANSION_DEPTH`].
//!
//! The expander does **not** itself re-parse output — that's the caller
//! (HIR lowering) so the right parser entrypoint can be picked
//! (`parse_expr` vs the full file parser).

use crate::hygiene::{HygieneEnv, ScopedTok};
use crate::registry::MacroDef;
use crate::scopes::{ScopeGen, ScopeId, Scopes};
use crate::token::{lex_fragment, tokens_to_source, Tok};
use mty_syntax::SyntaxKind;

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

/// Walk the macro body and gather every identifier introduced by a
/// `let` binding. v0.5 supports the following pattern shapes:
///
///   * `let IDENT = ...`               — simple binding (v0.4 baseline)
///   * `let mut IDENT = ...`           — simple, mutable
///   * `let (a, b, ...) = ...`         — tuple pattern
///   * `let Path { a, b: c, .. } = …`  — struct pattern
///   * `let &x = ...` / `let &mut x`   — ref pattern
///   * `let ref x = ...`               — ref-binding pattern
///   * `let x @ pat = ...`             — binding pattern
///
/// Pattern recognition is lexical (we don't have a parsed body AST).
/// Each `let` token starts a pattern extent that runs until the next
/// `=` token; every IDENT inside that extent (modulo type annotations
/// after `:`) is treated as a binding candidate.
fn collect_bound_idents(body: &[Tok], params: &[String]) -> Vec<String> {
    let mut bound: Vec<String> = vec![];
    let mut i = 0;
    while i < body.len() {
        if body[i].kind == SyntaxKind::LET_KW {
            // Find the end of the pattern: the first `=` at depth 0 after
            // `let`. Type annotation `: Ty` between the pattern and `=`
            // is allowed and skipped.
            let pattern_start = i + 1;
            let mut j = pattern_start;
            let mut paren_depth = 0i32;
            let mut brace_depth = 0i32;
            let mut bracket_depth = 0i32;
            let mut type_colon_at: Option<usize> = None;
            while j < body.len() {
                let k = body[j].kind;
                match k {
                    SyntaxKind::L_PAREN => paren_depth += 1,
                    SyntaxKind::R_PAREN => paren_depth -= 1,
                    SyntaxKind::L_BRACE => brace_depth += 1,
                    SyntaxKind::R_BRACE => brace_depth -= 1,
                    SyntaxKind::L_BRACK => bracket_depth += 1,
                    SyntaxKind::R_BRACK => bracket_depth -= 1,
                    SyntaxKind::EQ
                        if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 =>
                    {
                        break;
                    }
                    SyntaxKind::COLON
                        if paren_depth == 0
                            && brace_depth == 0
                            && bracket_depth == 0
                            && type_colon_at.is_none() =>
                    {
                        type_colon_at = Some(j);
                    }
                    SyntaxKind::SEMI
                        if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 =>
                    {
                        // Malformed `let pat;` — bail.
                        break;
                    }
                    _ => {}
                }
                j += 1;
            }
            let pattern_end = type_colon_at.unwrap_or(j);
            harvest_pattern_idents(body, pattern_start, pattern_end, params, &mut bound);
            i = j + 1;
            continue;
        }
        i += 1;
    }
    bound
}

/// Pattern walker. Collects IDENTs from `body[start..end]` that are
/// binding sites:
///
///   * Plain IDENT (or `mut IDENT`, `ref IDENT`): binding.
///   * Inside `{ ... }` (struct pattern): IDENT followed by COLON
///     introduces a renamed binding (the IDENT *after* the colon is
///     the binding); a bare IDENT is shorthand-binding.
///   * Inside `( ... )` (tuple pattern): every IDENT is a binding.
///   * `Path::Variant(IDENT, ...)` (enum pattern): IDENTs inside the
///     parens are bindings; the leading path segments are not.
///   * `&IDENT` / `&mut IDENT`: ref pattern.
///
/// Conservative: we never treat the same identifier as bound twice,
/// and we skip type-annotation positions (handled by the caller).
fn harvest_pattern_idents(
    body: &[Tok],
    start: usize,
    end: usize,
    params: &[String],
    bound: &mut Vec<String>,
) {
    let mut i = start;
    // Track whether we're in a struct-pattern's `{ ... }` (vs tuple paren).
    // We use a small stack of brackets seen so we know which "kind" we're
    // inside when we hit an IDENT.
    #[derive(Copy, Clone, PartialEq, Eq)]
    enum BracketKind {
        Tuple,  // (
        Struct, // {
    }
    let mut bracket_stack: Vec<BracketKind> = vec![];
    // For struct patterns, we need a tiny state machine: after the
    // most recent COMMA or L_BRACE, the next IDENT is either:
    //   - field-name only (`{ a, b }`) → binds `a`, `b`
    //   - field-name with rename (`{ a: x }`) → binds `x`, not `a`
    // We track "expecting field name" until we see either COMMA/R_BRACE
    // (commit `a` as binding) or COLON (next IDENT is the binding).
    let mut struct_pending_field: Option<usize> = None; // index of pending field-name token
    while i < end {
        let tok = &body[i];
        match tok.kind {
            SyntaxKind::L_BRACE => {
                bracket_stack.push(BracketKind::Struct);
                struct_pending_field = None;
            }
            SyntaxKind::R_BRACE => {
                // Commit any dangling field-name as a binding.
                if let Some(idx) = struct_pending_field.take() {
                    add_binding(&body[idx].text, params, bound);
                }
                bracket_stack.pop();
            }
            SyntaxKind::L_PAREN => {
                bracket_stack.push(BracketKind::Tuple);
            }
            SyntaxKind::R_PAREN => {
                bracket_stack.pop();
            }
            SyntaxKind::COMMA => {
                if let Some(idx) = struct_pending_field.take() {
                    add_binding(&body[idx].text, params, bound);
                }
            }
            SyntaxKind::COLON if bracket_stack.last().copied() == Some(BracketKind::Struct) => {
                // Inside a struct pattern, the next IDENT after `:` is the
                // binding name. Drop the pending field-name (it's just a
                // field selector, not a binding).
                struct_pending_field = None;
                // Skip trivia, then bind the next IDENT.
                let mut j = i + 1;
                while j < end && body[j].is_trivia() {
                    j += 1;
                }
                if j < end && body[j].kind == SyntaxKind::IDENT {
                    add_binding(&body[j].text, params, bound);
                    i = j; // continue from the bound IDENT
                }
            }
            SyntaxKind::MUT_KW | SyntaxKind::REF_KW => {
                // The NEXT IDENT is the binding.
                let mut j = i + 1;
                while j < end && body[j].is_trivia() {
                    j += 1;
                }
                if j < end && body[j].kind == SyntaxKind::IDENT {
                    add_binding(&body[j].text, params, bound);
                    i = j;
                }
            }
            SyntaxKind::AMP => {
                // `&IDENT` or `&mut IDENT`. The IDENT after `&` (or after `&mut`)
                // is the binding.
                let mut j = i + 1;
                while j < end && body[j].is_trivia() {
                    j += 1;
                }
                if j < end && body[j].kind == SyntaxKind::MUT_KW {
                    j += 1;
                    while j < end && body[j].is_trivia() {
                        j += 1;
                    }
                }
                if j < end && body[j].kind == SyntaxKind::IDENT {
                    add_binding(&body[j].text, params, bound);
                    i = j;
                }
            }
            SyntaxKind::IDENT => {
                let in_struct = bracket_stack.last().copied() == Some(BracketKind::Struct);
                if in_struct {
                    // Inside `{ ... }`: this could be a field-name (shorthand
                    // binding) OR a field-name preceding a `:` rename. Defer
                    // the decision until we see COLON / COMMA / R_BRACE.
                    struct_pending_field = Some(i);
                } else {
                    // Tuple-pattern paren OR top level (`let x = ...`): bind it.
                    // Skip:
                    //   * Path segments: `Path :: Variant(...)` or `Path.Variant(...)`
                    //   * Struct-pattern type names: `User { ... }` — the IDENT is
                    //     the type, not a binding (the bindings are inside the
                    //     braces and get handled when we enter Struct mode).
                    //   * Enum-pattern type names: `Some(x)` — IDENT followed by
                    //     `(` is the variant constructor, not a binding. The
                    //     bindings are inside the parens.
                    let mut k = i + 1;
                    while k < end && body[k].is_trivia() {
                        k += 1;
                    }
                    let next_kind = body.get(k).map(|t| t.kind).unwrap_or(SyntaxKind::EOF);
                    let is_constructor_or_path = matches!(
                        next_kind,
                        SyntaxKind::COLON_COLON
                            | SyntaxKind::DOT
                            | SyntaxKind::L_BRACE
                            | SyntaxKind::L_PAREN
                    );
                    if !is_constructor_or_path {
                        add_binding(&tok.text, params, bound);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    if let Some(idx) = struct_pending_field.take() {
        add_binding(&body[idx].text, params, bound);
    }
}

fn add_binding(name: &str, params: &[String], bound: &mut Vec<String>) {
    if name == "_" || name.is_empty() {
        return;
    }
    if params.iter().any(|p| p == name) {
        return;
    }
    if !bound.iter().any(|b| b == name) {
        bound.push(name.to_string());
    }
}

// ============================================================================
// Set-of-scopes expansion (RFC-009)
// ============================================================================

/// Result of a scope-aware expansion: every output token carries the
/// scope set that should be consulted at name-resolution time, plus
/// a separate index of the *binding occurrences* introduced by the
/// macro body (each binding's scope set is what later references must
/// be matched against — see [`crate::scopes::resolve`]).
///
/// The expander emits the token stream tagged with scope sets and a
/// list of binding occurrences (`bindings`) introduced by this
/// invocation. Callers that need the source text can run the result
/// through `strip_scopes` + `tokens_to_source`; the convenience
/// [`expand_scoped_to_source`] does both in one step.
#[derive(Debug, Clone)]
pub struct ScopedExpansion {
    /// The expanded token stream, each token tagged with its scope set.
    pub tokens: Vec<ScopedTok>,
    /// Binding occurrences introduced by the macro body: `(text, scope_set)`.
    /// References whose scope set is a superset of one of these will
    /// resolve to that binding (see [`crate::scopes::resolve`]).
    pub bindings: Vec<(String, Scopes)>,
    /// The fresh scope ID minted for this invocation. Returned for
    /// callers that want to record/inspect the macro's identity.
    pub intro: ScopeId,
}

/// Expand `def` with `args` and a scope-set hygiene environment.
///
/// The sole macro-expansion entry point as of v0.15. (The pre-v0.14
/// single-mark `expand` / `expand_to_source` shims were removed once
/// the deprecation window closed.)
///
/// Inputs:
///   * Each output token carries a [`Scopes`] set.
///   * The caller supplies a [`ScopeGen`] so each invocation gets a
///     fresh scope ID minted off the same allocator.
///   * `def_scopes` is the scope set inherited from the macro's
///     *definition* site. Pass [`Scopes::empty`] for top-level macros;
///     pass the outer macro's body scope for macros defined inside
///     another macro's expansion.
///   * `caller_arg_scopes` is the scope set the call-site arguments
///     already carry. Typically `Scopes::empty()` for top-level user
///     source; for macro-in-macro composition the outer expansion
///     supplies its own scope set here.
///
/// The set-of-scopes rules applied:
///   * Body tokens receive `def_scopes ∪ {fresh}`.
///   * Argument tokens receive the caller's scope set unchanged (they
///     were not introduced by *this* macro, per Flatt 2016 §3).
///   * A mangling pass (`__mac_<intro>_<orig>`) is also applied to
///     macro-introduced bindings so the emitted source remains
///     parseable by the existing front-end; bindings are recorded
///     with their scope sets for the resolver.
pub fn expand_scoped(
    def: &MacroDef,
    args: &[&str],
    gen: &mut ScopeGen,
    def_scopes: Scopes,
    caller_arg_scopes: Scopes,
) -> Result<ScopedExpansion, ExpandError> {
    if args.len() != def.params.len() {
        return Err(ExpandError::ArityMismatch {
            expected: def.params.len(),
            actual: args.len(),
        });
    }

    let intro = gen.fresh();
    let env = HygieneEnv::for_invocation(intro, def_scopes.clone());
    let body_scopes = env.body_scopes();

    // Pre-lex each argument source slice so substitution preserves
    // token kinds; tag each lexed argument with the caller's scopes.
    let mut arg_scoped: Vec<Vec<ScopedTok>> = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        match lex_fragment(arg) {
            Some(toks) => {
                arg_scoped.push(env.apply_to_argument(&toks, &caller_arg_scopes));
            }
            None => return Err(ExpandError::BadArgumentTokens { index: i }),
        }
    }

    // Recompute the legacy "bound idents" list so we can both mangle
    // (preserving the existing parseable output shape) AND record
    // each binding's scope set for the resolver.
    let bound_names = collect_bound_idents(&def.body, &def.params);
    let mut bindings: Vec<(String, Scopes)> = Vec::with_capacity(bound_names.len());
    for name in &bound_names {
        bindings.push((name.clone(), body_scopes.clone()));
    }

    // Walk the body. For each token decide:
    //   - Parameter? splice argument tokens (with their caller-side scopes).
    //   - Bound IDENT introduced by this macro? mangle (legacy) + body scopes.
    //   - Otherwise: a body token; assign body scopes.
    //
    // The mangling keeps the legacy expander's output shape stable
    // even before downstream consumers wire up scope-aware resolution
    // — both layers point at the same binding.
    let mut tokens: Vec<ScopedTok> = Vec::with_capacity(def.body.len() + args.len() * 4);
    for tok in &def.body {
        if tok.kind == SyntaxKind::IDENT {
            // Parameter substitution: emit `(` argument-tokens `)`.
            if let Some(idx) = def.params.iter().position(|p| p == &tok.text) {
                tokens.push(env.scope_body_token(Tok::new(SyntaxKind::L_PAREN, "(")));
                tokens.extend(arg_scoped[idx].iter().cloned());
                tokens.push(env.scope_body_token(Tok::new(SyntaxKind::R_PAREN, ")")));
                continue;
            }
            // Macro-introduced binding: mangle (legacy) and tag with body scopes.
            if bound_names.iter().any(|b| b == &tok.text) {
                let mangled = Tok::new(SyntaxKind::IDENT, format!("__mac_{intro}_{}", tok.text));
                tokens.push(env.scope_body_token(mangled));
                continue;
            }
        }
        // Plain body token: keep verbatim, attach body scopes.
        tokens.push(env.scope_body_token(tok.clone()));
    }

    Ok(ScopedExpansion {
        tokens,
        bindings,
        intro,
    })
}

/// Convenience over [`expand_scoped`]: returns the expansion's source
/// text alongside the scope-set trace. The string is what the HIR
/// preprocessor splices back into the file; the trace is what a
/// future scope-aware name resolver consults.
///
/// `def_scopes` and `caller_arg_scopes` follow the same conventions
/// as [`expand_scoped`]: pass [`Scopes::empty`] for top-level
/// invocations.
pub fn expand_scoped_to_source(
    def: &MacroDef,
    args: &[&str],
    gen: &mut ScopeGen,
    def_scopes: Scopes,
    caller_arg_scopes: Scopes,
) -> Result<(String, ScopedExpansion), ExpandError> {
    let exp = expand_scoped(def, args, gen, def_scopes, caller_arg_scopes)?;
    let plain = crate::hygiene::strip_scopes(&exp.tokens);
    let src = tokens_to_source(&plain);
    Ok((src, exp))
}

#[cfg(test)]
mod tests {
    //! Unit tests for the scope-aware expander. Behavioural coverage
    //! for the source-text shape lives in `tests/` (integration tests
    //! migrated off the pre-v0.15 `expand_to_source`). The cases kept
    //! here are the few that don't duplicate any integration test.

    use super::*;
    use crate::registry::MacroRegistry;
    use mty_ast::{AstNode, File};
    use mty_syntax::SyntaxNode;

    fn def(src: &str, name: &str) -> MacroDef {
        let p = mty_syntax::parse(src);
        let root = SyntaxNode::new_root(p.green);
        let file = File::cast(root).unwrap();
        let reg = MacroRegistry::from_file(&file.0);
        reg.get(name).cloned().unwrap()
    }

    fn expand_src(d: &MacroDef, args: &[&str]) -> (String, ScopeId) {
        let mut gen = ScopeGen::new();
        let exp = expand_scoped(d, args, &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
        let plain = crate::hygiene::strip_scopes(&exp.tokens);
        (tokens_to_source(&plain), exp.intro)
    }

    #[test]
    fn arity_mismatch_reported() {
        let d = def("macro id(x) => { x }\n", "id");
        let mut gen = ScopeGen::new();
        let e =
            expand_scoped(&d, &["1", "2"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap_err();
        assert!(matches!(
            e,
            ExpandError::ArityMismatch {
                expected: 1,
                actual: 2
            }
        ));
    }

    #[test]
    fn free_name_not_mangled() {
        let d = def("macro p() => { panic(\"oops\") }\n", "p");
        let (s, _intro) = expand_src(&d, &[]);
        assert!(s.contains("panic"), "got: {s}");
        assert!(!s.contains("__mac_"), "free names must not be mangled");
    }

    #[test]
    fn parameter_inside_tuple_pattern_is_not_mangled() {
        // The macro parameter `p` appears inside the body — it should be
        // substituted, not mangled.
        let d = def("macro head(p) => { let (a, _) = p; a }\n", "head");
        let (s, intro) = expand_src(&d, &["pair"]);
        // `p` gets substituted to `(pair)`.
        assert!(s.contains("(pair)"), "param not substituted: {s}");
        // `a` is the macro-introduced binding.
        assert!(
            s.contains(&format!("__mac_{intro}_a")),
            "a not mangled: {s}"
        );
    }
}
