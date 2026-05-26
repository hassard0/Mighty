//! Set-of-scopes hygiene applied to token streams (RFC-009).
//!
//! This module bridges the abstract [`crate::scopes::Scopes`] type and
//! the concrete [`crate::token::Tok`] stream that the expander
//! consumes. Each scoped token pairs a `Tok` with the set of scope
//! IDs active when that token was introduced.
//!
//! The v0.13 baseline preserves the existing mangling-based expander
//! intact (see [`crate::expand`]); the scope-set machinery is wired
//! in alongside it via [`HygieneEnv::apply_to_body`] and
//! [`HygieneEnv::apply_to_argument`], which together drive the
//! "Bindings as Sets of Scopes" rules:
//!
//!   * **Fresh scope per invocation.** Every macro call mints one
//!     scope ID (via [`crate::scopes::ScopeGen`]) which is then
//!     added to every token *originating from the macro's body*.
//!   * **User tokens carry their pre-existing scopes.** Tokens that
//!     were substituted in from the call site are left at their
//!     original scope set — they were NOT introduced by this macro,
//!     so the macro's scope is not added.
//!   * **Definition scope.** Tokens from the body additionally carry
//!     any scope inherited from the macro's *definition* context. A
//!     macro defined inside another macro's expansion picks up that
//!     outer scope; that's what makes
//!     [`crate::scopes::resolve`] disambiguate the swap-macro case.
//!
//! Combined with the resolver in [`crate::scopes::resolve`], these
//! rules give the front-end enough information to distinguish
//! same-named bindings introduced by different expansions — including
//! the composition cases (nested macros, recursive macros, swap
//! macros) that simple Racket-style marks cannot disambiguate.

use crate::scopes::{ScopeId, Scopes};
use crate::token::Tok;
use mty_syntax::SyntaxKind;

/// A token paired with the scope set active when it was introduced.
///
/// During expansion every token in the output carries one of these;
/// after expansion the front-end uses the scope set during name
/// resolution (see [`crate::scopes::resolve`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedTok {
    pub tok: Tok,
    pub scopes: Scopes,
}

impl ScopedTok {
    /// Pair a token with a scope set.
    pub fn new(tok: Tok, scopes: Scopes) -> Self {
        Self { tok, scopes }
    }

    /// A token that came straight from the user's source with no
    /// macro involvement — empty scope set.
    pub fn user(tok: Tok) -> Self {
        Self {
            tok,
            scopes: Scopes::empty(),
        }
    }

    /// Add a scope to this token's set (returning the modified token).
    pub fn with_scope(mut self, s: ScopeId) -> Self {
        self.scopes = self.scopes.with(s);
        self
    }

    /// True iff `self.tok.kind` is `IDENT`. Useful when the expander
    /// wants to apply scope logic only to name-bearing tokens.
    pub fn is_ident(&self) -> bool {
        self.tok.kind == SyntaxKind::IDENT
    }
}

/// A single macro expansion's hygiene environment.
///
/// Carries:
///   * `intro` — the scope ID minted for this invocation. Added to
///     every body-introduced token.
///   * `def_scopes` — scopes inherited from the macro's *definition*
///     context. Empty for top-level macros; non-empty for macros
///     defined inside another macro's expansion.
///
/// The env is consumed once per invocation; the expander allocates a
/// new one (via [`HygieneEnv::for_invocation`]) each time it walks a
/// `MACRO_CALL`.
#[derive(Debug, Clone)]
pub struct HygieneEnv {
    pub intro: ScopeId,
    pub def_scopes: Scopes,
}

impl HygieneEnv {
    /// Create the hygiene env for a fresh macro invocation.
    ///
    /// `intro` should come from [`crate::scopes::ScopeGen::fresh`];
    /// `def_scopes` should come from the macro's definition site
    /// (empty for top-level macros).
    pub fn for_invocation(intro: ScopeId, def_scopes: Scopes) -> Self {
        Self { intro, def_scopes }
    }

    /// Scope set for a token *introduced by the macro body*:
    /// `def_scopes ∪ {intro}`.
    pub fn body_scopes(&self) -> Scopes {
        self.def_scopes.with(self.intro)
    }

