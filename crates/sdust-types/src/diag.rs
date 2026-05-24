//! Diagnostic constructors for the type checker. Each function builds a
//! `Diagnostic` with the appropriate code, message, and severity.

use crate::defs::DefMap;
use crate::infer::Substitution;
use crate::ty::{pretty_ty, TyArena, TyId};
use sdust_diagnostics::{codes::*, Diagnostic, Label, Severity};
use sdust_hir::SourceSpan;

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
                "unknown derive `{}`; slice 5 supports `Copy`, `Hash`, `Eq`",
                name
            ),
        ),
    )
}
