//! v0.29 Track C: typed bang-send / ask return-type lowering.
//!
//! Pre-v0.29 the `Send` (`agent ! Msg(args)`) and `Ask`
//! (`agent ? Msg(args)`) checker arms unconditionally synthesised
//! Unit / a fresh inference variable, so call sites like
//! `let r: Str = bot ! Review(s)` either had to be hand-wrapped in
//! `format!(...)` (v0.27 demo 08 workaround) or threw a spurious
//! type mismatch.
//!
//! These tests pin that the resolved expression type at the bang-send
//! / ask site matches the protocol's declared `-> ReturnTy`, across:
//!   * `-> Str` (primitive)
//!   * `-> Vec[T]` (generic ADT)
//!   * `-> Result[T, E]` (two-arg generic ADT)
//!   * `-> ()` (explicit Unit)
//!   * missing return annotation (defaults to Unit)
//!   * arity-mismatched call (still resolves to declared reply; the
//!     arity check is a v0.30+ follow-up)

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_hir::HirExpr;
use mty_types::ty::{pretty_ty, TyData};
use mty_types::{check_package, check_package_typed};

fn errors(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "bang_send_return_type.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_lower_err {
        diags.extend(check_package(&pkg));
    }
    diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| format!("{}: {}", d.code.as_str(), d.primary.message))
        .collect()
}

/// Find the first `Send`/`Ask` expression in `pkg`, type-check, and
/// return its rendered resolved type via `pretty_ty`.
fn first_send_or_ask_ty(src: &str) -> String {
    let parsed = parse_source(src.into(), "bang_send_return_type.mty".into());
    let (pkg, lower_diags) = lower(&parsed);
    let any_lower_err = lower_diags
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));
    assert!(
        !any_lower_err,
        "expected clean lowering, got {:?}",
        lower_diags
    );
    let typed = check_package_typed(&pkg);
    let mut found = None;
    for (eid, expr) in pkg.exprs.iter() {
        if matches!(expr, HirExpr::Send { .. } | HirExpr::Ask { .. }) {
            found = Some(eid);
            break;
        }
    }
    let eid = found.expect("expected a Send/Ask expression in the source");
    let ty = typed
        .expr_ty
        .get(&eid)
        .copied()
        .expect("Send/Ask expr should have a recorded type");
    pretty_ty(ty, &typed.ty_arena, None, Some(&typed.def_map))
}

#[test]
fn bang_send_returns_declared_str_reply() {
    let src = r#"
        protocol Reviewer { Review(s: Str) -> Str }
        agent Bot: Reviewer {
          on Review(s) -> s
        }
        fn main() {
          let bot = spawn Bot()
          let r: Str = bot ! Review("hi")
        }
    "#;
    // Clean type-check: pre-v0.29 the `let r: Str = ...` annotation
    // tripped a Unit / Str mismatch.
    let errs = errors(src);
    assert!(
        errs.is_empty(),
        "expected clean typeck for declared `-> Str` bang-send, got {:?}",
        errs
    );
    let ty = first_send_or_ask_ty(src);
    assert_eq!(
        ty, "Str",
        "bang-send should resolve to declared reply Str, got {:?}",
        ty
    );
}

#[test]
fn bang_send_returns_declared_vec_reply() {
    let src = r#"
        protocol Picker { Pick(n: I32) -> Vec[Str] }
        agent Bag: Picker {
          on Pick(n) -> n
        }
        fn main() {
          let bag = spawn Bag()
          let r = bag ! Pick(3)
        }
    "#;
    let ty = first_send_or_ask_ty(src);
    assert!(
        ty.starts_with("Vec"),
        "expected Vec[...] reply, got {:?}",
        ty
    );
}

#[test]
fn bang_send_returns_declared_result_reply() {
    let src = r#"
        protocol Maybe { Try(s: Str) -> Result[Str, Str] }
        agent Tryer: Maybe {
          on Try(s) -> s
        }
        fn main() {
          let bot = spawn Tryer()
          let r = bot ! Try("hello")
        }
    "#;
    let ty = first_send_or_ask_ty(src);
    assert!(
        ty.starts_with("Result"),
        "expected Result[...] reply, got {:?}",
        ty
    );
}

#[test]
fn bang_send_with_explicit_unit_reply_resolves_unit() {
    let src = r#"
        protocol Sink { Drop(s: Str) -> () }
        agent S: Sink {
          on Drop(s) -> s
        }
        fn main() {
          let s_ref = spawn S()
          let _u = s_ref ! Drop("bye")
        }
    "#;
    let ty = first_send_or_ask_ty(src);
    assert!(
        ty == "Unit" || ty == "()",
        "expected Unit reply for `-> ()`, got {:?}",
        ty
    );
}

