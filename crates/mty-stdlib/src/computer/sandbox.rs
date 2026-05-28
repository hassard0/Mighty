//! Capability gate for `std.computer`.
//!
//! [`ComputerCap`] is the runtime value behind the `cap:
//! computer.screen + computer.input` declaration on `@computer_use`
//! agents. Every action the dispatcher executes is validated against
//! the cap *before* it reaches the OS:
//!
//! 1. **Permission** — `cap.allows_screen() / allows_input()`. The
//!    macro-generated agent declaration carries `computer.screen` /
//!    `computer.input` flags; calling a method without the matching
//!    permission raises [`SandboxViolation::Permission`].
//! 2. **Bounds** — `cap.with_bounds(x_min, y_min, x_max, y_max)`
//!    rejects clicks outside the rectangle. Default cap is unbounded
//!    — callers MUST opt in.
//! 3. **Key deny-list** — `cap.deny_keys(&["ctrl+alt+delete",
//!    "cmd+q"])` rejects the listed chords no matter what.
//! 4. **Per-turn rate limit** (v0.30 baseline: optional) —
//!    `cap.max_actions_per_run(N)` caps how many actions a single
//!    [`Dispatcher::run`](super::dispatcher::Dispatcher::run) may
//!    execute.
//!
//! The cap is `Clone` because the agent's [`Dispatcher`] takes ownership
//! of one copy while the caller keeps another for inspection /
//! reconfiguration mid-run.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

/// The runtime capability value.
///
/// Built via the [builder methods](Self::screen_and_input) and
/// passed to [`Dispatcher::new`](super::dispatcher::Dispatcher::new).
/// Internally uses `Arc<Mutex<…>>` for the action counter so multiple
/// clones share the per-run rate limit.
#[derive(Debug, Clone)]
pub struct ComputerCap {
    allow_screen: bool,
    allow_input: bool,
    bounds: Option<Bounds>,
    deny_keys: BTreeSet<String>,
    max_actions: Option<u32>,
    counter: Arc<Mutex<u32>>,
}

/// A bounding rectangle a click must lie inside. Half-open on the
/// upper edge: `x_min <= x < x_max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub x_min: u32,
    pub y_min: u32,
    pub x_max: u32,
    pub y_max: u32,
}

impl Bounds {
    pub fn new(x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> Self {
        Self {
            x_min,
            y_min,
            x_max,
            y_max,
        }
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x_min && x < self.x_max && y >= self.y_min && y < self.y_max
    }
}

/// Reasons the sandbox can reject an action.
///
/// Implements `Error` so callers can `?`-propagate through the
/// [`ComputerError`](super::ComputerError) top-level.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SandboxViolation {
    /// The cap does not authorise this action class (screen or input).
    #[error("permission denied: capability `{0}` not granted")]
    Permission(&'static str),

    /// The action targets a point outside the cap's bounding box.
    #[error("out of bounds: ({x},{y}) is outside the cap's allowed region ({x_min},{y_min})..({x_max},{y_max})")]
    OutOfBounds {
        x: u32,
        y: u32,
        x_min: u32,
        y_min: u32,
        x_max: u32,
        y_max: u32,
    },

    /// A keypress targets a chord on the deny-list.
    #[error("denied key: `{0}` is on the cap deny-list")]
    DeniedKey(String),

    /// The agent exceeded the per-run action quota.
    #[error(
        "rate limited: cap allows {limit} actions per run; the agent attempted {limit_plus_one}"
    )]
    RateLimited { limit: u32, limit_plus_one: u32 },
}

/// Fluent builder for [`ComputerCap`]. Use [`ComputerCap::builder`] to
/// construct one without auto-granting any permission.
#[derive(Debug, Clone, Default)]
pub struct ComputerCapBuilder {
    allow_screen: bool,
    allow_input: bool,
    bounds: Option<Bounds>,
    deny_keys: BTreeSet<String>,
    max_actions: Option<u32>,
}

impl ComputerCapBuilder {
    pub fn allow_screen(mut self) -> Self {
        self.allow_screen = true;
        self
    }