    /// Apply this env to a token introduced by the macro body. The
    /// body token gets the macro's full body-scope set (definition
    /// scopes + the fresh invocation scope).
    pub fn scope_body_token(&self, tok: Tok) -> ScopedTok {
        ScopedTok::new(tok, self.body_scopes())
    }

    /// Apply this env to a token that came from the *call site*
    /// (e.g. a substituted parameter). Per Flatt's rule, user tokens
    /// do NOT receive the macro's intro scope: they were not
    /// introduced by this expansion, so they retain whatever scope
    /// set they already had.
    ///
    /// `existing` is the scope set the token already carried at the
    /// call site (typically `Scopes::empty()` for top-level user
    /// source).
    pub fn scope_user_token(&self, tok: Tok, existing: Scopes) -> ScopedTok {
        ScopedTok::new(tok, existing)
    }

    /// Walk a macro body and lift every token into a [`ScopedTok`]
    /// with [`Self::body_scopes`]. Used as the starting point before
    /// parameter substitution.
    pub fn apply_to_body(&self, body: &[Tok]) -> Vec<ScopedTok> {
        body.iter()
            .map(|t| self.scope_body_token(t.clone()))
            .collect()
    }

    /// Walk a call-site argument's token stream and lift every token
    /// into a [`ScopedTok`] with the argument's pre-existing scope
    /// set. The macro's intro scope is *not* added — user tokens
    /// stay anchored to their caller's binding environment.
    pub fn apply_to_argument(&self, arg: &[Tok], caller_scopes: &Scopes) -> Vec<ScopedTok> {
        arg.iter()
            .map(|t| ScopedTok::new(t.clone(), caller_scopes.clone()))
            .collect()
    }
}

/// Strip scope sets from a `ScopedTok` stream, recovering a plain
/// `Vec<Tok>`. Used by callers that just want the resulting source
/// text and don't yet consume scope information downstream.
pub fn strip_scopes(toks: &[ScopedTok]) -> Vec<Tok> {
    toks.iter().map(|st| st.tok.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scopes::ScopeGen;
    use mty_syntax::SyntaxKind;

    fn ident(name: &str) -> Tok {
        Tok::new(SyntaxKind::IDENT, name)
    }

    #[test]
    fn scoped_tok_round_trip() {
        let t = ident("x");
        let st = ScopedTok::user(t.clone());
        assert!(st.scopes.is_empty());
        assert!(st.is_ident());
        let st2 = st.with_scope(7);
        assert_eq!(st2.scopes.len(), 1);
        assert!(st2.scopes.iter().any(|s| s == 7));
    }

    #[test]
    fn hygiene_env_body_gets_intro_scope() {
        let mut gen = ScopeGen::new();
        let env = HygieneEnv::for_invocation(gen.fresh(), Scopes::empty());
        let body = vec![ident("y")];
        let out = env.apply_to_body(&body);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].scopes.len(), 1);
    }

    #[test]
    fn hygiene_env_user_tokens_keep_their_scopes() {
        let mut gen = ScopeGen::new();
        let outer = gen.fresh();
        let inner = gen.fresh();
        // Caller already has scope `outer`; the inner macro invocation
        // has scope `inner` and applies it ONLY to body tokens.
        let env = HygieneEnv::for_invocation(inner, Scopes::empty());
        let arg = vec![ident("x")];
        let caller = Scopes::empty().with(outer);
        let out = env.apply_to_argument(&arg, &caller);
        assert_eq!(out[0].scopes, caller);
        assert!(!out[0].scopes.iter().any(|s| s == inner));
    }

    #[test]
    fn body_scopes_include_definition_scopes() {
        // A macro defined inside an outer expansion carries the outer
        // scope at its definition site; body tokens pick that up.
        let def_scopes = Scopes::from_iter([10]);
        let env = HygieneEnv::for_invocation(20, def_scopes.clone());
        let bs = env.body_scopes();
        assert!(bs.iter().any(|s| s == 10));
        assert!(bs.iter().any(|s| s == 20));
    }

    #[test]
    fn strip_scopes_round_trip() {
        let env = HygieneEnv::for_invocation(1, Scopes::empty());
        let body = vec![ident("a"), ident("b")];
        let scoped = env.apply_to_body(&body);
        let stripped = strip_scopes(&scoped);
        assert_eq!(stripped.len(), 2);
        assert_eq!(stripped[0].text, "a");
        assert_eq!(stripped[1].text, "b");
    }
}
