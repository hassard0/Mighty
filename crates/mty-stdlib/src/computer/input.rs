//! Mouse + keyboard dispatch for `std.computer`.
//!
//! v0.30 baseline ships [`MockMouse`] / [`MockKeyboard`] backends —
//! they record every action into an in-memory log instead of firing
//! real OS events. Real cross-platform backends (typically an `enigo`
//! wrapper) live behind the `computer-input` feature flag, OFF by
//! default so:
//!
//! - The default workspace build doesn't pull X11 / Cocoa / Win32 link
//!   deps.
//! - CI's `cargo test` never moves the real cursor.
//! - Production callers who do want real input opt in explicitly.
//!
//! The trait surfaces ([`MouseBackend`], [`KeyboardBackend`]) are
//! fixed, so a `Dispatcher::with_mouse(EnigoMouse::new()?)` switch
//! later is a one-line change for the caller.

use std::fmt;
use std::sync::{Arc, Mutex};

/// Standard mouse buttons. Mirrors the Anthropic `computer_20241022`
/// vocabulary — `left_click`, `middle_click`, `right_click`,
/// `double_click` map to button + count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    /// String form used in the Anthropic action JSON.
    pub fn as_anthropic(self) -> &'static str {
        match self {
            MouseButton::Left => "left",
            MouseButton::Middle => "middle",
            MouseButton::Right => "right",
        }
    }
}

/// Logical keyboard keys the model can press by name (Anthropic
/// computer-use accepts plain characters via `type` but uses a
/// reduced symbolic vocabulary for non-character keys).
///
/// Surfaces the names exactly as Anthropic's docs list them; the
/// [`Key::as_anthropic`] mapping is also the wire format. New
/// callers can use [`Key::from_str_lenient`] to parse the
/// equivalent xdotool-style names (`Return` → `enter`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Enter,
    Escape,
    Backspace,
    Tab,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    /// A function key by index (1..=12).
    Function(u8),
    /// A modifier+letter chord like `ctrl+l` or `ctrl+shift+t`. The
    /// string is normalised to lowercase, `+`-separated, with the
    /// modifier order `ctrl|alt|shift|meta|cmd|win`.
    Chord(String),
}

impl Key {
    /// String form used in the Anthropic action JSON's `text` field
    /// for `key` actions.
    pub fn as_anthropic(&self) -> String {
        match self {
            Key::Enter => "Return".into(),
            Key::Escape => "Escape".into(),
            Key::Backspace => "BackSpace".into(),
            Key::Tab => "Tab".into(),
            Key::Space => "space".into(),
            Key::ArrowUp => "Up".into(),
            Key::ArrowDown => "Down".into(),
            Key::ArrowLeft => "Left".into(),
            Key::ArrowRight => "Right".into(),
            Key::Home => "Home".into(),
            Key::End => "End".into(),
            Key::PageUp => "Page_Up".into(),
            Key::PageDown => "Page_Down".into(),
            Key::Delete => "Delete".into(),
            Key::Function(n) => format!("F{n}"),
            Key::Chord(s) => s.clone(),
        }
    }

    /// Parse a key string from the model's tool_use payload. Accepts
    /// Anthropic's vocabulary (`Return`, `Page_Down`) plus the
    /// xdotool-style aliases (`enter`, `pgdn`). Falls back to
    /// [`Key::Chord`] for anything containing `+` so modifier chords
    /// round-trip without losing structure.
    pub fn from_str_lenient(s: &str) -> Key {
        let lower = s.to_ascii_lowercase();
        match lower.as_str() {
            "return" | "enter" => Key::Enter,
            "escape" | "esc" => Key::Escape,
            "backspace" => Key::Backspace,
            "tab" => Key::Tab,
            "space" => Key::Space,
            "up" => Key::ArrowUp,
            "down" => Key::ArrowDown,
            "left" => Key::ArrowLeft,
            "right" => Key::ArrowRight,
            "home" => Key::Home,
            "end" => Key::End,
            "page_up" | "pgup" => Key::PageUp,
            "page_down" | "pgdn" => Key::PageDown,
            "delete" | "del" => Key::Delete,
            other if other.starts_with('f') && other[1..].chars().all(|c| c.is_ascii_digit()) => {
                let n: u8 = other[1..].parse().unwrap_or(1);
                if (1..=24).contains(&n) {
                    Key::Function(n)
                } else {
                    Key::Chord(lower)
                }
            }
            _ => Key::Chord(lower),
        }
    }

