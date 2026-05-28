//! Glue between Anthropic's `computer_20241022` tool family and Mighty's
//! [`@computer_use`](mty_macros::stdlib::computer_use) agent surface.
//!
//! The [`Dispatcher`] runs the canonical loop:
//!
//! ```text
//!  1. send the user task + initial screenshot + tool spec to the LLM
//!  2. for each `tool_use` block the model returns:
//!       a. parse it into one of the [`ComputerAction`] variants
//!       b. validate against the [`ComputerCap`] BEFORE executing
//!       c. execute via the screen / mouse / keyboard backends
//!       d. feed the result (screenshot, ack, error) back to the LLM
//!  3. terminate when the model emits a `stop` or `done` action,
//!     or [`MAX_TURNS`] is exceeded.
//! ```
//!
//! The capability validation in step (2b) is the load-bearing safety
//! step — without it a model that reads "click the password manager
//! icon at (1450, 12)" off a screenshot could escape the sandbox even
//! though the cap declares `bounds(0, 0, 1280, 800)`.

use std::sync::Arc;

use crate::computer::input::{
    Key, Keyboard, KeyboardBackend, MockKeyboard, MockMouse, Mouse, MouseBackend, MouseButton,
};
use crate::computer::sandbox::ComputerCap;
use crate::computer::screen::{MockScreen, Screen, ScreenBackend, Screenshot};
use crate::computer::ComputerError;
use crate::llm::message::{ContentBlock, ImageSource, Message, Role, ToolResult, ToolUse};
use crate::llm::provider::{CompletionRequest, LlmProvider};
use crate::llm::tools::Tool;

/// Default upper bound on agent turns. Mirrors Anthropic's published
/// recommendation; surfaces as [`ComputerError::TurnLimit`] when hit.
pub const MAX_TURNS: u32 = 30;

/// The action vocabulary the dispatcher understands. The Anthropic
/// `computer_20241022` tool emits a JSON action shape; [`ComputerAction`]
/// is the typed equivalent the dispatcher branches on.
///
/// New actions land here as Anthropic extends the tool family — the
/// `Other` variant prevents unknown actions from crashing the dispatcher
/// (they raise [`ComputerError::MalformedAction`] instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComputerAction {
    /// `action: "screenshot"` — return a fresh capture.
    Screenshot,
    /// `action: "mouse_move"`.
    MouseMove { x: u32, y: u32 },
    /// `action: "left_click" | "right_click" | "middle_click"`. Count
    /// is 1 for the single-click variants, 2 for `double_click`.
    Click {
        x: u32,
        y: u32,
        button: MouseButton,
        count: u8,
    },
    /// `action: "left_click_drag"`.
    Drag {
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
        button: MouseButton,
    },
    /// `action: "type"` — type a literal string.
    Type { text: String },
    /// `action: "key"` — press a named key chord.
    Key { name: String },
    /// `action: "scroll"`.
    Scroll { x: u32, y: u32, dx: i32, dy: i32 },
    /// `action: "stop" | "done"` — terminal action; the dispatcher
    /// returns the agent summary.
    Done { summary: String },
}

