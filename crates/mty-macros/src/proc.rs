//! Procedural macros (v0.5 — parse + storage only).
//!
//! A procedural macro is a Mighty function of shape
//! `fn(input: TokenStream) -> TokenStream` declared with the `proc macro`
//! item form. The body manipulates tokens at compile time and emits
//! replacement tokens for the call site.
//!
//! ## v0.5 status
//!
//! v0.5 ships **parsing + storage + impurity detection** for proc macros.
//! Execution is deferred to v0.6 because running a Mighty function at
//! compile time requires a sandboxed SIR sub-interpreter that doesn't
//! exist yet. Call sites for proc macros parse + lower correctly but
//! emit **MT6006** (`proc_macro_unsupported_v0_5`) instead of running
//! the body.
//!
//! ## Sandbox constraints (planned for v0.6)
//!
//! When the v0.6 interpreter ships, proc-macro execution will be
//! sandboxed with these limits:
//!
//!   * **No effects.** Token-tree manipulation only — no I/O, no `time`,
//!     no `env`, no `model`, no `rand`. v0.5 catches this statically and
//!     emits MT6005 at declaration time. See [`check_proc_macro_purity`].
//!   * **Wall-clock timeout.** 100 ms hard cap per expansion, enforced
//!     via `tokio::time::timeout` around the SIR step loop.
//!   * **Memory cap.** 16 MB of intermediate state (token buffers +
//!     interpreter stack).
//!   * **Step cap.** 100,000 SIR steps per expansion to bound CPU
//!     even if the wall-clock check is delayed.
//!
//! These constants are exposed below so callers (a future
//! `proc::execute(...)` API) and the spec doc agree on the same values.

use crate::registry::{MacroDef, MacroKind};
use crate::token::Tok;
use mty_syntax::SyntaxKind;

/// Wall-clock budget for one proc-macro expansion (v0.6 limit).
pub const PROC_MACRO_WALL_MS: u64 = 100;

/// Memory budget for one proc-macro expansion (v0.6 limit).
pub const PROC_MACRO_MEM_BYTES: usize = 16 * 1024 * 1024;

/// SIR-step budget for one proc-macro expansion (v0.6 limit).
pub const PROC_MACRO_STEPS: u64 = 100_000;

/// Reason a proc-macro body is rejected as impure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImpurityReason {
    /// Body invokes an `effect.<name>(...)` call.
    EffectCall(String),
    /// Body invokes a name from the well-known impure surface (`time`,
    /// `env`, `io`, `model`, `rand`) without going through `effect`.
    BareImpureCall(String),
}

impl std::fmt::Display for ImpurityReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImpurityReason::EffectCall(name) => {
                write!(f, "calls effect `{name}` (proc macros must be pure)")
            }
            ImpurityReason::BareImpureCall(name) => {
                write!(
                    f,
                    "calls impure surface `{name}` (proc macros must be pure)"
                )
            }
        }
    }
}

/// Result of executing a proc macro. v0.5 always returns
/// [`ProcMacroResult::Unsupported`]; v0.6 will replace this with a
/// real Ok/Err path.
#[derive(Debug, Clone)]
pub enum ProcMacroResult {
    /// Successful expansion produced these output tokens.
    Ok(Vec<Tok>),
    /// v0.5: parsed-and-stored but not yet executable. MT6006.
    Unsupported,
    /// Body was rejected for impurity. MT6005.
    Impure(ImpurityReason),
}

/// Attempt to "expand" a proc macro. v0.5 returns `Unsupported` unless
/// the body fails purity, in which case it returns `Impure`.
///
/// `_input` is the call-site token tree (the bytes between the `(`
/// and matching `)`, lexed). v0.5 ignores it because there's nothing
/// to feed to yet; v0.6 will pipe it through the SIR interpreter.
pub fn expand_proc(def: &MacroDef, _input: &[Tok]) -> ProcMacroResult {
    debug_assert_eq!(def.kind, MacroKind::Procedural);
    if let Some(reason) = check_proc_macro_purity(&def.body) {
        return ProcMacroResult::Impure(reason);
    }
    ProcMacroResult::Unsupported
}

