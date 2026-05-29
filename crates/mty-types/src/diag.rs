//! Diagnostic constructors for the type checker. Each function builds a
//! `Diagnostic` with the appropriate code, message, and severity.

use crate::defs::DefMap;
use crate::infer::Substitution;
use crate::ty::{pretty_ty, TyArena, TyId};
use mty_diagnostics::{codes::*, Diagnostic, Label, Severity};
use mty_hir::SourceSpan;

fn label(span: &SourceSpan, msg: impl Into<String>) -> Label {
    Label {
        start: span.start as usize,
        end: span.end as usize,
        message: msg.into(),
    }
}

pub fn mismatch(
    expected: TyId,
    found: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let e = pretty_ty(expected, arena, Some(subst), Some(defs));
    let f = pretty_ty(found, arena, Some(subst), Some(defs));
    Diagnostic::error(
        TYPE_MISMATCH,
        label(span, format!("expected `{}`, found `{}`", e, f)),
    )
}

pub fn unresolved_type(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        UNRESOLVED_TYPE,
        label(span, format!("cannot find type `{}` in scope", name)),
    )
}

pub fn cannot_infer(span: &SourceSpan, what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        CANNOT_INFER_TYPE,
        label(span, format!("cannot infer type for {}", what.into())),
    )
}

pub fn wrong_generic_arity(
    expected: usize,
    got: usize,
    span: &SourceSpan,
    what: &str,
) -> Diagnostic {
    Diagnostic::error(
        WRONG_GENERIC_ARITY,
        label(
            span,
            format!(
                "`{}` expects {} generic argument(s), got {}",
                what, expected, got
            ),
        ),
    )
}

pub fn wrong_arg_count(expected: usize, got: usize, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        WRONG_ARG_COUNT,
        label(
            span,
            format!("function expects {} argument(s), got {}", expected, got),
        ),
    )
}

pub fn unknown_field(field: &str, struct_name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        UNKNOWN_FIELD,
        label(
            span,
            format!("struct `{}` has no field `{}`", struct_name, field),
        ),
    )
}

pub fn unknown_method(
    method: &str,
    recv: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let r = pretty_ty(recv, arena, Some(subst), Some(defs));
    Diagnostic::error(
        UNKNOWN_METHOD,
        label(span, format!("type `{}` has no method `{}`", r, method)),
    )
}

pub fn not_callable(
    callee: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let c = pretty_ty(callee, arena, Some(subst), Some(defs));
    Diagnostic::error(
        NOT_CALLABLE,
        label(span, format!("value of type `{}` is not callable", c)),
    )
}

pub fn unknown_variant(variant: &str, enum_name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        UNKNOWN_VARIANT,
        label(
            span,
            format!("enum `{}` has no variant `{}`", enum_name, variant),
        ),
    )
}

pub fn question_outside_result(span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        QUESTION_OUTSIDE_RESULT,
        label(
            span,
            "`?` requires the enclosing function to return `Result[_, _]`",
        ),
    )
}

pub fn question_error_mismatch(
    expected: TyId,
    found: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let e = pretty_ty(expected, arena, Some(subst), Some(defs));
    let f = pretty_ty(found, arena, Some(subst), Some(defs));
    Diagnostic::error(
        QUESTION_ERROR_MISMATCH,
        label(
            span,
            format!("`?` error type mismatch: expected `{}`, found `{}`", e, f),
        ),
    )
}

pub fn wrong_variant_arity(
    variant: &str,
    expected: usize,
    got: usize,
    span: &SourceSpan,
) -> Diagnostic {
    Diagnostic::error(
        WRONG_VARIANT_ARITY,
        label(
            span,
            format!(
                "variant `{}` expects {} payload value(s), got {}",
                variant, expected, got
            ),
        ),
    )
}

/// v0.12 (Gap B / MT2022 emit-site): a value being initialised with a
/// struct literal resolves to a non-struct ADT (enum or opaque). Pre-v0.12
/// the synth path silently treated this as opaque; the new check fires
/// MT2022 with both the ADT's actual kind and its name.
pub fn not_a_struct(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        NOT_A_STRUCT,
        label(
            span,
            format!(
                "type `{}` is not a struct; struct literal syntax does not apply",
                name
            ),
        ),
    )
}

pub fn missing_struct_field(field: &str, struct_name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        MISSING_STRUCT_FIELD,
        label(
            span,
            format!(
                "missing field `{}` in initializer for `{}`",
                field, struct_name
            ),
        ),
    )
}

