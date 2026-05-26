//! `std.web.Canvas` — opaque handle on the host's 2D drawing context.
//!
//! See `crates/mty-codegen-wasm/wit/mty-web/canvas.wit` for the WIT
//! shape these methods lower to. On `wasm32-web` each method becomes
//! a single canonical-ABI call into the corresponding `mty:web/canvas`
//! import. On native (the `mty run` JIT path) the methods are no-ops
//! that record the call into an in-memory log so `std.test` agents can
//! still assert call ordering without touching a browser.
//!
//! ## Color packing
//!
//! `color: u32` follows the WIT contract — `0xRRGGBBAA`. The host shim
//! converts to a `rgba(...)` CSS string before calling into the
//! Canvas2D context; on native we just store the raw u32 in the call
//! log.

use std::sync::Mutex;

/// One recorded canvas call. Used by the native fallback so tests can
/// assert call ordering without instantiating a real browser host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasCall {
    Clear,
    FillRect {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        color: u32,
    },
    StrokeRect {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        color: u32,
    },
    FillText {
        text: String,
        x: i32,
        y: i32,
        color: u32,
    },
    SetFillStyle(u32),
    RequestAnimationFrame,
}

/// Opaque Mighty-side handle on the host's 2D drawing context.
///
/// Constructed by the host glue (browser side: from
/// `HTMLCanvasElement.getContext("2d")`; native side: an empty
/// recorder). The guest treats it as `resource canvas { … }`.
#[derive(Debug, Default)]
pub struct Canvas {
    width: u32,
    height: u32,
    calls: Mutex<Vec<CanvasCall>>,
}

impl Canvas {
    /// Construct a native-fallback canvas with the given backing
    /// dimensions. The browser-target codegen never calls this — it
    /// emits a direct `mty:web/canvas` import that lands the handle
    /// straight in the guest's local table.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Canvas width in CSS pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Canvas height in CSS pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Clear the entire backing surface to transparent black.
    pub fn clear(&self) {
        self.record(CanvasCall::Clear);
    }

    /// Fill an axis-aligned rectangle with `color`.
    pub fn fill_rect(&self, x: i32, y: i32, w: u32, h: u32, color: u32) {
        self.record(CanvasCall::FillRect { x, y, w, h, color });
    }

    /// Stroke (1px line width) an axis-aligned rectangle with `color`.
    pub fn stroke_rect(&self, x: i32, y: i32, w: u32, h: u32, color: u32) {
        self.record(CanvasCall::StrokeRect { x, y, w, h, color });
    }

    /// Render `text` with the current font at baseline `(x, y)`.
    pub fn fill_text(&self, text: impl Into<String>, x: i32, y: i32, color: u32) {
        self.record(CanvasCall::FillText {
            text: text.into(),
            x,
            y,
            color,
        });
    }

    /// Persist `color` as the default fill style on the host context.
    pub fn set_fill_style(&self, color: u32) {
        self.record(CanvasCall::SetFillStyle(color));
    }

    /// Ask the host to schedule one animation-frame callback.
    pub fn request_animation_frame(&self) {
        self.record(CanvasCall::RequestAnimationFrame);
    }

    /// Drain the recorded call log. Tests use this to assert ordering;
    /// the browser-target codegen never calls this.
    pub fn drain_calls(&self) -> Vec<CanvasCall> {
        let mut guard = self.calls.lock().expect("canvas calls mutex poisoned");
        std::mem::take(&mut *guard)
    }

    fn record(&self, call: CanvasCall) {
        if let Ok(mut g) = self.calls.lock() {
            g.push(call);
        }
    }
}

/// Canonical WIT import name for `canvas.clear`. Pinned here so the
/// codegen-wasm crate and any external test harness pattern-match on
/// the same string.
pub const WIT_IMPORT_CLEAR: (&str, &str) = ("mty:web/canvas@0.1", "clear");
/// Canonical WIT import name for `canvas.fill-rect`.
pub const WIT_IMPORT_FILL_RECT: (&str, &str) = ("mty:web/canvas@0.1", "fill-rect");
/// Canonical WIT import name for `canvas.stroke-rect`.
pub const WIT_IMPORT_STROKE_RECT: (&str, &str) = ("mty:web/canvas@0.1", "stroke-rect");
/// Canonical WIT import name for `canvas.fill-text`.
pub const WIT_IMPORT_FILL_TEXT: (&str, &str) = ("mty:web/canvas@0.1", "fill-text");
/// Canonical WIT import name for `canvas.set-fill-style`.
pub const WIT_IMPORT_SET_FILL_STYLE: (&str, &str) = ("mty:web/canvas@0.1", "set-fill-style");
/// Canonical WIT import name for `canvas.width`.
pub const WIT_IMPORT_WIDTH: (&str, &str) = ("mty:web/canvas@0.1", "width");
/// Canonical WIT import name for `canvas.height`.
pub const WIT_IMPORT_HEIGHT: (&str, &str) = ("mty:web/canvas@0.1", "height");
/// Canonical WIT import name for `canvas.request-animation-frame`.
pub const WIT_IMPORT_REQUEST_ANIMATION_FRAME: (&str, &str) =
    ("mty:web/canvas@0.1", "request-animation-frame");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_canvas_records_dimensions() {
        let c = Canvas::new(240, 480);
        assert_eq!(c.width(), 240);
        assert_eq!(c.height(), 480);
        // No calls recorded yet.
        assert!(c.drain_calls().is_empty());
    }

    #[test]
    fn fill_rect_records_call() {
        let c = Canvas::new(240, 480);
        c.fill_rect(0, 0, 240, 480, 0x1d_22_30_ff);
        let calls = c.drain_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            CanvasCall::FillRect {
                x: 0,
                y: 0,
                w: 240,
                h: 480,
                color: 0x1d_22_30_ff
            }
        );
    }

    #[test]
    fn fill_text_records_call() {
        let c = Canvas::new(640, 480);
        c.fill_text("hello", 12, 34, 0xff_ff_ff_ff);
        let calls = c.drain_calls();
        assert_eq!(calls.len(), 1);
        assert!(matches!(
            &calls[0],
            CanvasCall::FillText { text, x: 12, y: 34, color: 0xff_ff_ff_ff } if text == "hello"
        ));
    }

    #[test]
    fn drain_clears_log() {
        let c = Canvas::new(640, 480);
        c.clear();
        c.clear();
        assert_eq!(c.drain_calls().len(), 2);
        assert!(c.drain_calls().is_empty());
    }

    #[test]
    fn raf_records_call() {
        let c = Canvas::new(640, 480);
        c.request_animation_frame();
        assert_eq!(c.drain_calls(), vec![CanvasCall::RequestAnimationFrame]);
    }

    #[test]
    fn import_constants_are_canonical() {
        assert_eq!(WIT_IMPORT_CLEAR.0, "mty:web/canvas@0.1");
        assert_eq!(WIT_IMPORT_FILL_RECT.1, "fill-rect");
        assert_eq!(WIT_IMPORT_STROKE_RECT.1, "stroke-rect");
        assert_eq!(WIT_IMPORT_FILL_TEXT.1, "fill-text");
        assert_eq!(WIT_IMPORT_SET_FILL_STYLE.1, "set-fill-style");
        assert_eq!(WIT_IMPORT_WIDTH.1, "width");
        assert_eq!(WIT_IMPORT_HEIGHT.1, "height");
        assert_eq!(
            WIT_IMPORT_REQUEST_ANIMATION_FRAME.1,
            "request-animation-frame"
        );
    }
}