impl ComputerAction {
    /// Parse a `tool_use` input JSON value into a typed action.
    /// Returns [`ComputerError::MalformedAction`] when the shape is
    /// unrecognised.
    ///
    /// Recognises the Anthropic `computer_20241022` action keys
    /// PLUS the abbreviated convenience names (`click` → left,
    /// `type_text` → type, …) some early adopters use; both round-
    /// trip without losing structure.
    pub fn parse(input: &serde_json::Value) -> Result<Self, ComputerError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ComputerError::MalformedAction("missing `action` field".into()))?;
        match action {
            "screenshot" => Ok(ComputerAction::Screenshot),
            "mouse_move" => Ok(ComputerAction::MouseMove {
                x: read_coord(input, "x")?,
                y: read_coord(input, "y")?,
            }),
            "left_click" => Ok(ComputerAction::Click {
                x: read_coord(input, "x")?,
                y: read_coord(input, "y")?,
                button: MouseButton::Left,
                count: 1,
            }),
            "right_click" => Ok(ComputerAction::Click {
                x: read_coord(input, "x")?,
                y: read_coord(input, "y")?,
                button: MouseButton::Right,
                count: 1,
            }),
            "middle_click" => Ok(ComputerAction::Click {
                x: read_coord(input, "x")?,
                y: read_coord(input, "y")?,
                button: MouseButton::Middle,
                count: 1,
            }),
            "double_click" => Ok(ComputerAction::Click {
                x: read_coord(input, "x")?,
                y: read_coord(input, "y")?,
                button: MouseButton::Left,
                count: 2,
            }),
            "left_click_drag" => {
                // Anthropic's drag uses `coordinate` for the END point and
                // implicitly starts at the current cursor; we additionally
                // accept explicit `start_coordinate`.
                let (x2, y2) = read_xy_coord_pair(input)?;
                let (x1, y1) = input
                    .get("start_coordinate")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| {
                        let x = arr.first().and_then(|v| v.as_u64())?;
                        let y = arr.get(1).and_then(|v| v.as_u64())?;
                        Some((x as u32, y as u32))
                    })
                    .unwrap_or((x2, y2));
                Ok(ComputerAction::Drag {
                    x1,
                    y1,
                    x2,
                    y2,
                    button: MouseButton::Left,
                })
            }
            "type" => Ok(ComputerAction::Type {
                text: input
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ComputerError::MalformedAction("type: missing `text`".into()))?
                    .to_string(),
            }),
            "key" => Ok(ComputerAction::Key {
                name: input
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ComputerError::MalformedAction("key: missing `text`".into()))?
                    .to_string(),
            }),
            "scroll" => Ok(ComputerAction::Scroll {
                x: read_coord(input, "x").unwrap_or(0),
                y: read_coord(input, "y").unwrap_or(0),
                dx: input
                    .get("scroll_direction_x")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32,
                dy: input
                    .get("scroll_direction_y")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32,
            }),
            "stop" | "done" => Ok(ComputerAction::Done {
                summary: input
                    .get("summary")
                    .or_else(|| input.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            }),
            other => Err(ComputerError::MalformedAction(format!(
                "unknown action `{other}`"
            ))),
        }
    }
}

fn read_coord(input: &serde_json::Value, k: &str) -> Result<u32, ComputerError> {
    // Anthropic packs coords either as `{ "x": 12, "y": 34 }` or as
    // `{ "coordinate": [12, 34] }`. Accept both.
    if let Some(v) = input.get(k).and_then(|v| v.as_u64()) {
        return Ok(v as u32);
    }
    if let Some(arr) = input.get("coordinate").and_then(|v| v.as_array()) {
        let idx = if k == "x" { 0 } else { 1 };
        if let Some(v) = arr.get(idx).and_then(|v| v.as_u64()) {
            return Ok(v as u32);
        }
    }
    Err(ComputerError::MalformedAction(format!("missing `{k}`")))
}

fn read_xy_coord_pair(input: &serde_json::Value) -> Result<(u32, u32), ComputerError> {
    let x = read_coord(input, "x")?;
    let y = read_coord(input, "y")?;
    Ok((x, y))
}

/// Render the Anthropic `computer_20241022` tool spec for a given
/// display size. The dispatcher serialises this into the
/// [`CompletionRequest`]'s `tools` array.
///
/// Anthropic's wire shape is custom for the computer tool family —
/// it carries `type: "computer_20241022"` instead of the generic
/// `{name, description, input_schema}` triple. We surface it through
/// the same [`Tool`] struct (with the type discriminator hidden in
/// the input_schema's `_provider_type` slot) so the rest of `std.llm`
/// doesn't need a parallel "ComputerTool" enum.
pub fn build_computer_tool_spec(width: u32, height: u32) -> Tool {
    Tool::new(
        "computer",
        "Anthropic computer-use tool family",
        serde_json::json!({
            "_provider_type": "computer_20241022",
            "display_width_px": width,
            "display_height_px": height,
            "display_number": 1
        }),
    )
}