pub fn duplicate_struct_field(field: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        DUPLICATE_STRUCT_FIELD,
        label(
            span,
            format!("duplicate field `{}` in struct initializer", field),
        ),
    )
}

pub fn non_exhaustive_match(span: &SourceSpan, missing: &[String]) -> Diagnostic {
    // Slice 4 (A16): non-exhaustive match is an error (was warning in slice 3).
    let mut d = Diagnostic::error(NON_EXHAUSTIVE_MATCH, label(span, "non-exhaustive match"));
    if !missing.is_empty() {
        d.notes
            .push(format!("missing pattern(s): {}", missing.join(", ")));
    }
    d
}

/// v0.22 Coverage Closure (MT2016 emit-site): a `match` arm cannot fire
/// because an earlier arm always matches first (typical shape: a literal
/// or wildcard arm shadowing a later, more specific or identical arm).
/// Emitted as a **warning** to match the explain text (§slice-3 semantics
/// — unreachable arms are diagnostic noise, not a compile failure).
pub fn unreachable_match_arm(span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        UNREACHABLE_MATCH_ARM,
        label(
            span,
            "unreachable match arm (earlier arm always matches first)",
        ),
    );
    d.severity = Severity::Warning;
    d.notes
        .push("remove the dead arm, or reorder so the more specific pattern comes first".into());
    d
}

/// v0.22 Coverage Closure (MT2018 emit-site): the two branches of an
/// `if/else` expression produce incompatible types. Pre-v0.22 this
/// shape funnelled through MT2001 (generic type mismatch); v0.22 surfaces
/// MT2018 at the `if`-expression span so `mty explain MT2018` text points
/// at the join site rather than at one of the branch values.
pub fn if_branch_mismatch(
    then_ty: TyId,
    else_ty: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let t = pretty_ty(then_ty, arena, Some(subst), Some(defs));
    let e = pretty_ty(else_ty, arena, Some(subst), Some(defs));
    let mut d = Diagnostic::error(
        IF_BRANCH_MISMATCH,
        label(
            span,
            format!(
                "`if` branches produce incompatible types: `{}` vs `{}`",
                t, e
            ),
        ),
    );
    d.notes.push(
        "unify the two branches (e.g. via a common type), or remove the value-use of the `if`"
            .into(),
    );
    d
}

pub fn protocol_msg_unknown(msg: &str, span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        PROTOCOL_MSG_UNKNOWN,
        label(
            span,
            format!(
                "handler message `{}` is not declared by any implemented protocol",
                msg
            ),
        ),
    );
    d.severity = Severity::Warning;
    d
}

pub fn binop_type_mismatch(
    op: &str,
    lhs: TyId,
    rhs: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let l = pretty_ty(lhs, arena, Some(subst), Some(defs));
    let r = pretty_ty(rhs, arena, Some(subst), Some(defs));
    Diagnostic::error(
        BINOP_TYPE_MISMATCH,
        label(
            span,
            format!("operator `{}` not defined for `{}` and `{}`", op, l, r),
        ),
    )
}

pub fn return_type_mismatch(
    expected: TyId,
    found: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let e = pretty_ty(expected, arena, Some(subst), Some(defs));
    let f = pretty_ty(found, arena, Some(subst), Some(defs));
    Diagnostic::error(
        RETURN_TYPE_MISMATCH,
        label(
            span,
            format!("function returns `{}`, body produces `{}`", e, f),
        ),
    )
}

pub fn pub_param_needs_type(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        PUB_PARAM_NEEDS_TYPE,
        label(
            span,
            format!(
                "public function parameter `{}` requires an explicit type",
                name
            ),
        ),
    )
}

pub fn unresolved_value(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        UNRESOLVED_VALUE,
        label(span, format!("cannot find value `{}` in scope", name)),
    )
}

/// v0.3 (A65): an unresolved value name appeared inside a strict scope
/// (agent body, handler body, supervisor body, narrow-cap body). Slice 3's
/// permissive A21 fresh-var fallback only applies in top-level / extern /
/// unsafe scopes; strict scopes promote the failure to MT2021 with a
/// scope-aware note so the author understands why fresh-var inference
/// won't paper over the missing binding.
pub fn unresolved_value_strict(name: &str, scope: &str, span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        UNRESOLVED_VALUE,
        label(
            span,
            format!(
                "cannot find value `{}` in scope (strict {} scope; v0.3 A65)",
                name, scope
            ),
        ),
    );
    d.notes.push(format!(
        "the {} scope rejects unknown values — bind {} via state, ctor-param, prelude, or import",
        scope, name
    ));
    d
}