    pub fn allow_input(mut self) -> Self {
        self.allow_input = true;
        self
    }

    pub fn with_bounds(mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> Self {
        self.bounds = Some(Bounds::new(x_min, y_min, x_max, y_max));
        self
    }

    pub fn deny_keys<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for k in keys {
            self.deny_keys.insert(normalise_key_string(k.as_ref()));
        }
        self
    }

    pub fn max_actions_per_run(mut self, n: u32) -> Self {
        self.max_actions = Some(n);
        self
    }

    pub fn build(self) -> ComputerCap {
        ComputerCap {
            allow_screen: self.allow_screen,
            allow_input: self.allow_input,
            bounds: self.bounds,
            deny_keys: self.deny_keys,
            max_actions: self.max_actions,
            counter: Arc::new(Mutex::new(0)),
        }
    }
}

impl ComputerCap {
    /// Fluent builder. Starts with no permissions and no bounds.
    pub fn builder() -> ComputerCapBuilder {
        ComputerCapBuilder::default()
    }

    /// Convenience: cap that grants both screen + input but no
    /// bounds, no deny-list, no rate limit.
    ///
    /// The "default sandbox" — documented loudly as "no bounds, no
    /// deny list". Callers wanting safety MUST chain `.with_bounds()`
    /// + `.deny_keys()` on the builder.
    pub fn screen_and_input() -> Self {
        Self::builder().allow_screen().allow_input().build()
    }

    /// Capability-only cap that allows screen capture but no input
    /// (useful for read-only assistants).
    pub fn screen_only() -> Self {
        Self::builder().allow_screen().build()
    }

    /// Rebuild this cap with the supplied bounds — used by tests and
    /// by `Dispatcher::with_*` chains. The action counter is reset.
    #[must_use]
    pub fn with_bounds(mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> Self {
        self.bounds = Some(Bounds::new(x_min, y_min, x_max, y_max));
        self.counter = Arc::new(Mutex::new(0));
        self
    }

    /// Extend the deny-list. Idempotent — duplicates are coalesced.
    #[must_use]
    pub fn deny_keys<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for k in keys {
            self.deny_keys.insert(normalise_key_string(k.as_ref()));
        }
        self
    }

    /// Set the per-run action limit.
    #[must_use]
    pub fn max_actions_per_run(mut self, n: u32) -> Self {
        self.max_actions = Some(n);
        self
    }

    pub fn allows_screen(&self) -> bool {
        self.allow_screen
    }

    pub fn allows_input(&self) -> bool {
        self.allow_input
    }

    pub fn bounds(&self) -> Option<Bounds> {
        self.bounds
    }

    pub fn denied_keys(&self) -> Vec<String> {
        self.deny_keys.iter().cloned().collect()
    }

    pub fn max_actions(&self) -> Option<u32> {
        self.max_actions
    }

    /// Reset the per-run counter — called by the dispatcher at the
    /// start of every `run()`.
    pub fn reset_counter(&self) {
        *self.counter.lock().unwrap() = 0;
    }

    /// Current consumed-actions count.
    pub fn actions_consumed(&self) -> u32 {
        *self.counter.lock().unwrap()
    }

    /// Check + tick the per-run action counter. Called by the
    /// dispatcher before EVERY action. Returns
    /// [`SandboxViolation::RateLimited`] when the cap is exceeded.
    pub fn check_and_tick(&self) -> Result<(), SandboxViolation> {
        let mut c = self.counter.lock().unwrap();
        if let Some(limit) = self.max_actions {
            if *c >= limit {
                return Err(SandboxViolation::RateLimited {
                    limit,
                    limit_plus_one: *c + 1,
                });
            }
        }
        *c += 1;
        Ok(())
    }

    /// Validate a screen-capture call.
    pub fn check_screen(&self) -> Result<(), SandboxViolation> {
        if !self.allow_screen {
            return Err(SandboxViolation::Permission("computer.screen"));
        }
        self.check_and_tick()
    }

