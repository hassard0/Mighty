# Chapter 20 — Computer use

Example:
[`36_computer_use.mty`](https://github.com/hassard0/Mighty/blob/main/examples/36_computer_use.mty).

Anthropic's **Computer Use** feature lets an LLM drive a virtual
display, keyboard, and mouse. Mighty wraps it as a Mighty tool
with a **capability-typed sandbox**: the static bounds are
declared on the `@computer_use` decorator, and the runtime
enforces them.

Shipped in v0.30 Track C.

## The shape

```mty
@computer_use(
  display      = "1280x720",
  allowed_apps = ["chromium", "kitty"],
  fs_write     = ["/tmp/agent/**"],
  net          = false,
)
fn drive_browser(client: AnthropicClient, task: Str) -> Tainted[Str] {
  std.computer.run(client, task)
}
```

The decorator generates:

- A `ComputerUseSandbox` declared from the four static knobs.
- A `__computer_use_register_drive_browser(...)` that plugs the
  sandbox into the Computer Use tool registry.
- A wrapper around `std.computer.run` that enforces the sandbox
  on every X11 / wayland / display call from the spawned VM.

The bounds are **static**. You cannot pass them as runtime
arguments. A misbehaving Computer Use loop cannot spawn an
arbitrary process, cannot render to a different display, cannot
write outside `/tmp/agent/**`.

## What the runtime enforces

| Knob | Enforced by |
|---|---|
| `display = "WxH"` | The VM is booted with exactly that display geometry; resize requests are denied at the X server level. |
| `allowed_apps` | The VM's `exec` syscall is bounded to the listed binaries by name; everything else returns EACCES. |
| `fs_write` | All `write(2)` calls from inside the VM consult the glob list; out-of-allowlist writes return EROFS. |
| `net = false / true` | The VM has no host-network bridge; outbound packets are dropped at the link layer. |

The list mirrors the `sandbox` block in tour
[chapter 10](10-capabilities.md), with the `@computer_use`
decorator narrowing further to the Computer Use VM specifically.

## A typed return

The fn return type is `Tainted[Str]`. The display contents (and
anything the LLM might say about them) are tainted from the
moment they cross back into Mighty's process — see tour
[chapter 18](18-taint-types.md) for the sink-rejection story.

## Try it

```bash
mty check examples/36_computer_use.mty
```

Prints `ok`. A real run requires an Anthropic API key and a host
that can boot a display VM (Linux + libvirt + a Chromium image;
the macOS / Windows paths are on the v0.31 roadmap — "treat
cross-platform headless display as the real problem" per the
v0.30 follow-up notes).

## See also

- [`docs/internals/computer-use.md`](../internals/computer-use.md) —
  implementation: how the VM is booted, how the sandbox is
  threaded through the WIT bindings, how the trace records every
  click + keystroke for replay.
- The v0.31 follow-up "Demo 10 — browser operator" composes this
  with the SWE-bench tool set to drive a real browser agent.
