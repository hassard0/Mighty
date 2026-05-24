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
    let mut d = Diagnostic::error(
        NON_EXHAUSTIVE_MATCH,
        label(span, "non-exhaustive match (warning)"),
    );
    d.severity = Severity::Warning;
    if !missing.is_empty() {
        d.notes
            .push(format!("missing pattern(s): {}", missing.join(", ")));
    }
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
