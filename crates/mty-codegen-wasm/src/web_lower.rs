//! v0.24 — wasm32-web canvas lowering.
//!
//! Owns the per-`Emitter` state that maps each `CanvasOpKind` to its
//! lazily-declared `mty:web/canvas@0.1` core-wasm import index, plus
//! the helper that pushes the right argument tuple and emits the
//! `call` instruction.
//!
//! Factoring this out of `emit.rs` keeps the canvas-import bookkeeping
//! self-contained and lets the v0.24 test harness exercise it
//! directly without re-spinning the whole `Emitter`.
//!
//! ## Design
//!
//! - One [`CanvasImports`] lives on the `Emitter` for the duration of
//!   the module emission. Each `CanvasOpKind` slot is `None` until the
//!   first `BuiltinId::CanvasOp(kind)` call site triggers
//!   [`CanvasImports::ensure`], which declares the matching import in
//!   the emitter's `ImportSection` and records the resulting fn index.
//! - Repeated calls to the same op reuse the cached index — exactly
//!   the convention the v0.5 DOM lowering uses (see
//!   `Emitter::dom_set_text_idx` & friends).
//! - The canonical-ABI signatures match `wit/mty-web/canvas.wit`. Both
//!   `s32` (signed) and `u32` (unsigned) WIT params lower to wasm
//!   `i32` at the flat layer; the host shim is responsible for
//!   reinterpreting the bits. String params (`fill-text`) lower to
//!   the `(ptr, len)` pair convention shared with `log`.

use mty_ir::ir::CanvasOpKind;
use wasm_encoder::{EntityType, ImportSection, ValType};

use crate::emit::TySigPub;

/// Canonical module name for the canvas interface as it appears in
/// the embedded core module's import section.
///
/// Note: the public `mty:web/canvas@0.1` constants pinned by Track A
/// in `crates/mty-stdlib/src/web/canvas.rs` describe the *WIT
/// contract* (which lives in package `mty:web@0.1`). The string we
/// emit into the wasm import is the unversioned form
/// `mty:web/canvas` so the wit-component resolver — which sees the
/// host stub block as `package mty:web { ... }` (unversioned to keep
/// back-compat with the v0.5 DOM imports) — can wire the imports
/// against the matching interface in that stub package.
///
/// In practice this is a no-op rename: wit-component canonicalises
/// both forms into the same `(interface mty:web/canvas)` reference
/// when emitting the Component world; the only place the version
/// matters is in the WIT *text*, which the stub block already pins
/// at the right shape.
pub const CANVAS_MODULE: &str = "mty:web/canvas";

/// Per-`Emitter` state for the eight canvas imports. Lazily allocates
/// a core-wasm import slot the first time a function body references a
/// given op; subsequent calls reuse the cached index.
#[derive(Debug, Default)]
pub struct CanvasImports {
    clear: Option<u32>,
    fill_rect: Option<u32>,
    stroke_rect: Option<u32>,
    fill_text: Option<u32>,
    set_fill_style: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    request_animation_frame: Option<u32>,
}

impl CanvasImports {
    /// Look up the cached fn-index for `op`, if it has been declared.
    pub fn get(&self, op: CanvasOpKind) -> Option<u32> {
        match op {
            CanvasOpKind::Clear => self.clear,
            CanvasOpKind::FillRect => self.fill_rect,
            CanvasOpKind::StrokeRect => self.stroke_rect,
            CanvasOpKind::FillText => self.fill_text,
            CanvasOpKind::SetFillStyle => self.set_fill_style,
            CanvasOpKind::Width => self.width,
            CanvasOpKind::Height => self.height,
            CanvasOpKind::RequestAnimationFrame => self.request_animation_frame,
        }
    }

    fn set(&mut self, op: CanvasOpKind, idx: u32) {
        match op {
            CanvasOpKind::Clear => self.clear = Some(idx),
            CanvasOpKind::FillRect => self.fill_rect = Some(idx),
            CanvasOpKind::StrokeRect => self.stroke_rect = Some(idx),
            CanvasOpKind::FillText => self.fill_text = Some(idx),
            CanvasOpKind::SetFillStyle => self.set_fill_style = Some(idx),
            CanvasOpKind::Width => self.width = Some(idx),
            CanvasOpKind::Height => self.height = Some(idx),
            CanvasOpKind::RequestAnimationFrame => self.request_animation_frame = Some(idx),
        }
    }
}

