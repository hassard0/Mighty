//! Screen capture for `std.computer`.
//!
//! v0.30 baseline ships a [`MockScreen`] that holds an arbitrary
//! `Vec<u8>` "screenshot" plus a logical width / height. Real
//! per-platform capture (Win32 BitBlt, X11 `xcb`, `CGDisplay`) lives
//! behind the per-OS feature flags `computer-windows`, `computer-linux`,
//! `computer-macos`. The default build leaves them OFF so:
//!
//! 1. `cargo test --workspace` on CI never grabs a real display.
//! 2. The CLI builds on the OS-CI matrix without needing X11 / libxcb /
//!    Cocoa link deps.
//! 3. Production callers who actually want the OS backend opt in
//!    explicitly — same shape as `memory-sqlite` / `memory-openai`.
//!
//! The trait surface ([`ScreenBackend`]) is fixed, so once a real
//! backend lands callers can `Dispatcher::with_screen(Win32Screen::new())`
//! without touching anything else.

use std::fmt;

/// A captured screenshot — opaque image bytes plus the logical
/// dimensions the model should reason about.
///
/// The byte payload is provider-specific (PNG-encoded for the real
/// backends, arbitrary for [`MockScreen`]) — the dispatcher
/// base64-encodes it before handing it to the Anthropic Messages API
/// as an `image` content block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screenshot {
    /// Image bytes (PNG by convention; mock backends may use arbitrary
    /// payloads when only the shape matters).
    pub bytes: Vec<u8>,
    /// Logical width in pixels — the value the model is told the
    /// display is, regardless of device-pixel scaling.
    pub width: u32,
    /// Logical height in pixels — see `width`.
    pub height: u32,
    /// MIME type of `bytes`. Defaults to `image/png` for real
    /// backends; mock backends use `application/octet-stream` so a
    /// caller can detect the synthetic payload at runtime.
    pub media_type: String,
}

impl Screenshot {
    /// Build a new screenshot with explicit dimensions + media type.
    pub fn new(bytes: Vec<u8>, width: u32, height: u32, media_type: impl Into<String>) -> Self {
        Self {
            bytes,
            width,
            height,
            media_type: media_type.into(),
        }
    }

    /// Convenience: build a PNG screenshot from raw bytes (caller
    /// asserts they are PNG-encoded).
    pub fn png(bytes: Vec<u8>, width: u32, height: u32) -> Self {
        Self::new(bytes, width, height, "image/png")
    }

    /// Total byte count of the payload — useful in capture-size
    /// budget checks.
    pub fn size_bytes(&self) -> usize {
        self.bytes.len()
    }
}

/// Errors raised by [`ScreenBackend::capture`] implementations.
#[derive(Debug, thiserror::Error)]
pub enum ScreenError {
    /// The OS-level capture call failed (BitBlt returned 0, `xcb`
    /// disconnected, `CGDisplay` returned NULL, …). Carries the
    /// platform message.
    #[error("backend `{backend}` failed: {message}")]
    Backend { backend: String, message: String },

    /// The backend reported no available display (headless server,
    /// permissions denied on macOS Screen Recording, …).
    #[error("backend `{backend}` reports no display available")]
    NoDisplay { backend: String },

    /// Caller asked for a region that lies outside the framebuffer.
    #[error("region ({x},{y},{w}x{h}) is outside the {fb_w}x{fb_h} framebuffer")]
    OutOfBounds {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        fb_w: u32,
        fb_h: u32,
    },

    /// The backend was compiled out (the per-OS feature flag is not
    /// enabled and the default `MockScreen` is the only one wired in).
    #[error("backend `{0}` was compiled out; enable feature `{0}` to use it")]
    BackendDisabled(&'static str),
}

/// Object-safe screen-capture surface every backend implements.
///
/// Object-safety matters because the [`Dispatcher`](super::dispatcher::Dispatcher)
/// stores `Box<dyn ScreenBackend>` so the same agent can swap mock /
/// real backends without per-call code paths.
pub trait ScreenBackend: Send + Sync + fmt::Debug {
    /// Capture the full primary display.
    fn capture(&self) -> Result<Screenshot, ScreenError>;

    /// Capture a region of the primary display. Default impl captures
    /// the full display and slices — backends that can natively
    /// region-capture override for speed.
    fn capture_region(&self, x: u32, y: u32, w: u32, h: u32) -> Result<Screenshot, ScreenError> {
        let full = self.capture()?;
        if x + w > full.width || y + h > full.height {
            return Err(ScreenError::OutOfBounds {
                x,
                y,
                w,
                h,
                fb_w: full.width,
                fb_h: full.height,
            });
        }
        // The default cropper is a no-op on bytes (mock payloads are
        // opaque); real backends override to do an actual pixel
        // crop + re-encode.
        Ok(Screenshot {
            bytes: full.bytes,
            width: w,
            height: h,
            media_type: full.media_type,
        })
    }

    /// Logical display width — declared up front so the dispatcher can
    /// tell the model "the display is N x M pixels" without forcing a
    /// full capture first.
    fn width(&self) -> u32;

    /// Logical display height — see [`Self::width`].
    fn height(&self) -> u32;

    /// Backend identifier ("mock", "win32-bitblt", "x11-xcb",
    /// "macos-cg") — surfaced in diagnostics + telemetry.
    fn name(&self) -> &'static str;
}

/// The handle a caller actually passes around — wraps a `Box<dyn
/// ScreenBackend>` so the dispatcher can swap backends.
///
/// `Screen` is the public surface; `ScreenBackend` is the impl trait.
/// This mirrors `std.llm`'s `Client` vs `LlmProvider` split.
#[derive(Debug)]
pub struct Screen {
    backend: Box<dyn ScreenBackend>,
}

impl Screen {
    /// Wrap an arbitrary [`ScreenBackend`] in a [`Screen`].
    pub fn from_backend<B: ScreenBackend + 'static>(backend: B) -> Self {
        Self {
            backend: Box::new(backend),
        }
    }