/// v0.37 T2 (MT2027 emit-site): `expr as Ty` between two types that have
/// no defined scalar-conversion path. The parser used to silently
/// degrade these into `BinOp::Add`; now the parser emits a real
/// `CAST_EXPR` and the type checker rejects unsupported shapes here.
pub fn invalid_cast(
    src: TyId,
    dst: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let s = crate::ty::pretty_ty(src, arena, Some(subst), Some(defs));
    let d = crate::ty::pretty_ty(dst, arena, Some(subst), Some(defs));
    Diagnostic::error(
        INVALID_CAST,
        label(
            span,
            format!(
                "invalid cast: `{}` as `{}` is not a recognised scalar conversion",
                s, d
            ),
        ),
    )
}

/// v0.12 (Gap B / MT2025 emit-site): a borrow expression's inner term is
/// not a place (l-value), so `&expr` cannot apply. Pre-v0.12 the synth
/// path silently constructed a `Ref` over the synthesised type.
pub fn cannot_take_ref(span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        CANNOT_TAKE_REF,
        label(
            span,
            "cannot take reference of a non-place expression (literal, call result, etc.)",
        ),
    )
}

/// v0.14 (Gap B / MT2023 emit-site): a generic argument resolves to a
/// value-kind def (function, enum variant constructor) rather than a
/// type-kind def. Pre-v0.14 this was funnelled through MT2002
/// ("unresolved type"), which mis-described the actual failure: the
/// name DOES resolve — it's just the wrong kind for the position.
/// v0.14 fires MT2023 at the generic-arg site so the user sees the
/// kind-mismatch explanation from `mty explain MT2023`.
///
/// `outer` is the enclosing constructor whose generic slot was filled
/// (e.g. `Result` in `Result[main, Err]`); `arg_kind` is the rejected
/// kind ("function", "variant constructor", etc.).
pub fn generic_arg_kind_mismatch(
    outer: &str,
    arg_name: &str,
    arg_kind: &str,
    span: &SourceSpan,
) -> Diagnostic {
    let mut d = Diagnostic::error(
        GENERIC_ARG_MISMATCH,
        label(
            span,
            format!(
                "generic argument `{}` to `{}` is a {}, not a type",
                arg_name, outer, arg_kind
            ),
        ),
    );
    d.notes.push(format!(
        "type-kind expected; `{}` resolves to a value-kind def",
        arg_name
    ));
    d
}

pub fn lambda_arity_mismatch(expected: usize, got: usize, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        LAMBDA_ARITY_MISMATCH,
        label(
            span,
            format!("lambda has {} parameter(s), expected {}", got, expected),
        ),
    )
}

// --- Slice 5 diagnostic constructors ---

pub fn effect_undeclared(fn_name: &str, missing: &[String], span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        EFFECT_UNDECLARED,
        label(
            span,
            format!(
                "public function `{}` is missing declared effect(s): {}",
                fn_name,
                missing.join(", ")
            ),
        ),
    );
    d.notes.push(format!(
        "add `effect {}` to the function signature",
        missing.join(", ")
    ));
    d
}

pub fn alloc_in_core(fn_name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        ALLOC_IN_CORE,
        label(
            span,
            format!(
                "function `{}` allocates on the heap; the `core` profile bans `alloc`",
                fn_name
            ),
        ),
    )
}

pub fn capability_too_broad(
    arg: TyId,
    param: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let a = pretty_ty(arg, arena, Some(subst), Some(defs));
    let p = pretty_ty(param, arena, Some(subst), Some(defs));
    Diagnostic::error(
        CAPABILITY_TOO_BROAD,
        label(
            span,
            format!(
                "capability argument `{}` is too broad for parameter `{}`",
                a, p
            ),
        ),
    )
}

pub fn method_ambiguous(method: &str, candidates: &[String], span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        METHOD_AMBIGUOUS,
        label(
            span,
            format!(
                "method `{}` is provided by multiple traits in scope",
                method
            ),
        ),
    );
    if !candidates.is_empty() {
        d.notes
            .push(format!("candidates: {}", candidates.join(", ")));
    }
    d
}