    /// True if this key is a modifier-chord (`Ctrl+L`, …). Used by the
    /// deny-list check.
    pub fn is_chord(&self) -> bool {
        matches!(self, Key::Chord(_))
    }
}

/// Errors raised by the input backends.
#[derive(Debug, thiserror::Error)]
pub enum InputError {
    /// The OS-level dispatch call failed.
    #[error("backend `{backend}` failed: {message}")]
    Backend { backend: String, message: String },

    /// The backend was compiled out (the `computer-input` feature is
    /// not enabled and only [`MockMouse`] / [`MockKeyboard`] are
    /// available).
    #[error("backend `{0}` was compiled out; enable feature `computer-input` to use it")]
    BackendDisabled(&'static str),
}

/// One mouse event recorded by [`MockMouse`] — surfaced so tests can
/// assert what the dispatcher emitted without firing real OS events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseEvent {
    MoveTo {
        x: u32,
        y: u32,
    },
    Click {
        x: u32,
        y: u32,
        button: MouseButton,
        count: u8,
    },
    Drag {
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
        button: MouseButton,
    },
    Scroll {
        x: u32,
        y: u32,
        dx: i32,
        dy: i32,
    },
}

/// Object-safe mouse dispatch surface.
pub trait MouseBackend: Send + Sync + fmt::Debug {
    fn move_to(&self, x: u32, y: u32) -> Result<(), InputError>;
    fn click_at(&self, x: u32, y: u32, button: MouseButton) -> Result<(), InputError>;
    /// Default impl: click N times. Backends with a native
    /// double-click op override.
    fn click_n(&self, x: u32, y: u32, button: MouseButton, count: u8) -> Result<(), InputError> {
        for _ in 0..count.max(1) {
            self.click_at(x, y, button)?;
        }
        Ok(())
    }
    fn drag(
        &self,
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
        button: MouseButton,
    ) -> Result<(), InputError>;
    fn scroll(&self, x: u32, y: u32, dx: i32, dy: i32) -> Result<(), InputError>;
    fn name(&self) -> &'static str;
}

/// Object-safe keyboard dispatch surface.
pub trait KeyboardBackend: Send + Sync + fmt::Debug {
    fn type_text(&self, text: &str) -> Result<(), InputError>;
    fn key_press(&self, key: &Key) -> Result<(), InputError>;
    fn name(&self) -> &'static str;
}

/// Caller-facing handle that wraps a [`MouseBackend`].
#[derive(Debug)]
pub struct Mouse {
    backend: Box<dyn MouseBackend>,
}

impl Mouse {
    pub fn from_backend<B: MouseBackend + 'static>(backend: B) -> Self {
        Self {
            backend: Box::new(backend),
        }
    }

    pub fn from_boxed(backend: Box<dyn MouseBackend>) -> Self {
        Self { backend }
    }

    pub fn move_to(&self, x: u32, y: u32) -> Result<(), InputError> {
        self.backend.move_to(x, y)
    }

    pub fn click_at(&self, x: u32, y: u32, button: MouseButton) -> Result<(), InputError> {
        self.backend.click_at(x, y, button)
    }

    pub fn click_n(
        &self,
        x: u32,
        y: u32,
        button: MouseButton,
        count: u8,
    ) -> Result<(), InputError> {
        self.backend.click_n(x, y, button, count)
    }

    pub fn drag(
        &self,
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
        button: MouseButton,
    ) -> Result<(), InputError> {
        self.backend.drag(x1, y1, x2, y2, button)
    }

    pub fn scroll(&self, x: u32, y: u32, dx: i32, dy: i32) -> Result<(), InputError> {
        self.backend.scroll(x, y, dx, dy)
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }
}