#[test]
fn bang_send_with_missing_reply_annotation_defaults_to_unit() {
    // Protocol message has no `-> ReturnTy` → default Unit. The pre-v0.29
    // behaviour also returned Unit here, so this test pins the
    // backward-compat path.
    let src = r#"
        protocol Noisy { Ping(s: Str) }
        agent N: Noisy {
          on Ping(s) -> s
        }
        fn main() {
          let n_ref = spawn N()
          let _u = n_ref ! Ping("yo")
        }
    "#;
    let ty = first_send_or_ask_ty(src);
    assert_eq!(
        ty, "Unit",
        "missing `-> ReturnTy` should default to Unit, got {:?}",
        ty
    );
}

#[test]
fn bang_send_with_arity_mismatch_still_resolves_declared_reply() {
    // v0.29 mandate is **return-type** lowering — arity-mismatched calls
    // are a v0.30+ follow-up. Pin that the reply type is still the
    // declared one so call sites keep type-checking once the arity
    // diagnostic lands.
    let src = r#"
        protocol Reviewer { Review(s: Str) -> Str }
        agent Bot: Reviewer {
          on Review(s) -> s
        }
        fn main() {
          let bot = spawn Bot()
          // Missing required arg `s` — the arity check is a v0.30+
          // follow-up. The return type should still be the declared Str
          // so we don't double-fault.
          let r: Str = bot ! Review()
        }
    "#;
    let ty = first_send_or_ask_ty(src);
    assert_eq!(
        ty, "Str",
        "arity-mismatched bang-send should still resolve declared Str reply, got {:?}",
        ty
    );
}

#[test]
fn ask_returns_declared_reply_like_bang_send() {
    // `?Msg(args)` shares the same lowering as `!Msg(args)` post-v0.29
    // — both surface the protocol's declared reply.
    let src = r#"
        protocol Reviewer { Review(s: Str) -> Str }
        agent Bot: Reviewer {
          on Review(s) -> s
        }
        fn main() {
          let bot = spawn Bot()
          let r: Str = bot ? Review("hi")
        }
    "#;
    let ty = first_send_or_ask_ty(src);
    assert_eq!(
        ty, "Str",
        "ask should resolve to declared reply Str, got {:?}",
        ty
    );
}

#[test]
fn bang_send_with_unknown_target_falls_back_to_fresh_var() {
    // When the target type doesn't drill down to an agent ADT we
    // emitted a known protocol for, the checker falls back to a fresh
    // inference variable rather than hard-erroring. This keeps `let r =
    // some_handle ! Msg(...)` typeable while the handle's exact agent
    // type is being inferred elsewhere.
    let src = r#"
        protocol Reviewer { Review(s: Str) -> Str }
        fn main() {
          // `external_handle()` returns something opaque; no protocol
          // info available at the bang-send site.
          let r = external_handle() ! Review("hi")
        }
    "#;
    let ty = first_send_or_ask_ty(src);
    // Fresh inference var → renders as ?N. Either that or a concrete
    // type would be fine here — we just pin that no panic / crash
    // occurs and the expression has *some* recorded type.
    assert!(
        !ty.is_empty(),
        "expected some resolved type for unknown-target bang-send, got {:?}",
        ty
    );
}

#[test]
fn bang_send_str_reply_unifies_with_let_annotation() {
    // The motivating bug: `let r: Str = bot ! Review(s)` previously
    // emitted a Unit / Str mismatch (or silently dropped). Now it's
    // clean.
    let src = r#"
        protocol Reviewer { Review(s: Str) -> Str }
        agent Bot: Reviewer {
          on Review(s) -> s
        }
        fn main() {
          let bot = spawn Bot()
          let r: Str = bot ! Review("hi")
          let _ = r
        }
    "#;
    let errs = errors(src);
    assert!(
        errs.is_empty(),
        "typed bang-send result should unify with the `let r: Str` annotation, got {:?}",
        errs
    );
    // Bonus: ensure the side-table also recorded TyData::Str (rather
    // than Unit) — this catches a regression where the let-binding
    // hides the upstream Unit because the annotation is taken at face
    // value.
    let parsed = parse_source(src.into(), "bang_send_let.mty".into());
    let (pkg, _) = lower(&parsed);
    let typed = check_package_typed(&pkg);
    let send_eid = pkg
        .exprs
        .iter()
        .find_map(|(eid, e)| match e {
            HirExpr::Send { .. } => Some(eid),
            _ => None,
        })
        .expect("Send expr in source");
    let ty = typed
        .expr_ty
        .get(&send_eid)
        .copied()
        .expect("Send expr ty recorded");
    let data = typed.ty_arena.get(ty);
    assert!(
        matches!(data, TyData::Str),
        "expected resolved Str at Send site, got {:?}",
        data
    );
}