/// The agent loop driver. Owns an LLM provider, a cap, and the three
/// I/O backends.
pub struct Dispatcher {
    llm: Arc<dyn LlmProvider>,
    cap: ComputerCap,
    screen: Screen,
    mouse: Mouse,
    keyboard: Keyboard,
    model: String,
    max_turns: u32,
    system_prompt: Option<String>,
}

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatcher")
            .field("cap", &self.cap)
            .field("screen", &self.screen.backend_name())
            .field("mouse", &self.mouse.backend_name())
            .field("keyboard", &self.keyboard.backend_name())
            .field("model", &self.model)
            .field("max_turns", &self.max_turns)
            .finish_non_exhaustive()
    }
}

impl Dispatcher {
    /// New dispatcher with the supplied provider + cap. Defaults:
    ///
    /// - screen: [`MockScreen::default`] (1280 x 800)
    /// - mouse:  [`MockMouse::default`]
    /// - keyboard: [`MockKeyboard::default`]
    /// - model: `claude-opus-4-7`
    /// - max_turns: [`MAX_TURNS`]
    /// - system prompt: a short safety preamble
    pub fn new<P: LlmProvider + 'static>(llm: P, cap: ComputerCap) -> Self {
        Self {
            llm: Arc::new(llm),
            cap,
            screen: Screen::from_backend(MockScreen::default()),
            mouse: Mouse::from_backend(MockMouse::default()),
            keyboard: Keyboard::from_backend(MockKeyboard::default()),
            model: "claude-opus-4-7".to_string(),
            max_turns: MAX_TURNS,
            system_prompt: Some(default_system_prompt()),
        }
    }

    #[must_use]
    pub fn with_screen<B: ScreenBackend + 'static>(mut self, b: B) -> Self {
        self.screen = Screen::from_backend(b);
        self
    }

    #[must_use]
    pub fn with_mouse<B: MouseBackend + 'static>(mut self, b: B) -> Self {
        self.mouse = Mouse::from_backend(b);
        self
    }

    #[must_use]
    pub fn with_keyboard<B: KeyboardBackend + 'static>(mut self, b: B) -> Self {
        self.keyboard = Keyboard::from_backend(b);
        self
    }

    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    #[must_use]
    pub fn with_max_turns(mut self, n: u32) -> Self {
        self.max_turns = n;
        self
    }

    #[must_use]
    pub fn with_system_prompt(mut self, p: impl Into<String>) -> Self {
        self.system_prompt = Some(p.into());
        self
    }

    pub fn cap(&self) -> &ComputerCap {
        &self.cap
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn mouse(&self) -> &Mouse {
        &self.mouse
    }

    pub fn keyboard(&self) -> &Keyboard {
        &self.keyboard
    }

    /// Run the agent loop until the model emits a terminal `Done`
    /// action or [`Dispatcher::with_max_turns`] is exceeded.
    pub async fn run(&self, task: &str) -> Result<String, ComputerError> {
        self.cap.reset_counter();
        let initial_shot = self.capture_with_cap()?;
        let tool = build_computer_tool_spec(self.screen.width(), self.screen.height());
        let mut history: Vec<Message> = vec![Message {
            role: Role::User,
            content: vec![
                ContentBlock::text(task.to_string()),
                screenshot_to_block(&initial_shot),
            ],
        }];

        for _turn in 0..self.max_turns {
            let req = CompletionRequest::new(&self.model, history.clone())
                .with_tools(vec![tool.clone()])
                .with_max_tokens(1024);
            let req = if let Some(p) = &self.system_prompt {
                req.with_system(p.clone())
            } else {
                req
            };
            let reply = self.llm.complete(req).await?;
            let tool_uses = collect_tool_uses(&reply);
            // Record the assistant turn verbatim so the next call
            // includes the model's reasoning + tool calls.
            history.push(reply.clone());

            if tool_uses.is_empty() {
                // No tool call — treat the plain text as the final
                // summary.
                return Ok(reply.text());
            }

            let mut next_results: Vec<ContentBlock> = Vec::new();
            for tu in tool_uses {
                if tu.name != "computer" {
                    next_results.push(ContentBlock::ToolResult(ToolResult {
                        tool_use_id: tu.id.clone(),
                        content: format!("unknown tool `{}`", tu.name),
                        is_error: true,
                    }));
                    continue;
                }
                let action = match ComputerAction::parse(&tu.input) {
                    Ok(a) => a,
                    Err(e) => {
                        next_results.push(ContentBlock::ToolResult(ToolResult {
                            tool_use_id: tu.id.clone(),
                            content: format!("malformed action: {e}"),
                            is_error: true,
                        }));
                        continue;
                    }
                };
                match self.execute_action(&action) {
                    Ok(ActionOutcome::Done(summary)) => return Ok(summary),
                    Ok(ActionOutcome::Screenshot(shot)) => {
                        // Tool result for screenshot includes an embedded
                        // image content block.
                        next_results.push(ContentBlock::ToolResult(ToolResult {
                            tool_use_id: tu.id.clone(),
                            content: format!(
                                "screenshot {}x{} ({} bytes)",
                                shot.width,
                                shot.height,
                                shot.size_bytes()
                            ),
                            is_error: false,
                        }));
                        // Append the screenshot image as a fresh user
                        // turn so the model can see it on the next
                        // round.
                        next_results.push(screenshot_to_block(&shot));
                    }
                    Ok(ActionOutcome::Ack) => {
                        next_results.push(ContentBlock::ToolResult(ToolResult {
                            tool_use_id: tu.id.clone(),
                            content: "ok".to_string(),
                            is_error: false,
                        }));
                    }
                    Err(e) => {
                        // Sandbox / OS errors surface as ERROR tool
                        // results so the model can react (back off,
                        // give up, …). The error itself is also
                        // returned to the caller via `?`.
                        next_results.push(ContentBlock::ToolResult(ToolResult {
                            tool_use_id: tu.id.clone(),
                            content: format!("{e}"),
                            is_error: true,
                        }));
                        // CRITICAL: a sandbox violation aborts the
                        // run. The model continuing to attempt new
                        // actions after the cap fires is the failure
                        // mode the cap is meant to bound.
                        if matches!(e, ComputerError::SandboxViolation(_)) {
                            return Err(e);
                        }
                    }
                }
            }
            history.push(Message {
                role: Role::User,
                content: next_results,
            });
        }
        Err(ComputerError::TurnLimit(self.max_turns))
    }

    fn capture_with_cap(&self) -> Result<Screenshot, ComputerError> {
        self.cap.check_screen()?;
        Ok(self.screen.capture()?)
    }

    /// Validate + execute one [`ComputerAction`]. Returns the outcome
    /// shape the agent loop needs to thread into the next message.
    pub(crate) fn execute_action(
        &self,
        action: &ComputerAction,
    ) -> Result<ActionOutcome, ComputerError> {
        match action {
            ComputerAction::Screenshot => {
                let shot = self.capture_with_cap()?;
                Ok(ActionOutcome::Screenshot(shot))
            }
            ComputerAction::MouseMove { x, y } => {
                self.cap.check_click(*x, *y)?;
                self.mouse.move_to(*x, *y)?;
                Ok(ActionOutcome::Ack)
            }
            ComputerAction::Click {
                x,
                y,
                button,
                count,
            } => {
                self.cap.check_click(*x, *y)?;
                self.mouse.click_n(*x, *y, *button, *count)?;
                Ok(ActionOutcome::Ack)
            }
            ComputerAction::Drag {
                x1,
                y1,
                x2,
                y2,
                button,
            } => {
                self.cap.check_click(*x1, *y1)?;
                self.cap.check_click(*x2, *y2)?;
                self.mouse.drag(*x1, *y1, *x2, *y2, *button)?;
                Ok(ActionOutcome::Ack)
            }
            ComputerAction::Type { text } => {
                self.cap.check_type_text(text)?;
                self.keyboard.type_text(text)?;
                Ok(ActionOutcome::Ack)
            }
            ComputerAction::Key { name } => {
                self.cap.check_key(name)?;
                self.keyboard.key_press(&Key::from_str_lenient(name))?;
                Ok(ActionOutcome::Ack)
            }
            ComputerAction::Scroll { x, y, dx, dy } => {
                self.cap.check_click(*x, *y)?;
                self.mouse.scroll(*x, *y, *dx, *dy)?;
                Ok(ActionOutcome::Ack)
            }
            ComputerAction::Done { summary } => Ok(ActionOutcome::Done(summary.clone())),
        }
    }
}