pub fn method_not_found(
    method: &str,
    recv: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let r = pretty_ty(recv, arena, Some(subst), Some(defs));
    Diagnostic::error(
        METHOD_NOT_FOUND,
        label(
            span,
            format!("no method `{}` found for receiver `{}`", method, r),
        ),
    )
}

pub fn trait_coherence_violation(
    trait_name: &str,
    self_name: &str,
    span: &SourceSpan,
) -> Diagnostic {
    Diagnostic::error(
        TRAIT_COHERENCE_VIOLATION,
        label(
            span,
            format!(
                "trait `{}` is implemented twice for `{}`",
                trait_name, self_name
            ),
        ),
    )
}

pub fn dyn_requires_object_safe(trait_name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        DYN_REQUIRES_OBJECT_SAFE,
        label(
            span,
            format!(
                "trait `{}` is not object-safe (slice-5 conservative: no `Self` in methods, no method generics)",
                trait_name
            ),
        ),
    )
}

pub fn protocol_arity_mismatch(
    msg: &str,
    expected: usize,
    got: usize,
    span: &SourceSpan,
) -> Diagnostic {
    Diagnostic::error(
        PROTOCOL_ARITY_MISMATCH,
        label(
            span,
            format!(
                "handler `on {}(...)` declares {} parameter(s), protocol declares {}",
                msg, got, expected
            ),
        ),
    )
}

/// v0.3 (A65): handler parameter type derived from in-body usage does not
/// unify with the protocol's declared parameter type. Reported only for
/// protocols defined in the current package (local) and in the prelude;
/// external protocols continue to emit MT2026 instead so v0.2 examples
/// keep compiling.
#[allow(clippy::too_many_arguments)]
pub fn protocol_param_type_mismatch(
    msg: &str,
    proto: &str,
    param_name: &str,
    declared: TyId,
    inferred: TyId,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let d_ty = pretty_ty(declared, arena, Some(subst), Some(defs));
    let i_ty = pretty_ty(inferred, arena, Some(subst), Some(defs));
    let mut d = Diagnostic::error(
        PROTOCOL_PARAM_TYPE_MISMATCH,
        label(
            span,
            format!(
                "handler `on {}` uses param `{}` as `{}`, but protocol `{}` declares it as `{}`",
                msg, param_name, i_ty, proto, d_ty
            ),
        ),
    );
    d.notes.push(format!(
        "the protocol declaration of `{}` is the source of truth — adjust the handler usage or the protocol",
        msg
    ));
    d
}

pub fn protocol_missing_handler(msg: &str, proto: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        PROTOCOL_MISSING_HANDLER,
        label(
            span,
            format!(
                "agent implements protocol `{}` but provides no `on {}(...)` handler",
                proto, msg
            ),
        ),
    )
}

pub fn protocol_extra_handler(msg: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        PROTOCOL_EXTRA_HANDLER,
        label(
            span,
            format!(
                "handler `on {}(...)` refers to a message not declared by any implemented protocol",
                msg
            ),
        ),
    )
}

pub fn derive_copy_field_not_copy(adt_name: &str, field: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        DERIVE_COPY_FIELD_NOT_COPY,
        label(
            span,
            format!(
                "cannot derive `Copy` for `{}`: field `{}` is not Copy",
                adt_name, field
            ),
        ),
    )
}

pub fn derive_unknown(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        DERIVE_UNKNOWN,
        label(
            span,
            format!(
                "unknown derive `{}`; v0.3 supports `Copy`, `Hash`, `Eq`, `Sendable`",
                name
            ),
        ),
    )
}

/// v0.3 (A65): cross-agent message argument violates the Sendable trait.
/// Reported at `!Msg(args)` / `?Msg(args)` call sites. The `reason`
/// argument carries the specific failure (e.g. "contains a `&T`
/// reference", "type is not Copy and not Owned").
#[allow(clippy::too_many_arguments)]
pub fn non_sendable_message_arg(
    arg_idx: usize,
    arg_ty: TyId,
    reason: &str,
    span: &SourceSpan,
    arena: &TyArena,
    subst: &Substitution,
    defs: &DefMap,
) -> Diagnostic {
    let a = pretty_ty(arg_ty, arena, Some(subst), Some(defs));
    let mut d = Diagnostic::error(
        NON_SENDABLE_MESSAGE_ARG,
        label(
            span,
            format!(
                "argument {} (`{}`) is not Sendable: {}",
                arg_idx + 1,
                a,
                reason
            ),
        ),
    );
    d.notes.push(
        "Sendable = Copy types, or owned Sized values that contain no internal references; \
         `derive(Sendable)` to opt a user struct in"
            .into(),
    );
    d
}