/// Core-Wasm signature for `op`, post-canonical-ABI flattening.
///
/// `s32` + `u32` collapse to `i32`; `string` becomes a `(ptr, len)`
/// pair of `i32`s pushed on the stack before the call (caller's
/// responsibility — see `emit_call` in `emit.rs`).
pub fn canvas_signature(op: CanvasOpKind) -> (Vec<ValType>, Vec<ValType>) {
    match op {
        // No-arg, void.
        CanvasOpKind::Clear | CanvasOpKind::RequestAnimationFrame => (vec![], vec![]),
        // (x:s32, y:s32, w:u32, h:u32, color:u32) -> ().
        CanvasOpKind::FillRect | CanvasOpKind::StrokeRect => (
            vec![
                ValType::I32, // x
                ValType::I32, // y
                ValType::I32, // w
                ValType::I32, // h
                ValType::I32, // color
            ],
            vec![],
        ),
        // (text:string=(ptr,len), x:s32, y:s32, color:u32) -> ().
        CanvasOpKind::FillText => (
            vec![
                ValType::I32, // text-ptr
                ValType::I32, // text-len
                ValType::I32, // x
                ValType::I32, // y
                ValType::I32, // color
            ],
            vec![],
        ),
        // (color:u32) -> ().
        CanvasOpKind::SetFillStyle => (vec![ValType::I32], vec![]),
        // () -> u32.
        CanvasOpKind::Width | CanvasOpKind::Height => (vec![], vec![ValType::I32]),
    }
}

/// Declare the import for `op` in `imports`, returning its fn-index
/// (allocated from `import_count`). On the second call for the same
/// op the cached index is returned without touching the import
/// section.
///
/// `intern_sig` is the emitter's existing signature-deduping callback
/// so we don't append duplicate type entries; `import_count` is
/// `&mut` because each newly-declared import bumps it.
///
/// Returns the fn-index of the import (stable for the rest of module
/// emission).
pub fn ensure_canvas_import<F>(
    state: &mut CanvasImports,
    imports: &mut ImportSection,
    import_count: &mut u32,
    mut intern_sig: F,
    op: CanvasOpKind,
) -> u32
where
    F: FnMut(TySigPub) -> u32,
{
    if let Some(idx) = state.get(op) {
        return idx;
    }
    let (params, results) = canvas_signature(op);
    let ty_idx = intern_sig(TySigPub { params, results });
    imports.import(
        CANVAS_MODULE,
        op.as_wit_method(),
        EntityType::Function(ty_idx),
    );
    let idx = *import_count;
    *import_count += 1;
    state.set(op, idx);
    idx
}

/// Returns `true` iff `name` should appear in the wasm32-web core
/// module's export section.
///
/// v0.23: canonical callback names (`frame`, `keydown`, `keyup`).
/// v0.26 Track D: ALSO export any user fn whose name is a valid
/// non-private wasm identifier (starts with an ASCII letter and isn't
/// `main`/`cabi_realloc`/`memory` — those are handled separately).
/// The permissive shape lets agent-driven web programs expose
/// arbitrary host-callable entry points (e.g.
/// `inst.exports.dispatch_message(...)`) without first carving out a
/// new keyword. Names starting with `_` or `__` stay private to
/// preserve the v0.24 surface promise that "underscore-prefixed fns
/// are implementation details".
pub fn is_web_callback_export(name: &str) -> bool {
    if name == "main" || name == "cabi_realloc" || name == "memory" {
        return false;
    }
    if name.starts_with('_') {
        return false;
    }
    // Must be a plausible wasm identifier — at minimum a non-empty
    // string starting with an ASCII letter.
    name.chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_match_wit_shape() {
        // Spot-check against `wit/mty-web/canvas.wit`.
        assert_eq!(canvas_signature(CanvasOpKind::Clear), (vec![], vec![]));
        assert_eq!(
            canvas_signature(CanvasOpKind::FillRect).0.len(),
            5,
            "fill-rect takes 5 args"
        );
        assert_eq!(canvas_signature(CanvasOpKind::Width).1, vec![ValType::I32]);
        assert_eq!(
            canvas_signature(CanvasOpKind::FillText).0.len(),
            5,
            "fill-text: (ptr,len,x,y,color) = 5 i32s"
        );
        assert_eq!(
            canvas_signature(CanvasOpKind::SetFillStyle).0,
            vec![ValType::I32]
        );
    }

    #[test]
    fn callback_export_names() {
        // Canonical callback names always export.
        assert!(is_web_callback_export("frame"));
        assert!(is_web_callback_export("keydown"));
        assert!(is_web_callback_export("keyup"));
        // Reserved infrastructure names never export through this
        // helper (they're handled by separate emit logic).
        assert!(!is_web_callback_export("main"));
        assert!(!is_web_callback_export("cabi_realloc"));
        assert!(!is_web_callback_export("memory"));
        // v0.26 Track D — non-reserved user fns DO export, so a host
        // can drive `inst.exports.dispatch_message(...)` without
        // first inventing a new canonical-name keyword.
        assert!(is_web_callback_export("frame_helper"));
        assert!(is_web_callback_export("dispatch_message"));
        // Underscore-prefixed names stay private.
        assert!(!is_web_callback_export("_internal"));
        assert!(!is_web_callback_export("__hidden"));
    }

    #[test]
    fn cache_round_trip() {
        let mut state = CanvasImports::default();
        assert!(state.get(CanvasOpKind::FillRect).is_none());
        state.set(CanvasOpKind::FillRect, 7);
        assert_eq!(state.get(CanvasOpKind::FillRect), Some(7));
        // Other slots stay None.
        assert!(state.get(CanvasOpKind::Clear).is_none());
    }
}