    /// Validate a click / move at `(x, y)`.
    pub fn check_click(&self, x: u32, y: u32) -> Result<(), SandboxViolation> {
        if !self.allow_input {
            return Err(SandboxViolation::Permission("computer.input"));
        }
        if let Some(b) = self.bounds {
            if !b.contains(x, y) {
                return Err(SandboxViolation::OutOfBounds {
                    x,
                    y,
                    x_min: b.x_min,
                    y_min: b.y_min,
                    x_max: b.x_max,
                    y_max: b.y_max,
                });
            }
        }
        self.check_and_tick()
    }

    /// Validate a `type_text` call. Per-character bounds are not
    /// enforced — only the chord deny-list applies.
    pub fn check_type_text(&self, _text: &str) -> Result<(), SandboxViolation> {
        if !self.allow_input {
            return Err(SandboxViolation::Permission("computer.input"));
        }
        self.check_and_tick()
    }

    /// Validate a `key_press` against the deny-list.
    pub fn check_key(&self, key_string: &str) -> Result<(), SandboxViolation> {
        if !self.allow_input {
            return Err(SandboxViolation::Permission("computer.input"));
        }
        let normalised = normalise_key_string(key_string);
        if self.deny_keys.contains(&normalised) {
            return Err(SandboxViolation::DeniedKey(normalised));
        }
        self.check_and_tick()
    }
}

