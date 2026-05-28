# `std.computer` — Anthropic Computer Use as a First-Class Capability

**Status:** v0.30 Track C (Anthropic Computer Use integration)
**Scope:** `crates/mty-stdlib/src/computer/`, `crates/mty-macros/src/stdlib/computer_use.rs`, the `@computer_use` attribute, and the dispatcher's wire shape against the Anthropic Messages API.

This page is the design rationale + threat model for the `std.computer`
surface. The user-facing API reference lives in
`docs/reference/stdlib/computer.md` (a thin pointer to the rustdoc).

## TL;DR

Anthropic's [Computer Use](https://docs.anthropic.com/en/docs/agents-and-tools/computer-use)
puts the model in front of a screenshot and lets it emit mouse / keyboard
events. Mighty wraps that loop in three load-bearing pieces:

1. **`std.computer.dispatcher`** — the agent loop that screenshots → asks
   the model → parses the `tool_use` block → executes it → repeats. The
   loop terminates on a `done` action or after `MAX_TURNS = 30` rounds.
2. **`std.computer.sandbox::ComputerCap`** — the runtime capability gate
   that every action passes through *before* it reaches the OS. The cap
   carries opt-in click bounds, an opt-in key deny-list, and an opt-in
   per-run action quota.
3. **`@computer_use(width:..., height:..., cap:...)`** — the agent
   decorator. Declares the capability requirement at the source level so
   the type checker rejects an agent that calls `std.computer.run(...)`
   without the matching cap.

## Why is the sandbox in stdlib, not user code?

Computer Use is the broadest capability surface Mighty exposes — a single
click can do anything the OS user could do. Three failure modes are easy
to hit if every caller hand-rolls the safety net:

1. **Prompt injection via screenshot.** A popup, banner, or email subject
   line in the screenshot can convince the model to click a system menu
   item. The cap's `bounds` + `deny_keys` reject the dangerous action
   *after* the model has decided to fire it but *before* the OS sees it.
2. **Loop runaway.** Without a turn limit a model that disagrees with
   itself can burn through tokens forever. `MAX_TURNS` bounds this; the
   per-run action quota bounds the OS-side blast radius too.
3. **Unaudited input.** If the dispatcher fires events directly the
   action history is opaque. Every cap-validated action also lands in
   the dispatcher's structured trace so callers can replay it.

## Layout

```
crates/mty-stdlib/src/computer/
  mod.rs          re-exports + ComputerError + module docs
  screen.rs       Screen / ScreenBackend / MockScreen, Screenshot
  input.rs        Mouse / Keyboard, MouseButton / Key, mock backends
  sandbox.rs      ComputerCap + SandboxViolation + Bounds
  dispatcher.rs   Dispatcher::run + ComputerAction (typed loop)
crates/mty-macros/src/stdlib/computer_use.rs
  @computer_use macro: parses width/height/cap/model + synth spec fn
crates/mty-stdlib/src/llm/anthropic.rs
  AnthropicClient::ask_with_computer + computer_tool_array helper
```

## The capability gate

`ComputerCap` is built via a fluent builder. The two methods that grant
permission (`allow_screen`, `allow_input`) are the only fields the type
system cares about; everything else (`bounds`, `deny_keys`,
`max_actions_per_run`) is opt-in runtime tightening.

```rust
use mty_stdlib::computer::ComputerCap;

let cap = ComputerCap::builder()
    .allow_screen()
    .allow_input()
    .with_bounds(0, 0, 1280, 800)
    .deny_keys(["ctrl+alt+delete", "cmd+q", "win+l"])
    .max_actions_per_run(50)
    .build();
```

Every dispatcher action calls one of `check_screen`, `check_click`,
`check_type_text`, or `check_key` before reaching the backend. Failure
returns `SandboxViolation`, which the dispatcher converts to
`ComputerError::SandboxViolation` and propagates to the caller —
critically, the action is NOT executed.

### Bounds semantics

Bounds are half-open: `x_min <= x < x_max && y_min <= y < y_max`. A
click on the upper edge is rejected, matching the convention used by
all other half-open ranges in the codebase.

### Deny-list normalisation

Key strings are normalised before lookup:

- Lowercased.
- Modifier order canonicalised to `ctrl | alt | shift | meta | cmd | win`.
- Synonyms collapsed: `control → ctrl`, `command → cmd`,
  `super → win`.

So `Ctrl+Alt+Del` and `alt+ctrl+del` hash to the same entry; the
deny-list always wins.

### Per-run action quota

`max_actions_per_run(N)` caps the total number of cap-validated actions
per `Dispatcher::run` invocation. The counter is reset at the start of
each `run()`. Hitting the quota raises `SandboxViolation::RateLimited`
and the run terminates.

## Threat model

What CAN a malicious payload visible in a screenshot do?