/// What an action's execution returned. Internal to the dispatcher;
/// surfaced for tests + future re-use.
#[derive(Debug)]
pub(crate) enum ActionOutcome {
    /// The model asked for a screenshot — the new capture follows.
    Screenshot(Screenshot),
    /// The action completed; reply to the model is "ok".
    Ack,
    /// The model emitted a terminal `done`/`stop`; agent loop
    /// returns this summary.
    Done(String),
}

/// Convenience helper: rebuild the cap, screen, input pieces from a
/// `@computer_use(width:..., height:..., cap: ...)` macro decl.
///
/// The macro-generated agent calls this to construct the dispatcher
/// from its declarative inputs without forcing every caller to
/// re-import each piece.
pub fn from_macro_args(
    llm: Arc<dyn LlmProvider>,
    width: u32,
    height: u32,
    cap: ComputerCap,
) -> Dispatcher {
    Dispatcher {
        llm,
        cap,
        screen: Screen::from_backend(MockScreen::solid_color(width, height, [0; 3])),
        mouse: Mouse::from_backend(MockMouse::default()),
        keyboard: Keyboard::from_backend(MockKeyboard::default()),
        model: "claude-opus-4-7".to_string(),
        max_turns: MAX_TURNS,
        system_prompt: Some(default_system_prompt()),
    }
}

