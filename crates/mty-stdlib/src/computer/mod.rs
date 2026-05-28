//! `std.computer` — first-class Anthropic Computer Use, capability-typed.
//!
//! v0.30 Track C: integrate Anthropic's
//! [Computer Use](https://docs.anthropic.com/en/docs/agents-and-tools/computer-use)
//! tool family (`computer_20241022`) as a first-class Mighty stdlib
//! capability. The model receives a screenshot, then emits a stream of
//! mouse/keyboard `tool_use` blocks; the [`Dispatcher`] in
//! [`dispatcher`] enforces a typed capability boundary before any
//! action actually fires.
//!
//! ## Why this lives in `std.computer` and not user code
//!
//! 1. **Sandbox is non-trivial.** Computer Use is an arbitrary-code
//!    primitive: a single `tool_use { name: "computer", action: "key",
//!    text: "ctrl+alt+delete" }` is enough to log a user out. Putting
//!    the sandbox in stdlib means every user gets the same
//!    [`ComputerCap`]-gated boundary by default.
//! 2. **Capability typing is the security model.** Without
//!    [`ComputerCap`] in scope, [`screen::Screen::capture`] /
//!    [`input::Mouse::click_at`] / [`input::Keyboard::type_text`]
//!    refuse to fire at runtime; the `@computer_use` macro generates a
//!    declaration that surfaces the capability requirement at type-check
//!    time so the diagnostic lands before the program ever runs.
//! 3. **Provider integration is one line.** [`dispatcher::Dispatcher`]
//!    wraps an [`LlmProvider`](crate::llm::LlmProvider) and a
//!    [`ComputerCap`]; the agent loop is `dispatcher.run(task).await`.
//!
//! ## Module layout
//!
//! | Module | Role |
//! |---|---|
//! | [`screen`] | Capture a screenshot. Backend-pluggable; default is the [`screen::MockScreen`] PNG-bytes-shaped buffer so `cargo test` never grabs a real display. |
//! | [`input`] | Mouse + keyboard dispatch. Mirrors `enigo`-style API surface but defaults to a [`input::MockMouse`] / [`input::MockKeyboard`] that record actions instead of firing them — CI-safe out of the box. |
//! | [`sandbox`] | The [`ComputerCap`] type. Holds optional click-bounds + a deny-list of key chords; consulted on EVERY action by the dispatcher. |
//! | [`dispatcher`] | The glue: takes an LLM provider + cap + screen + input pair, runs the Anthropic computer-use loop. |
//!
//! ## Quickstart (Rust)
//!
//! ```no_run
//! use mty_stdlib::computer::{
//!     dispatcher::Dispatcher,
//!     sandbox::ComputerCap,
//!     screen::MockScreen,
//!     input::{MockMouse, MockKeyboard},
//! };
//! use mty_stdlib::llm::anthropic::AnthropicClient;
//!
//! # async fn run() -> Result<(), mty_stdlib::computer::ComputerError> {
//! let cap = ComputerCap::screen_and_input()
//!     .with_bounds(0, 0, 1280, 800)
//!     .deny_keys(&["ctrl+alt+delete", "cmd+q"]);
//! let llm = AnthropicClient::from_env().expect("ANTHROPIC_API_KEY");
//! let dispatcher = Dispatcher::new(llm, cap)
//!     .with_screen(MockScreen::solid_color(1280, 800, [0, 0, 0]))
//!     .with_mouse(MockMouse::default())
//!     .with_keyboard(MockKeyboard::default());
//! let summary = dispatcher.run("take a screenshot then say done").await?;
//! println!("{summary}");
//! # Ok(()) }
//! ```
//!
//! ## Threat model
//!
//! Computer Use puts the model in a position where a malicious payload
//! visible in a screenshot (an email subject, a chat message, a
//! webpage popup) can be treated as instructions ("prompt injection
//! via the display"). The capability gate cannot prevent the model
//! from being deceived — it CAN prevent the consequences:
//!
//! - **Bounds** rejects clicks outside the agent's intended viewport,
//!   so a popup that shifts the click target to a system menu fails
//!   closed.
//! - **Deny-list** rejects dangerous key chords (system menu, OS
//!   logoff, password manager unlock) regardless of how the model was
//!   convinced to type them.
//! - **Taint propagation** (v0.30 Track A): screenshot bytes are
//!   `Tainted<Vec<u8>>`; the dispatcher validates actions against the
//!   cap BEFORE executing, so a tainted action that escapes the cap
//!   raises [`ComputerError::SandboxViolation`] without reaching the
//!   OS.
//!
//! See `docs/internals/computer-use.md` for the full model.

pub mod dispatcher;
pub mod input;
pub mod sandbox;
pub mod screen;

pub use dispatcher::{Dispatcher, MAX_TURNS};
pub use input::{
    InputError, Key, Keyboard, KeyboardBackend, MockKeyboard, MockMouse, Mouse, MouseBackend,
    MouseButton,
};
pub use sandbox::{ComputerCap, ComputerCapBuilder, SandboxViolation};
pub use screen::{MockScreen, Screen, ScreenBackend, ScreenError, Screenshot};

/// Top-level error type for [`std.computer`](self).
///
/// Most call sites convert from one of the four leaf errors via
/// `?`; the top-level enum is shaped so a single `match` can react
/// to "sandbox said no" vs "OS-level capture failed" vs "the LLM
/// emitted an action we can't parse" without crossing module
/// boundaries.
#[derive(Debug, thiserror::Error)]
pub enum ComputerError {
    /// The action was rejected by [`ComputerCap`]. The model emitted a
    /// click outside the bounded region, or a key chord on the
    /// deny-list, or called a tool the cap does not authorise.
    #[error("sandbox violation: {0}")]
    SandboxViolation(#[from] SandboxViolation),

    /// The screen-capture backend failed.
    #[error("screen capture failed: {0}")]
    Screen(#[from] ScreenError),

    /// The input backend failed.
    #[error("input dispatch failed: {0}")]
    Input(#[from] InputError),

    /// The LLM provider returned an error.
    #[error("llm provider failed: {0}")]
    Llm(#[from] crate::llm::LlmError),

    /// The dispatcher could not parse the model's `tool_use` block as
    /// a valid computer-use action (unknown action, missing arg, …).
    /// Almost always a provider regression — the schema enforces shape
    /// on the wire side.
    #[error("malformed computer-use action: {0}")]
    MalformedAction(String),

    /// The dispatcher hit [`MAX_TURNS`] without the model emitting a
    /// terminal `Done` action. The agent is stopped to bound runaway
    /// loops.
    #[error("agent did not converge in {0} turns")]
    TurnLimit(u32),
}
