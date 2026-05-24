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
    Fail,   // try_send; MT5012 MailboxFull on Full
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

## v0.3 slab-pool backing (closes A40)

Slice 7 let `tokio::sync::mpsc` own every `MessageFrame` on the
heap. v0.3 adds a per-mailbox **slab pool** of fixed-size payload
slots that the mpsc channel borrows from on each `send`. The mpsc
backbone still owns ordering; the slab pool caps in-flight memory
and gives slots a stable index for cache-friendly reuse.

```text
  ┌──────────────────────────────────────────────────────────────┐
  │ Mailbox                                                      │
  │                                                              │
  │   slab : SlabPool (1024 × 64-byte slots by default)          │
  │   tx   : mpsc::Sender<MessageFrame>  (cap == slab.capacity)  │
  │   rx   : mpsc::Receiver<MessageFrame>                        │
  │                                                              │
  │   send(frame):                                               │
  │     1. encode frame metadata into a pool slot (acquire)      │
  │     2. attach the PooledFrame handle to `frame._slab`        │
  │     3. push frame onto the mpsc channel                      │
  └──────────────────────────────────────────────────────────────┘
```

### Slot layout

Each slot is

```rust
pub struct Slot {
    pub inline:   Vec<u8>,             // capacity == inline_bytes (default 64)
    pub overflow: Option<Box<[u8]>>,    // present when payload > inline_bytes
    pub used:     usize,                // filled bytes
}
```

Payloads up to `inline_bytes` live entirely on the pre-allocated
inline buffer — no per-send allocation. Larger payloads spill into
a one-shot `Box<[u8]>` per slot; the slot still tracks the size so
backpressure accounting is accurate.

### Tunables

| Constant                    | Default | Description                          |
|-----------------------------|--------:|--------------------------------------|
| `DEFAULT_POOL_SIZE`         |   1024  | slots per `SlabPool::default()`      |
| `DEFAULT_INLINE_BYTES`      |     64  | inline bytes per slot                |
| `Mailbox::new(cap, policy)` |       — | pool size == `cap` (channel cap too) |

Explicit layout: `SlabPool::with_layout(capacity, inline_bytes)`
followed by `Mailbox::with_pool(cap, policy, pool)`.

### Reuse semantics

The pool's free-list is **LIFO** for cache locality: the most-
recently-released slot is the next one handed out. This does **not**
reorder messages — message order is owned by the mpsc channel; the
slab indices are non-observable from user programs.

When the pool is exhausted (every slot in use), `acquire_or_overflow`
returns a *standalone* `PooledFrame` whose payload bytes live
outside the pool — no backpressure on the slot side, but the channel
itself still bounds depth.

### Send policies (unchanged from slice 7)

| Policy   | Pool full + Channel full behaviour                    |
|----------|-------------------------------------------------------|
| `Block`  | Sender awaits both: overflow slot + channel capacity. |
| `Drop`   | Best-effort `try_send`; failure silently discards.    |
| `Fail`   | Returns MT5012 `MailboxFull`.                         |

### Statistics + introspection

`Mailbox::introspect()` returns a `MailboxStats { capacity,
channel_used, slab_used, slab_capacity }` snapshot. `SlabPool::stats()`
exposes lifetime (acquired, released) counters; tests assert no
leaks by checking `acquired == released` after teardown.

### Migration

The public `Mailbox` API is unchanged. Existing callers see the
same `send` / `try_send` / `take_receiver` semantics; the slab pool
is an internal implementation detail attached via the new
`MessageFrame::_slab` field (`pub(crate)`-only).

### Tests

- `crates/sdust-runtime/tests/mailbox_slab_pool.rs` — FIFO under
  reuse, Block backpressure, Fail MT5012, overflow path, slot
  leak-detection via `pool.stats()`.
- `crates/sdust-runtime/tests/mailbox_basic.rs` — slice-7 surface
  contracts (unchanged).

## See also

- `docs/internals/runtime.md` — how the agent loop drains the mailbox
- `docs/internals/budgets.md` — `mb` budget controls the capacity
- `docs/spec/v0.1-amendments.md` — A40, A70+ (v0.3)
