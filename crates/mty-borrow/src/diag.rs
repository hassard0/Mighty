//! SD3xxx diagnostic constructors for the borrow checker.

use mty_diagnostics::{codes::*, Diagnostic, Label};
use mty_hir::SourceSpan;

fn label(span: &SourceSpan, msg: impl Into<String>) -> Label {
    Label {
        start: span.start as usize,
        end: span.end as usize,
        message: msg.into(),
    }
}

pub fn use_after_move(name: &str, span: &SourceSpan, moved_at: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        USE_AFTER_MOVE,
        label(span, format!("use of moved value `{}`", name)),
    )
    .with_secondary(Label {
        start: moved_at.start as usize,
        end: moved_at.end as usize,
        message: "value moved here".into(),
    })
}

pub fn move_out_of_borrow(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        MOVE_OUT_OF_BORROW,
        label(
            span,
            format!("cannot move out of `{}` because it is borrowed", name),
        ),
    )
}

pub fn borrow_after_move(name: &str, span: &SourceSpan, moved_at: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        BORROW_AFTER_MOVE,
        label(span, format!("cannot borrow `{}` after it was moved", name)),
    )
    .with_secondary(Label {
        start: moved_at.start as usize,
        end: moved_at.end as usize,
        message: "value moved here".into(),
    })
}

pub fn mut_borrow_while_shared(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        MUT_BORROW_WHILE_SHARED,
        label(
            span,
            format!(
                "cannot borrow `{}` as mutable while shared borrows exist",
                name
            ),
        ),
    )
}

pub fn shared_borrow_while_mut(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        SHARED_BORROW_WHILE_MUT,
        label(
            span,
            format!(
                "cannot borrow `{}` as shared while a mutable borrow is live",
                name
            ),
        ),
    )
}

pub fn two_mut_borrows(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        TWO_MUT_BORROWS,
        label(span, format!("`{}` is already mutably borrowed", name)),
    )
}

pub fn borrow_outlives_owner(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        BORROW_OUTLIVES_OWNER,
        label(span, format!("borrow of `{}` outlives its owner", name)),
    )
}

pub fn cannot_move_borrowed(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        CANNOT_MOVE_BORROWED,
        label(span, format!("cannot move `{}` while it is borrowed", name)),
    )
}

pub fn move_out_of_ref(span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        MOVE_OUT_OF_REF,
        label(span, "cannot move out of a reference"),
    )
}

/// v0.3 (A56): precise SD3009 with the ref expression's pretty name.
pub fn move_out_of_ref_named(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        MOVE_OUT_OF_REF,
        label(
            span,
            format!(
                "cannot move out of `*{}`: dereferencing a reference does not transfer ownership",
                name
            ),
        ),
    )
}

/// v0.3 (A54): conflict with field-level resolution.
pub fn mut_borrow_while_shared_place(place: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        MUT_BORROW_WHILE_SHARED,
        label(
            span,
            format!(
                "cannot borrow `{}` as mutable while shared borrows of an overlapping place exist",
                place
            ),
        ),
    )
}

/// v0.3 (A54): conflict with field-level resolution.
pub fn shared_borrow_while_mut_place(place: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        SHARED_BORROW_WHILE_MUT,
        label(
            span,
            format!(
                "cannot borrow `{}` as shared while a mutable borrow of an overlapping place is live",
                place
            ),
        ),
    )
}

/// v0.3 (A54): two-mut conflict with field-level resolution.
pub fn two_mut_borrows_place(place: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        TWO_MUT_BORROWS,
        label(
            span,
            format!(
                "`{}` (or an overlapping place) is already mutably borrowed",
                place
            ),
        ),
    )
}

pub fn arena_escape(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        ARENA_ESCAPE,
        label(
            span,
            format!(
                "value `{}` allocated in the arena cannot escape it; promote with `move` first",
                name
            ),
        ),
    )
}

pub fn non_sendable_message_arg(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        NON_SENDABLE_MESSAGE_ARG,
        label(
            span,
            format!(
                "argument `{}` to a cross-agent message is not Sendable",
                name
            ),
        ),
    )
}

pub fn mut_borrow_of_immut_local(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        MUT_BORROW_OF_IMMUT_LOCAL,
        label(
            span,
            format!(
                "cannot mutably borrow immutable local `{}` (add `mut` to its binding)",
                name
            ),
        ),
    )
}

pub fn assign_to_immut_local(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        ASSIGN_TO_IMMUT_LOCAL,
        label(span, format!("cannot assign to immutable local `{}`", name)),
    )
}

pub fn use_of_uninitialized(name: &str, span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        USE_OF_UNINITIALIZED,
        label(
            span,
            format!("use of possibly uninitialized binding `{}`", name),
        ),
    )
}