// v0.16 RFC-008 — user-authored row-poly fn validation diagnostics
// (MT4055..MT4059). See `mty-diagnostics::codes` for the stable code
// reservations and `mty explain MT405x` text. Each builder mirrors the
// shape of the existing closed-set effect diagnostics (`effect_undeclared`)
// for consistency.

/// MT4055 row_var_unused: the fn declares a row variable in its
/// effect clause but no fn-typed parameter exists to bind it.
pub fn row_var_unused(fn_name: &str, row_var: &str, span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        ROW_VAR_UNUSED,
        label(
            span,
            format!(
                "function `{}` declares row variable `{}` but has no \
                 closure parameter that could bind it",
                fn_name, row_var
            ),
        ),
    );
    d.notes.push(format!(
        "add a `fn(...) -> _` parameter, or drop `{}` and write a \
         concrete closed effect row",
        row_var
    ));
    d.notes
        .push("RFC-008 §inference — see `mty explain MT4055`".into());
    d
}

/// MT4057 row_var_returned_but_unbound: row var lives on the return
/// side but the fn has no fn-typed parameter from which it could be
/// inferred. Specialisation of MT4055 with a clearer fix note.
pub fn row_var_returned_but_unbound(fn_name: &str, row_var: &str, span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        ROW_VAR_RETURNED_UNBOUND,
        label(
            span,
            format!(
                "function `{}` returns effects through row variable `{}` \
                 but has no closure parameter to bind it",
                fn_name, row_var
            ),
        ),
    );
    d.notes.push(format!(
        "add a parameter of fn type (e.g. `f: fn(A) -> B`) so the row \
         variable `{}` can be bound at each call site",
        row_var
    ));
    d.notes
        .push("RFC-008 §inference — see `mty explain MT4057`".into());
    d
}

/// v0.17 — MT4056 row_var_in_concrete_only: the fn declares both a
/// concrete effects component AND a row variable, but no fn-typed
/// parameter exists to bind the row var. The concrete part is doing
/// all the work; the row var sits structurally inert. The
/// declaration is most likely a typo (`!{fs}` was intended).
///
/// Caller-side: a whole-program version that inspects every call
/// site to confirm the row var never actually binds anything is
/// deferred to v0.18; the v0.17 emit is a heuristic — "no fn-typed
/// param AND concrete effects exist" — that catches the most common
/// misuse without false positives.
pub fn row_var_in_concrete_only(fn_name: &str, row_var: &str, span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        ROW_VAR_IN_CONCRETE_ONLY,
        label(
            span,
            format!(
                "function `{}` declares row variable `{}` next to a \
                 concrete effects component, but has no closure \
                 parameter that could bind it — the row var sits \
                 structurally inert",
                fn_name, row_var
            ),
        ),
    );
    d.notes.push(format!(
        "either drop `| {}` and keep just the concrete effects, \
         or add a `fn(...) -> _` parameter so the row variable can \
         carry caller-side effects",
        row_var
    ));
    d.notes
        .push("RFC-008 §inference — see `mty explain MT4056`".into());
    d
}

/// MT4058 row_var_arity_mismatch: the caller supplied the wrong
/// number of closure args for a user row-poly fn (v0.17 active emit
/// at the call site). RFC-008 §inference.
///
/// `fn_name` is the callee's name. `expected_param_count` is how
/// many fn-typed parameters the callee declared; `got_lambda_count`
/// is how many lambda-shaped args the caller actually passed.
pub fn row_var_arity_mismatch(
    fn_name: &str,
    expected_param_count: usize,
    got_lambda_count: usize,
    span: &SourceSpan,
) -> Diagnostic {
    let mut d = Diagnostic::error(
        ROW_VAR_ARITY_MISMATCH,
        label(
            span,
            format!(
                "call to row-poly fn `{}` supplies {} closure \
                 argument(s), but the fn declares {} fn-typed \
                 parameter(s)",
                fn_name, got_lambda_count, expected_param_count
            ),
        ),
    );
    d.notes.push(format!(
        "each closure-typed parameter binds the row variable \
         independently; supply exactly {} closure(s)",
        expected_param_count
    ));
    d.notes
        .push("RFC-008 §inference — see `mty explain MT4058`".into());
    d
}