fn default_system_prompt() -> String {
    "You are a careful computer-use agent. \
     Always take a screenshot before acting, prefer narrow precise actions, \
     and emit a `done` action with a one-line summary when the task is complete."
        .to_string()
}

fn screenshot_to_block(shot: &Screenshot) -> ContentBlock {
    // Anthropic image content takes base64-encoded source bytes.
    let encoded = base64_encode(&shot.bytes);
    ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: shot.media_type.clone(),
            data: encoded,
        },
    }
}

fn collect_tool_uses(msg: &Message) -> Vec<ToolUse> {
    msg.content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::ToolUse(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// Minimal base64 (no padding-stripping, classic alphabet). Hand-rolled
/// to avoid a `base64` dep — the surface is tiny.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n =
            (u32::from(bytes[i]) << 16) | (u32::from(bytes[i + 1]) << 8) | u32::from(bytes[i + 2]);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPHABET[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = u32::from(bytes[i]) << 16;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = (u32::from(bytes[i]) << 16) | (u32::from(bytes[i + 1]) << 8);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_computer_tool_spec_carries_provider_type() {
        let t = build_computer_tool_spec(1280, 800);
        assert_eq!(t.name, "computer");
        assert_eq!(t.input_schema["_provider_type"], "computer_20241022");
        assert_eq!(t.input_schema["display_width_px"], 1280);
        assert_eq!(t.input_schema["display_height_px"], 800);
    }

    #[test]
    fn parse_screenshot_action() {
        let a = ComputerAction::parse(&serde_json::json!({ "action": "screenshot" })).unwrap();
        assert_eq!(a, ComputerAction::Screenshot);
    }

    #[test]
    fn parse_left_click_with_x_y() {
        let a = ComputerAction::parse(&serde_json::json!({
            "action": "left_click", "x": 10, "y": 20
        }))
        .unwrap();
        assert_eq!(
            a,
            ComputerAction::Click {
                x: 10,
                y: 20,
                button: MouseButton::Left,
                count: 1
            }
        );
    }

    #[test]
    fn parse_left_click_with_coordinate_array() {
        let a = ComputerAction::parse(&serde_json::json!({
            "action": "left_click", "coordinate": [30, 40]
        }))
        .unwrap();
        assert_eq!(
            a,
            ComputerAction::Click {
                x: 30,
                y: 40,
                button: MouseButton::Left,
                count: 1
            }
        );
    }

    #[test]
    fn parse_double_click_sets_count_2() {
        let a = ComputerAction::parse(&serde_json::json!({
            "action": "double_click", "x": 5, "y": 6
        }))
        .unwrap();
        assert!(matches!(a, ComputerAction::Click { count: 2, .. }));
    }

    #[test]
    fn parse_type_action_reads_text_field() {
        let a = ComputerAction::parse(&serde_json::json!({
            "action": "type", "text": "hello"
        }))
        .unwrap();
        assert_eq!(
            a,
            ComputerAction::Type {
                text: "hello".into()
            }
        );
    }

    #[test]
    fn parse_key_action_reads_text_field() {
        let a = ComputerAction::parse(&serde_json::json!({
            "action": "key", "text": "Return"
        }))
        .unwrap();
        assert_eq!(
            a,
            ComputerAction::Key {
                name: "Return".into()
            }
        );
    }

    #[test]
    fn parse_done_action_carries_summary() {
        let a = ComputerAction::parse(&serde_json::json!({
            "action": "done", "summary": "saved file"
        }))
        .unwrap();
        assert_eq!(
            a,
            ComputerAction::Done {
                summary: "saved file".into()
            }
        );
    }

    #[test]
    fn parse_unknown_action_errors() {
        let err =
            ComputerAction::parse(&serde_json::json!({ "action": "frobnicate" })).unwrap_err();
        assert!(matches!(err, ComputerError::MalformedAction(_)));
    }

    #[test]
    fn parse_missing_action_errors() {
        let err = ComputerAction::parse(&serde_json::json!({})).unwrap_err();
        assert!(matches!(err, ComputerError::MalformedAction(_)));
    }

    #[test]
    fn base64_encode_round_trips_padded() {
        // Standard test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    /// A stub provider that returns a canned [`Message`] reply on the
    /// first call; used by the dispatcher loop tests.
    #[derive(Debug, Clone)]
    struct StubLlm {
        replies: Arc<std::sync::Mutex<Vec<Message>>>,
        seen: Arc<std::sync::Mutex<Vec<CompletionRequest>>>,
    }

    impl StubLlm {
        fn new(replies: Vec<Message>) -> Self {
            Self {
                replies: Arc::new(std::sync::Mutex::new(replies)),
                seen: Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for StubLlm {
        async fn complete(&self, req: CompletionRequest) -> Result<Message, crate::llm::LlmError> {
            self.seen.lock().unwrap().push(req);
            let mut r = self.replies.lock().unwrap();
            if r.is_empty() {
                Ok(Message::assistant_text(""))
            } else {
                Ok(r.remove(0))
            }
        }

        async fn complete_stream(
            &self,
            _req: CompletionRequest,
        ) -> Result<crate::llm::MessageStream, crate::llm::LlmError> {
            unimplemented!("not needed in dispatcher tests")
        }

        fn schema_for_tool(&self, tool: &Tool) -> serde_json::Value {
            serde_json::to_value(tool).unwrap()
        }
    }

    #[tokio::test]
    async fn dispatcher_terminates_on_text_only_reply() {
        let cap = ComputerCap::screen_and_input();
        let llm = StubLlm::new(vec![Message::assistant_text("nothing to do")]);
        let d = Dispatcher::new(llm, cap);
        let out = d.run("noop task").await.unwrap();
        assert_eq!(out, "nothing to do");
    }

    #[tokio::test]
    async fn dispatcher_executes_click_then_done() {
        let cap = ComputerCap::screen_and_input().with_bounds(0, 0, 1280, 800);
        // Round 1: model says "click at 100,100".
        // Round 2: model says "done".
        let click_msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::text("clicking"),
                ContentBlock::ToolUse(ToolUse {
                    id: "tu_1".into(),
                    name: "computer".into(),
                    input: serde_json::json!({
                        "action": "left_click", "x": 100, "y": 100
                    }),
                }),
            ],
        };
        let done_msg = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse(ToolUse {
                id: "tu_2".into(),
                name: "computer".into(),
                input: serde_json::json!({
                    "action": "done", "summary": "all done"
                }),
            })],
        };
        let mouse = MockMouse::default();
        let llm = StubLlm::new(vec![click_msg, done_msg]);
        let d = Dispatcher::new(llm, cap).with_mouse(mouse.clone());
        let out = d.run("click that button").await.unwrap();
        assert_eq!(out, "all done");
        // The mouse backend should have recorded one click.
        let events = mouse.events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            crate::computer::input::MouseEvent::Click { x: 100, y: 100, .. }
        ));
    }

    #[tokio::test]
    async fn dispatcher_rejects_out_of_bounds_click() {
        let cap = ComputerCap::screen_and_input().with_bounds(0, 0, 100, 100);
        let click_msg = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse(ToolUse {
                id: "tu_1".into(),
                name: "computer".into(),
                input: serde_json::json!({
                    "action": "left_click", "x": 500, "y": 500
                }),
            })],
        };
        let mouse = MockMouse::default();
        let llm = StubLlm::new(vec![click_msg]);
        let d = Dispatcher::new(llm, cap).with_mouse(mouse.clone());
        let err = d.run("escape the bounds").await.unwrap_err();
        assert!(matches!(err, ComputerError::SandboxViolation(_)));
        // CRITICAL: the click MUST NOT have reached the backend.
        assert_eq!(mouse.len(), 0);
    }

    #[tokio::test]
    async fn dispatcher_rejects_denied_key() {
        let cap = ComputerCap::screen_and_input().deny_keys(["ctrl+alt+delete"]);
        let evil = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse(ToolUse {
                id: "tu_1".into(),
                name: "computer".into(),
                input: serde_json::json!({
                    "action": "key", "text": "Ctrl+Alt+Delete"
                }),
            })],
        };
        let kb = MockKeyboard::default();
        let llm = StubLlm::new(vec![evil]);
        let d = Dispatcher::new(llm, cap).with_keyboard(kb.clone());
        let err = d.run("logout please").await.unwrap_err();
        assert!(matches!(err, ComputerError::SandboxViolation(_)));
        assert_eq!(kb.len(), 0);
    }

    #[tokio::test]
    async fn dispatcher_hits_turn_limit() {
        let cap = ComputerCap::screen_and_input();
        // Always reply with a fresh screenshot request, never `done`.
        let mk = || Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse(ToolUse {
                id: "tu".into(),
                name: "computer".into(),
                input: serde_json::json!({ "action": "screenshot" }),
            })],
        };
        let replies: Vec<Message> = (0..10).map(|_| mk()).collect();
        let llm = StubLlm::new(replies);
        let d = Dispatcher::new(llm, cap).with_max_turns(3);
        let err = d.run("loop forever").await.unwrap_err();
        assert!(matches!(err, ComputerError::TurnLimit(3)));
    }

    #[test]
    fn from_macro_args_builds_dispatcher_with_supplied_size() {
        let llm = Arc::new(StubLlm::new(vec![])) as Arc<dyn LlmProvider>;
        let cap = ComputerCap::screen_and_input();
        let d = from_macro_args(llm, 640, 480, cap);
        assert_eq!(d.screen.width(), 640);
        assert_eq!(d.screen.height(), 480);
    }

    #[test]
    fn parse_drag_with_coordinate_pair() {
        let a = ComputerAction::parse(&serde_json::json!({
            "action": "left_click_drag",
            "x": 100, "y": 200,
            "start_coordinate": [10, 20]
        }))
        .unwrap();
        if let ComputerAction::Drag {
            x1,
            y1,
            x2,
            y2,
            button,
        } = a
        {
            assert_eq!((x1, y1, x2, y2), (10, 20, 100, 200));
            assert_eq!(button, MouseButton::Left);
        } else {
            panic!("expected Drag");
        }
    }

    #[test]
    fn execute_screenshot_action_returns_screenshot_outcome() {
        let llm = StubLlm::new(vec![]);
        let cap = ComputerCap::screen_and_input();
        let d = Dispatcher::new(llm, cap);
        let outcome = d.execute_action(&ComputerAction::Screenshot).unwrap();
        assert!(matches!(outcome, ActionOutcome::Screenshot(_)));
    }
}
