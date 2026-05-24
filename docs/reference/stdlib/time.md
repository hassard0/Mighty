# `std.time`

Monotonic clock + duration arithmetic.

## Surface

```sd
fn now(clock: Clock) -> Instant
fn sleep(clock: Clock, dur: Duration) -> Unit

impl Instant {
  fn elapsed_since(other: Instant) -> Duration
}
```

In Rust:

```rust
pub fn now(_cap: Clock) -> Instant;
pub async fn sleep(_cap: Clock, dur: Duration);
pub fn sleep_blocking(_cap: Clock, dur: Duration);
```

## Capability model

`now` and `sleep` take a `Clock` capability — a unit type today, but
the runtime synthesizes one per agent so the type system can track
which agents have time access. Code that calls `std.time` outside an
agent context (e.g. `main`) gets a `Clock::default()`.

## Monotonicity

`Instant` wraps `std::time::Instant`, which is monotonic on every
supported platform. `Instant.elapsed_since(other)` returns
`Duration::ZERO` when `self < other` (matches
`std::time::Instant::checked_duration_since`).

## Sync vs async sleep

- `sleep` is async (`tokio::time::sleep`) — use inside agent handlers
  and other tokio contexts.
- `sleep_blocking` is the synchronous fallback (`std::thread::sleep`)
  — used by the MtyIR interpreter when no tokio runtime is on the
  stack (e.g. for tests).

## Example

```rust
use sdust_stdlib::time::{now, sleep_blocking, Clock};
use std::time::Duration;

let start = now(Clock);
sleep_blocking(Clock, Duration::from_millis(100));
let elapsed = now(Clock).elapsed_since(start);
assert!(elapsed >= Duration::from_millis(100));
```