/// MT4059 row_var_subsumption_fail: the closure passed to a user
/// row-poly fn carries effects the caller's declared effect clause
/// does not allow. Caller-side analogue of MT4050 (stdlib HOFs).
/// v0.17 active emit — fires at the call site when the caller's
/// enclosing fn has a closed effect row and the row substitution
/// would add effects.
pub fn row_var_subsumption_fail(
    fn_name: &str,
    disallowed: &[String],
    span: &SourceSpan,
) -> Diagnostic {
    let joined = if disallowed.is_empty() {
        "(unspecified)".to_string()
    } else {
        disallowed.join(", ")
    };
    let mut d = Diagnostic::error(
        ROW_VAR_SUBSUMPTION_FAIL,
        label(
            span,
            format!(
                "closure passed to `{}` introduces effects {{{}}} that \
                 the enclosing fn's effect clause does not allow",
                fn_name, joined
            ),
        ),
    );
    d.notes.push(format!(
        "add `effect {}` to the enclosing fn, or use a pure closure",
        disallowed
            .first()
            .cloned()
            .unwrap_or_else(|| "<effect>".into())
    ));
    d.notes
        .push("RFC-008 row_var_subsumption_fail — see `mty explain MT4059`".into());
    d
}

/// v0.15 — MT4050 row_subsumption_fail. Emitted by the row-poly stdlib
/// HOF dispatcher (see `effects.rs::walk_expr_effects` →
/// `HirExpr::MethodCall` branch) when the closure-argument's inferred
/// effect row carries effects the caller's declared effect clause does
/// not allow.
///
/// `method` is the HOF method name (`map`, `and_then`, ...). `disallowed`
/// is the human-readable list of effects the closure produced that aren't
/// in the caller's declared set. `span` is the call-site expression span.
pub fn hof_closure_effects_rejected(
    method: &str,
    disallowed: &[String],
    span: &SourceSpan,
) -> Diagnostic {
    let joined = if disallowed.is_empty() {
        "(unspecified)".to_string()
    } else {
        disallowed.join(", ")
    };
    let mut d = Diagnostic::error(
        ROW_SUBSUMPTION_FAIL,
        label(
            span,
            format!(
                "closure passed to `{}` introduces effects {{{}}} \
                 that the enclosing fn's effect clause does not allow",
                method, joined
            ),
        ),
    );
    d.notes.push(format!(
        "add `effect {}` to the enclosing fn's signature, \
         or replace the closure with a pure one",
        disallowed
            .iter()
            .next()
            .cloned()
            .unwrap_or_else(|| "<effect>".into())
    ));
    d.notes.push(
        "RFC-008 §\"v0.14 follow-up\" row_subsumption_fail — see `mty explain MT4050`".into(),
    );
    d
}

// ----------------------------------------------------------------------
// v0.21 — Cap-name resolver diagnostics (MT4060..MT4065).
//
// Each builder converts a `cap_resolver::CapResolutionError` (or its
// raw fields, when the caller has more context than the resolver) into
// a `Diagnostic` with the correct stable code. The builders are also
// callable directly from unit tests in
// `crates/mty-types/tests/cap_resolution.rs`.
// ----------------------------------------------------------------------

use crate::cap_resolver::CapResolutionError;
use crate::ty::CapFamily;

fn pretty_family(family: &CapFamily) -> String {
    match family {
        CapFamily::Fs => "Fs".into(),
        CapFamily::Net => "Net".into(),
        CapFamily::Clock => "Clock".into(),
        CapFamily::Dom => "Dom".into(),
        CapFamily::Model => "Model".into(),
        CapFamily::Custom(s) => s.clone(),
    }
}

/// MT4060 cap_name_unbound: a capability name reference has no
/// declaration nor any active scope binding.
pub fn cap_name_unbound(name: &str, span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        CAP_NAME_UNBOUND,
        label(
            span,
            format!("capability `{}` is not declared and not in scope", name),
        ),
    );
    d.notes.push(
        "declare it at module level (`cap <Name>: <Family>`), or accept it as a handler/fn parameter"
            .into(),
    );
    d.notes
        .push("v0.21 cap-resolver — see `mty explain MT4060`".into());
    d
}

