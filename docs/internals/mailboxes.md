# Mailboxes (slice 7)

**Module:** `sdust_runtime::mailbox`
**Spec:** §25.3 Mailboxes

## Shape

Each agent owns one `Mailbox`:

```rust
pub struct Mailbox {
    tx: mpsc::Sender<MessageFrame>,
    rx: parking_lot::Mutex<Option<mpsc::Receiver<MessageFrame>>>,
    capacity: usize,
    policy: SendPolicy,
}
```

The mailbox is a tokio bounded MPSC channel. The receiver is taken
once by the agent's run loop via `take_receiver()`.

## MessageFrame

```rust
pub struct MessageFrame {
    pub proto_msg: String,                         // e.g. "Ping", "Query"
    pub payload: SmallPayload,                     // inline vec or empty
    pub reply: Option<oneshot::Sender<RuntimeResult<Value>>>,
    pub deadline: Option<Instant>,
    pub seq: u64,
}
```

`SmallPayload::Empty` skips heap-allocation when the message has no
args. `SmallPayload::Inline(Vec<Value>)` is the general case.

Two constructors:

- `fire_and_forget(msg, payload)` — no reply oneshot, used by `!Msg`
- `ask(msg, payload, deadline)` — returns `(frame, oneshot::Receiver)`
  the caller awaits

## Send policies

```rust
pub enum SendPolicy {
    Block,  // sender awaits capacity (default)
    Drop,   // try_send; ignore Full errors
    Fail,   // try_send; SD5012 MailboxFull on Full
}
```

Per A40 the default depth is **1024** and the default policy is
**Block** — sender backpressures rather than tripping a trap. Programs
that want explicit drop-or-fail semantics set the `mb` and `mb_policy`
budget entries.

## Spec §25.3 fast-path comparison

| Spec fast-path feature             | Slice-7 form |
|------------------------------------|--------------|
| fixed-size message frame           | `MessageFrame` struct (compile-time sized) |
| inline small payload               | `SmallPayload::Inline(Vec<Value>)` |
| pointer to arena/owned payload     | (via `Value::Ref` for now — full arena ptr in slice 8) |
| protocol message ID                | `proto_msg: String` (slice 7); interned u32 in slice 8 |
| reply handle if needed             | `Option<oneshot::Sender<...>>` |

## Tests

`crates/sdust-runtime/tests/mailbox_basic.rs` exercises:

1. FIFO ordering under bounded capacity.
2. `try_send` Fail-policy behaviour on full mailbox.
3. Ask/reply round-trip via the oneshot.
4. Drop policy silently discards on full.

## See also

- `docs/internals/runtime.md` — how the agent loop drains the mailbox
- `docs/internals/budgets.md` — `mb` budget controls the capacity
- `docs/spec/v0.1-amendments.md` — A40