/// Caller-facing handle that wraps a [`KeyboardBackend`].
#[derive(Debug)]
pub struct Keyboard {
    backend: Box<dyn KeyboardBackend>,
}

impl Keyboard {
    pub fn from_backend<B: KeyboardBackend + 'static>(backend: B) -> Self {
        Self {
            backend: Box::new(backend),
        }
    }

    pub fn from_boxed(backend: Box<dyn KeyboardBackend>) -> Self {
        Self { backend }
    }

    pub fn type_text(&self, text: &str) -> Result<(), InputError> {
        self.backend.type_text(text)
    }

    pub fn key_press(&self, key: &Key) -> Result<(), InputError> {
        self.backend.key_press(key)
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }
}

/// CI-safe mock mouse — records every event into an in-memory log
/// instead of moving the real cursor. Tests assert against
/// [`MockMouse::events`].
#[derive(Debug, Clone, Default)]
pub struct MockMouse {
    log: Arc<Mutex<Vec<MouseEvent>>>,
}

impl MockMouse {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of every event recorded so far. Returns a fresh
    /// `Vec` so the caller can hold it without freezing the lock.
    pub fn events(&self) -> Vec<MouseEvent> {
        self.log.lock().unwrap().clone()
    }

    /// True if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.log.lock().unwrap().is_empty()
    }

    /// Total number of events recorded.
    pub fn len(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    fn push(&self, e: MouseEvent) {
        self.log.lock().unwrap().push(e);
    }
}

impl MouseBackend for MockMouse {
    fn move_to(&self, x: u32, y: u32) -> Result<(), InputError> {
        self.push(MouseEvent::MoveTo { x, y });
        Ok(())
    }

    fn click_at(&self, x: u32, y: u32, button: MouseButton) -> Result<(), InputError> {
        self.push(MouseEvent::Click {
            x,
            y,
            button,
            count: 1,
        });
        Ok(())
    }

    fn click_n(&self, x: u32, y: u32, button: MouseButton, count: u8) -> Result<(), InputError> {
        self.push(MouseEvent::Click {
            x,
            y,
            button,
            count,
        });
        Ok(())
    }

    fn drag(
        &self,
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
        button: MouseButton,
    ) -> Result<(), InputError> {
        self.push(MouseEvent::Drag {
            x1,
            y1,
            x2,
            y2,
            button,
        });
        Ok(())
    }

    fn scroll(&self, x: u32, y: u32, dx: i32, dy: i32) -> Result<(), InputError> {
        self.push(MouseEvent::Scroll { x, y, dx, dy });
        Ok(())
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}

/// One keyboard event recorded by [`MockKeyboard`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyEvent {
    Type(String),
    Key(Key),
}

/// CI-safe mock keyboard — records every event instead of generating
/// real OS keystrokes.
#[derive(Debug, Clone, Default)]
pub struct MockKeyboard {
    log: Arc<Mutex<Vec<KeyEvent>>>,
}

impl MockKeyboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<KeyEvent> {
        self.log.lock().unwrap().clone()
    }

    pub fn is_empty(&self) -> bool {
        self.log.lock().unwrap().is_empty()
    }

    pub fn len(&self) -> usize {
        self.log.lock().unwrap().len()
    }
}

impl KeyboardBackend for MockKeyboard {
    fn type_text(&self, text: &str) -> Result<(), InputError> {
        self.log.lock().unwrap().push(KeyEvent::Type(text.into()));
        Ok(())
    }