/// MT4061 cap_family_mismatch: the named cap resolves but its
/// declared family differs from the use-site's expected family.
pub fn cap_family_mismatch(
    name: &str,
    declared: &CapFamily,
    expected: &CapFamily,
    span: &SourceSpan,
) -> Diagnostic {
    let mut d = Diagnostic::error(
        CAP_FAMILY_MISMATCH,
        label(
            span,
            format!(
                "capability `{}` is declared as `{}` but used as `{}`",
                name,
                pretty_family(declared),
                pretty_family(expected),
            ),
        ),
    );
    d.notes.push(format!(
        "rename the cap or wrap the use-site in a `{}`-typed handle",
        pretty_family(expected)
    ));
    d.notes
        .push("v0.21 cap-resolver — see `mty explain MT4061`".into());
    d
}

/// MT4062 cap_scope_violation: a reference to a cap whose binding
/// frame has already been popped.
pub fn cap_scope_violation(name: &str, popped_at_depth: usize, span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        CAP_SCOPE_VIOLATION,
        label(
            span,
            format!(
                "capability `{}` is no longer in scope (popped at depth {})",
                name, popped_at_depth
            ),
        ),
    );
    d.notes.push(
        "move the use-site inside the `with cap(...) { ... }` body, or rebind the cap at the outer scope"
            .into(),
    );
    d.notes
        .push("v0.21 cap-resolver — see `mty explain MT4062`".into());
    d
}

/// MT4063 cap_redeclaration: same name declared twice in the same
/// scope frame.
pub fn cap_redeclaration(name: &str, frame_depth: usize, span: &SourceSpan) -> Diagnostic {
    let mut d = Diagnostic::error(
        CAP_REDECLARATION,
        label(
            span,
            format!(
                "capability `{}` is already declared in scope frame {}",
                name, frame_depth
            ),
        ),
    );
    d.notes.push(
        "rename one of the declarations, or scope the inner binding via `with cap(<other>) ...`"
            .into(),
    );
    d.notes
        .push("v0.21 cap-resolver — see `mty explain MT4063`".into());
    d
}

/// MT4064 cap_method_unknown: method not in the family's surface.
pub fn cap_method_unknown(
    family: &CapFamily,
    method: &str,
    available: &[String],
    span: &SourceSpan,
) -> Diagnostic {
    let mut d = Diagnostic::error(
        CAP_METHOD_UNKNOWN,
        label(
            span,
            format!(
                "no method `{}` on capability family `{}`",
                method,
                pretty_family(family)
            ),
        ),
    );
    if !available.is_empty() {
        d.notes
            .push(format!("available methods: {}", available.join(", ")));
    }
    d.notes
        .push("v0.21 cap-resolver — see `mty explain MT4064`".into());
    d
}

/// MT4065 cap_constraint_invalid: narrowing constraint not accepted
/// by family.
pub fn cap_constraint_invalid(
    family: &CapFamily,
    method: &str,
    reason: &str,
    span: &SourceSpan,
) -> Diagnostic {
    let mut d = Diagnostic::error(
        CAP_CONSTRAINT_INVALID,
        label(
            span,
            format!(
                "invalid narrowing constraint for `{}.{}`: {}",
                pretty_family(family),
                method,
                reason
            ),
        ),
    );
    d.notes
        .push("v0.21 cap-resolver — see `mty explain MT4065`".into());
    d
}

/// Convenience: convert a [`CapResolutionError`] into the
/// corresponding diagnostic. Used by the type checker's cap-resolver
/// pass to keep call-sites compact.
pub fn cap_resolution_error(err: &CapResolutionError, span: &SourceSpan) -> Diagnostic {
    match err {
        CapResolutionError::Unbound { name } => cap_name_unbound(name, span),
        CapResolutionError::FamilyMismatch {
            name,
            declared,
            expected,
        } => cap_family_mismatch(name, declared, expected, span),
        CapResolutionError::ScopeViolation {
            name,
            popped_at_depth,
        } => cap_scope_violation(name, *popped_at_depth, span),
        CapResolutionError::Redeclaration { name, frame_depth } => {
            cap_redeclaration(name, *frame_depth, span)
        }
        CapResolutionError::UnknownMethod {
            family,
            method,
            available,
        } => cap_method_unknown(family, method, available, span),
        CapResolutionError::InvalidConstraint {
            family,
            method,
            reason,
        } => cap_constraint_invalid(family, method, reason, span),
    }
}