/// Normalise a key chord string: lowercase, `+`-separated, sort modifier
/// order so `Ctrl+Alt+Del` and `alt+ctrl+del` hash to the same entry.
///
/// The modifier sort order is `ctrl|alt|shift|meta|cmd|win` — anything
/// else is treated as the actual key half (and stays at the end).
pub(crate) fn normalise_key_string(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    if !lower.contains('+') {
        return lower;
    }
    let parts: Vec<&str> = lower.split('+').map(str::trim).collect();
    let modifier_rank = |m: &str| -> Option<u8> {
        match m {
            "ctrl" | "control" => Some(0),
            "alt" => Some(1),
            "shift" => Some(2),
            "meta" => Some(3),
            "cmd" | "command" => Some(4),
            "win" | "super" => Some(5),
            _ => None,
        }
    };
    let mut modifiers: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|p| modifier_rank(p).is_some())
        .collect();
    let keys: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|p| modifier_rank(p).is_none())
        .collect();
    modifiers.sort_by_key(|m| modifier_rank(m).unwrap());
    let mut out = String::new();
    for (i, m) in modifiers.iter().chain(keys.iter()).enumerate() {
        if i > 0 {
            out.push('+');
        }
        // Canonicalise control->ctrl, command->cmd, super->win for display.
        let canon = match *m {
            "control" => "ctrl",
            "command" => "cmd",
            "super" => "win",
            other => other,
        };
        out.push_str(canon);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cap_grants_nothing() {
        let cap = ComputerCap::builder().build();
        assert!(!cap.allows_screen());
        assert!(!cap.allows_input());
        assert!(cap.bounds().is_none());
        assert!(cap.denied_keys().is_empty());
        assert!(matches!(
            cap.check_screen().unwrap_err(),
            SandboxViolation::Permission("computer.screen")
        ));
        assert!(matches!(
            cap.check_click(0, 0).unwrap_err(),
            SandboxViolation::Permission("computer.input")
        ));
    }

    #[test]
    fn screen_and_input_grants_both() {
        let cap = ComputerCap::screen_and_input();
        assert!(cap.allows_screen());
        assert!(cap.allows_input());
        cap.check_screen().unwrap();
        cap.check_click(10, 10).unwrap();
        cap.check_type_text("hi").unwrap();
    }

    #[test]
    fn bounds_rejects_click_outside() {
        let cap = ComputerCap::screen_and_input().with_bounds(100, 100, 200, 200);
        assert!(cap.check_click(150, 150).is_ok());
        let err = cap.check_click(50, 150).unwrap_err();
        assert!(matches!(err, SandboxViolation::OutOfBounds { .. }));
        let err = cap.check_click(150, 250).unwrap_err();
        assert!(matches!(err, SandboxViolation::OutOfBounds { .. }));
        // Half-open on the upper edge.
        let err = cap.check_click(200, 150).unwrap_err();
        assert!(matches!(err, SandboxViolation::OutOfBounds { .. }));
    }

    #[test]
    fn bounds_contains_is_half_open() {
        let b = Bounds::new(0, 0, 10, 10);
        assert!(b.contains(0, 0));
        assert!(b.contains(9, 9));
        assert!(!b.contains(10, 9));
        assert!(!b.contains(9, 10));
    }

    #[test]
    fn deny_list_blocks_normalised_chord() {
        let cap = ComputerCap::screen_and_input().deny_keys(["Ctrl+Alt+Del", "CMD+Q"]);
        // Re-ordered modifier still matches.
        assert!(matches!(
            cap.check_key("Alt+Ctrl+Del").unwrap_err(),
            SandboxViolation::DeniedKey(_)
        ));
        // Plain key not on list passes.
        cap.check_key("Enter").unwrap();
    }

    #[test]
    fn rate_limit_caps_action_count() {
        let cap = ComputerCap::screen_and_input().max_actions_per_run(3);
        cap.check_click(1, 1).unwrap();
        cap.check_click(2, 2).unwrap();
        cap.check_click(3, 3).unwrap();
        let err = cap.check_click(4, 4).unwrap_err();
        assert!(matches!(err, SandboxViolation::RateLimited { .. }));
        // After reset the budget refills.
        cap.reset_counter();
        cap.check_click(5, 5).unwrap();
    }

    #[test]
    fn actions_consumed_tracks_calls() {
        let cap = ComputerCap::screen_and_input();
        assert_eq!(cap.actions_consumed(), 0);
        cap.check_screen().unwrap();
        cap.check_click(0, 0).unwrap();
        cap.check_type_text("x").unwrap();
        cap.check_key("Enter").unwrap();
        assert_eq!(cap.actions_consumed(), 4);
    }

    #[test]
    fn clone_shares_counter() {
        let cap = ComputerCap::screen_and_input().max_actions_per_run(2);
        let other = cap.clone();
        cap.check_click(0, 0).unwrap();
        other.check_click(0, 0).unwrap();
        // Third call against either handle should rate-limit.
        let err = cap.check_click(0, 0).unwrap_err();
        assert!(matches!(err, SandboxViolation::RateLimited { .. }));
    }

    #[test]
    fn normalise_orders_modifiers_canonically() {
        assert_eq!(normalise_key_string("ctrl+alt+del"), "ctrl+alt+del");
        assert_eq!(normalise_key_string("alt+ctrl+del"), "ctrl+alt+del");
        assert_eq!(normalise_key_string("shift+alt+x"), "alt+shift+x");
        assert_eq!(normalise_key_string("CMD+Q"), "cmd+q");
        assert_eq!(normalise_key_string("Enter"), "enter");
        assert_eq!(normalise_key_string("Control+L"), "ctrl+l");
    }

    #[test]
    fn screen_only_cap_blocks_input() {
        let cap = ComputerCap::screen_only();
        cap.check_screen().unwrap();
        let err = cap.check_click(0, 0).unwrap_err();
        assert!(matches!(
            err,
            SandboxViolation::Permission("computer.input")
        ));
    }

    #[test]
    fn builder_chain_round_trips_fields() {
        let cap = ComputerCap::builder()
            .allow_screen()
            .allow_input()
            .with_bounds(10, 20, 30, 40)
            .deny_keys(["ctrl+w"])
            .max_actions_per_run(5)
            .build();
        assert!(cap.allows_screen());
        assert!(cap.allows_input());
        assert_eq!(cap.bounds(), Some(Bounds::new(10, 20, 30, 40)));
        assert_eq!(cap.denied_keys(), vec!["ctrl+w".to_string()]);
        assert_eq!(cap.max_actions(), Some(5));
    }
}