    fn key_press(&self, key: &Key) -> Result<(), InputError> {
        self.log.lock().unwrap().push(KeyEvent::Key(key.clone()));
        Ok(())
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_button_anthropic_names() {
        assert_eq!(MouseButton::Left.as_anthropic(), "left");
        assert_eq!(MouseButton::Middle.as_anthropic(), "middle");
        assert_eq!(MouseButton::Right.as_anthropic(), "right");
    }

    #[test]
    fn key_anthropic_round_trip_basic() {
        assert_eq!(Key::Enter.as_anthropic(), "Return");
        assert_eq!(Key::PageDown.as_anthropic(), "Page_Down");
        assert_eq!(Key::Function(5).as_anthropic(), "F5");
        assert_eq!(Key::Chord("ctrl+l".into()).as_anthropic(), "ctrl+l");
    }

    #[test]
    fn key_from_str_lenient_handles_aliases() {
        assert_eq!(Key::from_str_lenient("Return"), Key::Enter);
        assert_eq!(Key::from_str_lenient("enter"), Key::Enter);
        assert_eq!(Key::from_str_lenient("ESC"), Key::Escape);
        assert_eq!(Key::from_str_lenient("pgdn"), Key::PageDown);
        assert_eq!(Key::from_str_lenient("F12"), Key::Function(12));
        assert_eq!(Key::from_str_lenient("ctrl+L"), Key::Chord("ctrl+l".into()));
    }

    #[test]
    fn key_is_chord_only_for_chord_variant() {
        assert!(Key::Chord("ctrl+l".into()).is_chord());
        assert!(!Key::Enter.is_chord());
        assert!(!Key::Function(1).is_chord());
    }

    #[test]
    fn mock_mouse_records_move_click_drag_scroll() {
        let m = MockMouse::new();
        let mouse = Mouse::from_backend(m.clone());
        mouse.move_to(10, 20).unwrap();
        mouse.click_at(30, 40, MouseButton::Left).unwrap();
        mouse.click_n(50, 60, MouseButton::Right, 2).unwrap();
        mouse.drag(0, 0, 100, 100, MouseButton::Left).unwrap();
        mouse.scroll(5, 5, 0, -3).unwrap();
        let events = m.events();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0], MouseEvent::MoveTo { x: 10, y: 20 });
        assert_eq!(
            events[1],
            MouseEvent::Click {
                x: 30,
                y: 40,
                button: MouseButton::Left,
                count: 1
            }
        );
        assert_eq!(
            events[2],
            MouseEvent::Click {
                x: 50,
                y: 60,
                button: MouseButton::Right,
                count: 2
            }
        );
        assert!(matches!(events[3], MouseEvent::Drag { .. }));
        assert_eq!(
            events[4],
            MouseEvent::Scroll {
                x: 5,
                y: 5,
                dx: 0,
                dy: -3
            }
        );
    }

    #[test]
    fn mock_mouse_len_and_is_empty() {
        let m = MockMouse::new();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
        m.move_to(0, 0).unwrap();
        assert!(!m.is_empty());
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn mock_keyboard_records_type_and_key() {
        let k = MockKeyboard::new();
        let kb = Keyboard::from_backend(k.clone());
        kb.type_text("hello world").unwrap();
        kb.key_press(&Key::Enter).unwrap();
        kb.key_press(&Key::Chord("ctrl+s".into())).unwrap();
        let events = k.events();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], KeyEvent::Type("hello world".into()));
        assert_eq!(events[1], KeyEvent::Key(Key::Enter));
        assert_eq!(events[2], KeyEvent::Key(Key::Chord("ctrl+s".into())));
    }

    #[test]
    fn mock_keyboard_backend_name_is_mock() {
        let k = MockKeyboard::new();
        assert_eq!(k.name(), "mock");
    }

    #[test]
    fn mouse_backend_name_is_mock() {
        let m = MockMouse::new();
        assert_eq!(m.name(), "mock");
    }

    #[test]
    fn mock_mouse_shares_log_through_clone() {
        // Important: `Arc<Mutex>` semantics — cloning the mock should
        // share the underlying log so the dispatcher and the test see
        // the same events.
        let a = MockMouse::new();
        let b = a.clone();
        Mouse::from_backend(a).move_to(1, 2).unwrap();
        assert_eq!(b.len(), 1);
    }
}