| Threat                              | Mitigated by                                            | Status |
|------------------------------------|--------------------------------------------------------|--------|
| Click outside the agent's viewport  | `ComputerCap::with_bounds`                              | full   |
| Press an OS-level chord             | `ComputerCap::deny_keys`                                | full   |
| Spam clicks beyond budget           | `ComputerCap::max_actions_per_run`                      | full   |
| Read clipboard / paste secrets      | (out of scope; requires OS-level access policy)         | none   |
| Trigger an indefinite loop          | `Dispatcher::with_max_turns` + `MAX_TURNS = 30` default | full   |
| Exfiltrate via DNS                  | (orthogonal — `std.net` capability gate)                | none   |

What CAN'T it do?

- Skip the cap. Every action goes through `Dispatcher::execute_action`
  which calls `cap.check_*` before touching the backend. There is no
  "fast path".
- Convince the dispatcher to execute a `tool_use` from a non-`computer`
  tool. The dispatcher hard-codes `tu.name == "computer"` and returns
  a tool-error `ToolResult` for anything else.
- Read the cap-validated action log. The dispatcher's trace is not
  shipped to the model; only the explicit `ack` / `error` summary
  goes back.

## Provider integration

The Anthropic Messages client gets one new method,
`AnthropicClient::ask_with_computer(req, (width, height))`. It:

1. Calls `build_body` (so system / temperature / max_tokens stay
   consistent with `complete`).
2. Replaces the `tools` array with the `computer_20241022` spec via
   `computer_tool_array(width, height)`.
3. Adds the `anthropic-beta: computer-use-2024-10-22` header so
   keys without the computer-use beta access bit fail closed with a
   clear 403.
4. Parses the reply through the same `MessagesResponse` shape as
   `complete`; the model's `tool_use` blocks land in the typed
   `Message` and the dispatcher picks them up.

The dispatcher's `run()` loop uses the regular `complete()` path (not
`ask_with_computer`) — when the tool's wire shape is already the
generic `{ name, description, input_schema }` triple with the
provider-type discriminator embedded in the schema, the regular path
accepts it. The `ask_with_computer` helper exists for direct callers
who want the canonical wire shape + beta header without going through
the dispatcher.

## The `@computer_use` macro

```mty
@computer_use(width: 1280, height: 800, cap: computer.screen + computer.input)
agent BrowserOperator: BrowserOp {
  on Run(goal) {
    goal
  }
}
```

The macro expands by:

1. Parsing `width` / `height` / `cap` / `model` from the attribute
   arguments. Validates each shape, emits MT6017–MT6020 on failure.
2. Synthesising `fn __computer_use_spec_<AGENT>() -> Str` whose body
   returns a JSON spec the runtime reads to wire the dispatcher.
3. Returning the user's original `agent` decl unchanged so it remains
   spawnable from non-decorated call sites.

The spec carries the cap text verbatim so the runtime cap-builder
(`from_macro_args`) can hand the agent's handler a pre-built
`ComputerCap` instance via the dispatcher's scope.

## CI safety

The default `MockScreen` / `MockMouse` / `MockKeyboard` backends mean
the test suite never grabs a real display or moves the real cursor —
they record events into in-memory logs the tests assert against.

Real per-OS backends (Win32 BitBlt, X11 xcb, CGDisplay) are gated
behind the off-by-default feature flags `computer-windows`,
`computer-linux`, `computer-macos`. Production callers who want the
OS backend opt in explicitly; CI never sees the dep tree those
backends pull in (xdotool / libxcb / Cocoa).

## v0.31 follow-ups

- **Virtual display backend.** Wrap an Xvfb / virtual-framebuffer
  process so computer-use runs in an isolated display the host user
  never sees. Removes the "model could move my mouse" objection
  entirely.
- **Multi-monitor support.** The dispatcher currently asks the model
  for a single primary display. Anthropic's tool spec already carries
  `display_number`; surface it through the dispatcher.
- **Tainted action propagation.** When Track A's `Tainted<T>` ships,
  screenshot bytes become `Tainted<Vec<u8>>` and model-derived
  actions become `Tainted<ComputerAction>`. The cap check already
  runs before execution; this is a typing change, not a semantic one.
- **Action trace export.** The dispatcher already records every
  cap-validated action internally; surface it through `Dispatcher::
  trace()` so callers can replay / audit.
- **Wire-shape upgrade.** Anthropic's `computer-use-2024-10-22` will
  age out; the dispatcher's `ComputerAction::parse` keeps both old
  and new field shapes via the `coordinate: [x, y]` vs `x, y`
  branches, so the schema upgrade is a one-arm extension.

## Test inventory

- **Unit (`src/computer/*.rs`):** 48 tests covering parse / sandbox /
  screen / input / dispatcher loop / base64 encoder.
- **Integration (`tests/computer_use_anthropic.rs`):** 6 wiremock'd
  end-to-end tests against the Anthropic Messages wire shape.
- **Macro (`src/stdlib/computer_use.rs`):** 13 in-module tests +
  8 integration tests in `tests/computer_use_macro.rs`.
- **Diagnostics:** explainability check on MT6017–MT6020 in
  `crates/mty-macros/src/diag.rs`.

That's 75+ tests across the new surface; the unit + macro suites run
in < 1 s wall time on a warm cache.
