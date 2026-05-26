//! `std.web.Input` + `std.web.Key` — keyboard event decoder.
//!
//! See `crates/mty-codegen-wasm/wit/mty-web/input.wit` for the WIT
//! shape these methods lower to. The Mighty surface is:
//!
//! ```mty
//! use std.web
//! agent Game(canvas: Canvas, input: Input) {
//!   on KeyDown(k: Key) {
//!     match k {
//!       Key.ArrowLeft  => ...,
//!       Key.ArrowRight => ...,
//!       Key.Space      => ...,
//!       _              => (),
//!     }
//!   }
//! }
//! ```
//!
//! The host shim pushes raw `KeyboardEvent.key` strings into the
//! exported `keydown(k: string)` / `keyup(k: string)` callbacks; the
//! generated agent stub calls [`Key::from_dom_string`] to decode
//! into a `Key` before invoking the user-written handler.

use std::sync::Mutex;

/// Mighty-side decoded keyboard event. Maps the common arrow / space
/// / single-char DOM strings into a typed enum so guests don't have
/// to match on string literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Space,
    Enter,
    Escape,
    /// A single printable character (`"a"`, `"Z"`, `"7"`, …).
    Char(char),
    /// Any other named key (`"F1"`, `"Shift"`, `"Control"`, …).
    Other(String),
}

impl Key {
    /// Decode a raw `KeyboardEvent.key` string into a `Key`.
    ///
    /// Single-character strings become `Key::Char(c)`; named keys with
    /// a dedicated variant land there; everything else falls through
    /// to `Key::Other`.
    pub fn from_dom_string(s: &str) -> Self {
        match s {
            "ArrowLeft" => Key::ArrowLeft,
            "ArrowRight" => Key::ArrowRight,
            "ArrowUp" => Key::ArrowUp,
            "ArrowDown" => Key::ArrowDown,
            " " | "Space" | "Spacebar" => Key::Space,
            "Enter" => Key::Enter,
            "Escape" | "Esc" => Key::Escape,
            other => {
                let mut chars = other.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Key::Char(c),
                    _ => Key::Other(other.to_string()),
                }
            }
        }
    }

    /// Mirror of [`Self::from_dom_string`] — render `Self` back into
    /// the DOM-canonical string form. Used by tests and by the
    /// host-shim's `keydown(k)` round-trip path.
    pub fn to_dom_string(&self) -> String {
        match self {
            Key::ArrowLeft => "ArrowLeft".into(),
            Key::ArrowRight => "ArrowRight".into(),
            Key::ArrowUp => "ArrowUp".into(),
            Key::ArrowDown => "ArrowDown".into(),
            Key::Space => " ".into(),
            Key::Enter => "Enter".into(),
            Key::Escape => "Escape".into(),
            Key::Char(c) => c.to_string(),
            Key::Other(s) => s.clone(),
        }
    }
}

/// Recorded `Input` subscription. The native fallback records every
/// `subscribe_*` call so `std.test` agents can assert the guest
/// asked for the right event streams; the browser-target codegen
/// lowers each method to a direct WIT import and never touches this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputCall {
    SubscribeKeyDown,
    SubscribeKeyUp,
}

/// Opaque Mighty-side handle on the host's keyboard event stream.
#[derive(Debug, Default)]
pub struct Input {
    calls: Mutex<Vec<InputCall>>,
}

impl Input {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the host to start delivering keydown callbacks.
    pub fn subscribe_keydown(&self) {
        self.record(InputCall::SubscribeKeyDown);
    }

    /// Ask the host to start delivering keyup callbacks.
    pub fn subscribe_keyup(&self) {
        self.record(InputCall::SubscribeKeyUp);
    }

    /// Drain the recorded subscription log. See
    /// [`crate::web::canvas::Canvas::drain_calls`] for the rationale.
    pub fn drain_calls(&self) -> Vec<InputCall> {
        let mut guard = self.calls.lock().expect("input calls mutex poisoned");
        std::mem::take(&mut *guard)
    }

    fn record(&self, call: InputCall) {
        if let Ok(mut g) = self.calls.lock() {
            g.push(call);
        }
    }
}

/// Canonical WIT import name for `input.subscribe-keydown`.
pub const WIT_IMPORT_SUBSCRIBE_KEYDOWN: (&str, &str) = ("mty:web/input@0.1", "subscribe-keydown");
/// Canonical WIT import name for `input.subscribe-keyup`.
pub const WIT_IMPORT_SUBSCRIBE_KEYUP: (&str, &str) = ("mty:web/input@0.1", "subscribe-keyup");
/// Canonical WIT export name for the host → guest `keydown` callback.
pub const WIT_EXPORT_KEYDOWN: &str = "keydown";
/// Canonical WIT export name for the host → guest `keyup` callback.
pub const WIT_EXPORT_KEYUP: &str = "keyup";
/// Canonical WIT export name for the host → guest `frame` callback.
pub const WIT_EXPORT_FRAME: &str = "frame";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_keys_decode() {
        assert_eq!(Key::from_dom_string("ArrowLeft"), Key::ArrowLeft);
        assert_eq!(Key::from_dom_string("ArrowRight"), Key::ArrowRight);
        assert_eq!(Key::from_dom_string("ArrowUp"), Key::ArrowUp);
        assert_eq!(Key::from_dom_string("ArrowDown"), Key::ArrowDown);
    }

    #[test]
    fn space_variants_collapse() {
        assert_eq!(Key::from_dom_string(" "), Key::Space);
        assert_eq!(Key::from_dom_string("Space"), Key::Space);
        assert_eq!(Key::from_dom_string("Spacebar"), Key::Space);
    }

    #[test]
    fn single_char_decodes_to_char() {
        assert_eq!(Key::from_dom_string("a"), Key::Char('a'));
        assert_eq!(Key::from_dom_string("Z"), Key::Char('Z'));
        assert_eq!(Key::from_dom_string("7"), Key::Char('7'));
    }

    #[test]
    fn unknown_keys_become_other() {
        assert_eq!(Key::from_dom_string("F1"), Key::Other("F1".into()));
        assert_eq!(Key::from_dom_string("Shift"), Key::Other("Shift".into()));
    }

    #[test]
    fn round_trip_through_dom_string() {
        for k in [
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::Space,
            Key::Enter,
            Key::Escape,
            Key::Char('q'),
            Key::Other("F12".into()),
        ] {
            let s = k.to_dom_string();
            assert_eq!(
                Key::from_dom_string(&s),
                k,
                "round-trip failed for {:?} via {:?}",
                k,
                s
            );
        }
    }

    #[test]
    fn input_records_subscriptions() {
        let i = Input::new();
        i.subscribe_keydown();
        i.subscribe_keyup();
        i.subscribe_keydown();
        assert_eq!(
            i.drain_calls(),
            vec![
                InputCall::SubscribeKeyDown,
                InputCall::SubscribeKeyUp,
                InputCall::SubscribeKeyDown,
            ]
        );
    }

    #[test]
    fn import_constants_are_canonical() {
        assert_eq!(WIT_IMPORT_SUBSCRIBE_KEYDOWN.0, "mty:web/input@0.1");
        assert_eq!(WIT_IMPORT_SUBSCRIBE_KEYDOWN.1, "subscribe-keydown");
        assert_eq!(WIT_IMPORT_SUBSCRIBE_KEYUP.1, "subscribe-keyup");
        assert_eq!(WIT_EXPORT_KEYDOWN, "keydown");
        assert_eq!(WIT_EXPORT_KEYUP, "keyup");
        assert_eq!(WIT_EXPORT_FRAME, "frame");
    }
}
