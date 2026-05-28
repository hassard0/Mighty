//! v0.32 Track A — breakpoint hook trait + line→PC mapping.
//!
//! The DAP server (in `mty-cli`) wraps the SIR interpreter in a
//! "debug session" loop where every step is preceded by a check
//! against an installed [`BreakpointHook`]. The interpreter calls
//! `should_break` before executing each `Stmt` or terminator; the
//! hook returns `BreakDecision::Break` to suspend the program, or
//! `BreakDecision::Continue` to let it run.
//!
//! Spans → lines: the IR doesn't store lines directly, only byte
//! offsets via [`crate::ir::FnSpanTable`]. The DAP server keeps a
//! per-file source-text cache and translates byte offsets to lines
//! before invoking the hook, so the interpreter itself only sees a
//! `(fn_id, block_idx, stmt_idx, span_start, span_end)` tuple.
//!
//! Function breakpoints (`fn:name` / `agent:Name`) are handled at the
//! call site by the DAP server, which intercepts `Stmt::Assign` of
//! `Rvalue::Call { func: FnRef::User(_) }` shapes via a parallel
//! "intercept" hook — see [`BreakpointHook::on_call`] below.

use crate::ir::{BlockId, IrFnId};
use mty_hir::SourceSpan;

/// What the interpreter should do when a hook fires at a given
/// step boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakDecision {
    /// Resume execution as normal.
    Continue,
    /// Suspend execution. The interpreter returns control to the
    /// caller (the DAP loop) via [`super::run::StepResult::Suspended`].
    Break,
}

/// Position the interpreter is about to execute.
#[derive(Debug, Clone)]
pub struct StepPosition {
    pub fn_id: IrFnId,
    pub block: BlockId,
    /// Statement index inside the block. `None` indicates the
    /// terminator (i.e. `pc == block.stmts.len()`).
    pub stmt_idx: Option<usize>,
    /// Byte-offset span of the statement / terminator, or
    /// `SourceSpan { 0, 0 }` if no span was recorded.
    pub span: SourceSpan,
}

/// Trait the DAP server implements. The interpreter calls these
/// methods at well-defined step boundaries; the hook persists across
/// invocations of `run_fn_with_breakpoints` so the DAP server can
/// thread its own state (line breakpoints, function breakpoints,
/// step mode, current call depth) across resumptions.
pub trait BreakpointHook: Send {
    /// Called before each `Stmt` and terminator. The interpreter
    /// passes the upcoming position; the hook returns a decision.
    ///
    /// Default: always continue. Implementors override to surface
    /// breakpoint hits.
    fn before_step(&mut self, _pos: &StepPosition, _depth: usize) -> BreakDecision {
        BreakDecision::Continue
    }

    /// Called when the interpreter is about to enter a user-defined
    /// fn (i.e. a `Rvalue::Call { func: FnRef::User(id) }`). The hook
    /// can intercept by returning `BreakDecision::Break`; the
    /// interpreter then suspends *before* pushing the call frame so
    /// the caller can inspect the about-to-be-called fn.
    fn on_call(&mut self, _callee: IrFnId, _depth: usize) -> BreakDecision {
        BreakDecision::Continue
    }

    /// Called when a frame returns. Used by the DAP server's
    /// step-out logic.
    fn on_return(&mut self, _depth: usize) -> BreakDecision {
        BreakDecision::Continue
    }
}

/// No-op default hook. Plug in via
/// [`super::run::run_fn_with_breakpoints`] when you don't actually
/// want any breakpoints (e.g. for the non-DAP test path).
pub struct NullHook;
impl BreakpointHook for NullHook {}

/// Helper: convert a byte offset to a 1-based line number using the
/// supplied source text. Used by the DAP server to materialise
/// per-stmt positions in DAP terms.
///
/// Returns `1` for offsets past EOF (DAP requires 1-based lines and
/// rejecting `0`).
pub fn offset_to_line(src: &str, offset: u32) -> u32 {
    let off = offset as usize;
    let bytes = src.as_bytes();
    let cap = bytes.len().min(off);
    let mut line: u32 = 1;
    let mut i = 0;
    while i < cap {
        if bytes[i] == b'\n' {
            line += 1;
        }
        i += 1;
    }
    line
}

/// Helper: convert a byte offset to a (line, column) tuple, both
/// 1-based. Column counts UTF-8 code units (DAP convention is UTF-16
/// code units, but VS Code accepts either when the adapter declares
/// `columnsStartAt1`).
pub fn offset_to_line_col(src: &str, offset: u32) -> (u32, u32) {
    let off = offset as usize;
    let bytes = src.as_bytes();
    let cap = bytes.len().min(off);
    let mut line: u32 = 1;
    let mut col: u32 = 1;
    let mut i = 0;
    while i < cap {
        if bytes[i] == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
        i += 1;
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_to_line_simple() {
        let src = "line1\nline2\nline3";
        assert_eq!(offset_to_line(src, 0), 1);
        assert_eq!(offset_to_line(src, 5), 1);
        assert_eq!(offset_to_line(src, 6), 2);
        assert_eq!(offset_to_line(src, 12), 3);
    }

    #[test]
    fn offset_to_line_at_eof() {
        let src = "a\nb\n";
        assert_eq!(offset_to_line(src, 999), 3);
    }

    #[test]
    fn offset_to_line_col_simple() {
        let src = "abc\ndefg\nhij";
        assert_eq!(offset_to_line_col(src, 0), (1, 1));
        assert_eq!(offset_to_line_col(src, 2), (1, 3));
        assert_eq!(offset_to_line_col(src, 4), (2, 1));
        assert_eq!(offset_to_line_col(src, 7), (2, 4));
        assert_eq!(offset_to_line_col(src, 9), (3, 1));
    }

    #[test]
    fn null_hook_always_continues() {
        let mut h = NullHook;
        let pos = StepPosition {
            fn_id: IrFnId(0),
            block: BlockId(0),
            stmt_idx: Some(0),
            span: SourceSpan { start: 0, end: 0 },
        };
        assert_eq!(h.before_step(&pos, 0), BreakDecision::Continue);
        assert_eq!(h.on_call(IrFnId(1), 0), BreakDecision::Continue);
        assert_eq!(h.on_return(0), BreakDecision::Continue);
    }

    /// Custom hook that breaks at a specific line — matches what the
    /// DAP server will install at runtime.
    struct LineBreakHook {
        target_line: u32,
        src: String,
        fired: bool,
    }
    impl BreakpointHook for LineBreakHook {
        fn before_step(&mut self, pos: &StepPosition, _depth: usize) -> BreakDecision {
            let line = offset_to_line(&self.src, pos.span.start);
            if line == self.target_line {
                self.fired = true;
                BreakDecision::Break
            } else {
                BreakDecision::Continue
            }
        }
    }

    #[test]
    fn line_break_hook_fires_at_target() {
        let src = "fn main() {\n  let x = 1\n  let y = 2\n}\n".to_string();
        let mut h = LineBreakHook {
            target_line: 3,
            src,
            fired: false,
        };
        // Simulate a step at the byte offset for "let y = 2" (after
        // line 2's trailing newline at byte 25).
        let pos = StepPosition {
            fn_id: IrFnId(0),
            block: BlockId(0),
            stmt_idx: Some(1),
            span: SourceSpan { start: 26, end: 35 },
        };
        let dec = h.before_step(&pos, 0);
        assert_eq!(dec, BreakDecision::Break);
        assert!(h.fired);
    }
}