/// Walk a proc-macro body's tokens and return the first impurity
/// reason, if any. The check is purely syntactic — it looks for
/// identifiers in call position whose name matches the well-known
/// impure surface (`effect`, `time`, `env`, `io`, `model`, `rand`).
///
/// False negatives are tolerated (we won't catch aliasing through a
/// `let` binding); v0.6's sandbox is the authoritative gate. v0.5's
/// check exists to fail fast for the obvious cases at decl time.
pub fn check_proc_macro_purity(body: &[Tok]) -> Option<ImpurityReason> {
    // Strategy: scan for IDENT tokens. For each IDENT, check if the
    // NEXT non-trivia token is `.` (effect-shape: `effect.io(...)`) or
    // `(` (bare call). The first IDENT in `effect.foo(...)` chains is
    // `effect`, which is itself a keyword in Mighty — so we also
    // match EFFECT_KW.
    let mut i = 0;
    while i < body.len() {
        let kind = body[i].kind;
        let text = body[i].text.as_str();
        let is_effect_kw = kind == SyntaxKind::EFFECT_KW;
        let is_ident = kind == SyntaxKind::IDENT;
        if is_effect_kw || is_ident {
            // Skip trivia and look at the next token.
            let mut j = i + 1;
            while j < body.len() && body[j].is_trivia() {
                j += 1;
            }
            let next = body.get(j).map(|t| t.kind);
            if is_effect_kw && next == Some(SyntaxKind::DOT) {
                // Read effect name after the dot.
                let mut k = j + 1;
                while k < body.len() && body[k].is_trivia() {
                    k += 1;
                }
                let eff_name = body
                    .get(k)
                    .map(|t| t.text.clone())
                    .unwrap_or_else(|| "<unknown>".to_string());
                return Some(ImpurityReason::EffectCall(eff_name));
            }
            if is_ident
                && matches!(next, Some(SyntaxKind::L_PAREN) | Some(SyntaxKind::DOT))
                && matches!(text, "time" | "env" | "io" | "model" | "rand")
            {
                // Either `time(...)` bare call or `time.now()` method call.
                return Some(ImpurityReason::BareImpureCall(text.to_string()));
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::MacroRegistry;
    use mty_ast::{AstNode, File};
    use mty_syntax::SyntaxNode;

    fn parse_proc(src: &str, name: &str) -> MacroDef {
        let p = mty_syntax::parse(src);
        let root = SyntaxNode::new_root(p.green);
        let file = File::cast(root).unwrap();
        let reg = MacroRegistry::from_file(&file.0);
        reg.get(name).cloned().unwrap()
    }

    #[test]
    fn pure_proc_macro_is_unsupported_not_impure() {
        let src = "proc macro identity(input: TokenStream) -> TokenStream { input }\n";
        let def = parse_proc(src, "identity");
        assert!(matches!(
            expand_proc(&def, &[]),
            ProcMacroResult::Unsupported
        ));
    }

    #[test]
    fn effect_call_in_proc_body_is_impure() {
        let src = concat!(
            "proc macro leak(input: TokenStream) -> TokenStream {\n",
            "  effect.io(\"hi\")\n",
            "  input\n",
            "}\n",
        );
        let def = parse_proc(src, "leak");
        match expand_proc(&def, &[]) {
            ProcMacroResult::Impure(ImpurityReason::EffectCall(name)) => {
                assert_eq!(name, "io");
            }
            other => panic!("expected EffectCall impurity, got: {:?}", other),
        }
    }

    #[test]
    fn bare_time_call_in_proc_body_is_impure() {
        let src = concat!(
            "proc macro stamp(input: TokenStream) -> TokenStream {\n",
            "  let t = time.now()\n",
            "  input\n",
            "}\n",
        );
        let def = parse_proc(src, "stamp");
        assert!(matches!(
            expand_proc(&def, &[]),
            ProcMacroResult::Impure(ImpurityReason::BareImpureCall(_))
        ));
    }
}