    /// Build a [`Screen`] from an already-boxed backend.
    pub fn from_boxed(backend: Box<dyn ScreenBackend>) -> Self {
        Self { backend }
    }

    /// Capture the full primary display via the underlying backend.
    pub fn capture(&self) -> Result<Screenshot, ScreenError> {
        self.backend.capture()
    }

    /// Capture a region of the display.
    pub fn capture_region(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> Result<Screenshot, ScreenError> {
        self.backend.capture_region(x, y, w, h)
    }

    pub fn width(&self) -> u32 {
        self.backend.width()
    }

    pub fn height(&self) -> u32 {
        self.backend.height()
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }
}

/// Default mock backend. Holds a pre-baked byte buffer and reports a
/// configurable display size.
///
/// Always available — the per-OS real backends are feature-gated; the
/// mock is the only baseline guaranteed to exist on every target. CI
/// uses this exclusively.
#[derive(Debug, Clone)]
pub struct MockScreen {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    media_type: String,
}

impl MockScreen {
    /// Build a mock screen with an explicit payload + dimensions.
    pub fn new(bytes: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            bytes,
            width,
            height,
            media_type: "application/octet-stream".into(),
        }
    }

    /// Build a synthetic solid-colour screen — useful in tests where
    /// only the dimensions matter.
    ///
    /// The byte payload is `width * height * 3` bytes (RGB triples)
    /// filled with the supplied colour. Not a real PNG — it's labelled
    /// `application/octet-stream` so callers that inspect the media
    /// type can tell.
    pub fn solid_color(width: u32, height: u32, rgb: [u8; 3]) -> Self {
        let n = (width as usize) * (height as usize) * 3;
        let mut bytes = Vec::with_capacity(n);
        for _ in 0..(width as usize * height as usize) {
            bytes.push(rgb[0]);
            bytes.push(rgb[1]);
            bytes.push(rgb[2]);
        }
        Self::new(bytes, width, height)
    }

    /// Override the media type (e.g. label as `image/png` when feeding
    /// a real PNG-encoded payload).
    pub fn with_media_type(mut self, mt: impl Into<String>) -> Self {
        self.media_type = mt.into();
        self
    }
}

impl Default for MockScreen {
    fn default() -> Self {
        // 1280 x 800 solid black — matches the default cap bounds and
        // the Anthropic docs' recommended display size.
        Self::solid_color(1280, 800, [0, 0, 0])
    }
}

impl ScreenBackend for MockScreen {
    fn capture(&self) -> Result<Screenshot, ScreenError> {
        Ok(Screenshot {
            bytes: self.bytes.clone(),
            width: self.width,
            height: self.height,
            media_type: self.media_type.clone(),
        })
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_screen_default_is_1280_800() {
        let s = MockScreen::default();
        assert_eq!(s.width(), 1280);
        assert_eq!(s.height(), 800);
        assert_eq!(s.name(), "mock");
    }

    #[test]
    fn mock_screen_solid_color_payload_has_expected_size() {
        let s = MockScreen::solid_color(4, 2, [10, 20, 30]);
        let shot = s.capture().unwrap();
        assert_eq!(shot.width, 4);
        assert_eq!(shot.height, 2);
        // 4 * 2 pixels * 3 channels = 24 bytes
        assert_eq!(shot.bytes.len(), 24);
        // Every pixel is the supplied colour
        for chunk in shot.bytes.chunks(3) {
            assert_eq!(chunk, &[10, 20, 30]);
        }
    }

    #[test]
    fn mock_screen_capture_region_slices_dimensions() {
        let s = MockScreen::solid_color(100, 100, [0, 0, 0]);
        let shot = s.capture_region(10, 10, 30, 40).unwrap();
        assert_eq!(shot.width, 30);
        assert_eq!(shot.height, 40);
    }

    #[test]
    fn mock_screen_capture_region_out_of_bounds_errors() {
        let s = MockScreen::solid_color(10, 10, [0, 0, 0]);
        let err = s.capture_region(0, 0, 100, 100).unwrap_err();
        assert!(matches!(err, ScreenError::OutOfBounds { .. }));
    }

    #[test]
    fn screen_handle_round_trips_through_backend() {
        let scr = Screen::from_backend(MockScreen::solid_color(50, 60, [1, 2, 3]));
        assert_eq!(scr.width(), 50);
        assert_eq!(scr.height(), 60);
        assert_eq!(scr.backend_name(), "mock");
        let shot = scr.capture().unwrap();
        assert_eq!(shot.width, 50);
        assert_eq!(shot.height, 60);
    }

    #[test]
    fn screenshot_size_bytes_matches_payload_len() {
        let shot = Screenshot::png(vec![1, 2, 3, 4, 5], 100, 100);
        assert_eq!(shot.size_bytes(), 5);
        assert_eq!(shot.media_type, "image/png");
    }

    #[test]
    fn screenshot_new_round_trips_fields() {
        let shot = Screenshot::new(vec![9, 9, 9], 7, 8, "image/jpeg");
        assert_eq!(shot.bytes, vec![9, 9, 9]);
        assert_eq!(shot.width, 7);
        assert_eq!(shot.height, 8);
        assert_eq!(shot.media_type, "image/jpeg");
    }

    #[test]
    fn mock_screen_with_media_type_overrides_default() {
        let s = MockScreen::solid_color(1, 1, [0; 3]).with_media_type("image/png");
        let shot = s.capture().unwrap();
        assert_eq!(shot.media_type, "image/png");
    }
}
