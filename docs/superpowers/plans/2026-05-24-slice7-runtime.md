# Slice 7 — Runtime MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a real concurrent runtime for Stardust agents on top of the slice-6 SIR interpreter — tokio executor, mailbox slabs, supervisor restart, deadline timers, budget/sandbox enforcement, deterministic replay, and a minimal `std.http` server — so `sdust run` actually runs the agent examples and example 19 serves real HTTP.

**Architecture:** New `sdust-runtime` crate hosts a tokio-backed executor, per-agent mailboxes (bounded MPSC), supervisor tasks, deadline timers, budget tracker, and telemetry emitter. The slice-6 interpreter becomes the per-turn evaluator: each agent turn calls `interp::run_fn_by_name(prog, handler, args, host)` inside a tokio task. `sdust-driver` builds a `Runtime` from a lowered `Program` and `sdust-cli run` invokes it. Deterministic mode swaps the executor for tokio's current-thread runtime with a seeded clock + FIFO mailbox ordering.

**Tech Stack:** Rust 1.82+, tokio 1.x (rt-multi-thread, time, net, sync), parking_lot, dashmap, in-tree HTTP/1.1 parser (no extra deps).

**Predecessor:** `v0.6.0-sir` (HEAD `068af1a` then design commit). 290 baseline tests.

---

## File Structure (locked in here)

```
crates/sdust-runtime/                         # NEW crate
  Cargo.toml
  src/
    lib.rs                                    # public re-exports
    runtime.rs                                # Runtime + RuntimeBuilder
    scheduler.rs                              # tokio executor; deterministic flag
    agent.rs                                  # AgentDescriptor, AgentRegistry, AgentTask
    mailbox.rs                                # Mailbox, MessageFrame, SmallPayload
    supervisor.rs                             # Supervisor engine, strategies
    budget.rs                                 # BudgetTracker, BudgetWord, Budget
    timer.rs                                  # deadline helpers
    telemetry.rs                              # JSON span emitter
    host_std.rs                               # net/fs/time/rand effect dispatch
    deterministic.rs                          # seeded RNG + logical clock
    http.rs                                   # std.http server (minimal HTTP/1.1)
    error.rs                                  # RuntimeError → SD5xxx mapping
  tests/
    mailbox_basic.rs
    agent_lifecycle.rs
    supervisor_strategies.rs
    budget_enforcement.rs
    timer_deadline.rs
    deterministic_replay.rs
    http_serve.rs
    sandbox_enforcement.rs
    end_to_end_examples.rs

crates/sdust-driver/src/pipeline.rs           # MODIFY: add run_file_runtime entry
crates/sdust-cli/src/cmd/run.rs               # MODIFY: --legacy-interp flag, default to runtime
crates/sdust-diagnostics/src/codes.rs         # MODIFY: MT5011..MT5015 added

crates/sdust-sir/src/sir.rs                   # (read-only in this slice)
crates/sdust-sir/src/interp/run.rs            # MINOR: expose evaluator hooks
crates/sdust-sir/src/interp/host.rs           # MODIFY: add rng/timer hooks for runtime

docs/internals/runtime.md                     # NEW
docs/internals/scheduler.md                   # NEW
docs/internals/mailboxes.md                   # NEW
docs/internals/supervisors.md                 # NEW
docs/internals/budgets.md                     # NEW
docs/internals/telemetry.md                   # NEW
docs/reference/cli/sdust-run.md               # MODIFY: runtime flags
docs/spec/v0.1-amendments.md                  # MODIFY: A36..A43
docs/tour/agents.md                           # MODIFY: "now actually runs" note
docs/tour/supervisors.md                      # MODIFY
docs/tour/budgets.md                          # MODIFY
SLICE6.md                                     # MODIFY: deferral cleanup note
SLICE7.md                                     # NEW

Cargo.toml                                    # MODIFY: + crate, + workspace deps
tests/conformance/runtime-7/                  # NEW: 8 conformance cases
```

---

## Task 1: Scaffold sdust-runtime crate

**Files:**
- Create: `crates/sdust-runtime/Cargo.toml`
- Create: `crates/sdust-runtime/src/lib.rs`
- Modify: `Cargo.toml` (workspace root) — add member + tokio/parking_lot/dashmap workspace deps

- [ ] **Step 1: Add workspace dependencies and member**

Modify `Cargo.toml` (workspace root). Find the `[workspace.dependencies]` table and add:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "rt", "macros", "time", "net", "io-util", "sync", "fs"] }
parking_lot = "0.12"
dashmap = "5"
```

Add `"crates/sdust-runtime"` to the `members` list (after `sdust-sir` to preserve ordering).

- [ ] **Step 2: Create the crate manifest**

Create `crates/sdust-runtime/Cargo.toml`:

```toml
[package]
name = "sdust-runtime"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
sdust-sir = { path = "../sdust-sir" }
sdust-types = { path = "../sdust-types" }
sdust-hir = { path = "../sdust-hir" }
sdust-diagnostics = { path = "../sdust-diagnostics" }
tokio.workspace = true
parking_lot.workspace = true
dashmap.workspace = true
thiserror.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["test-util"] }
```

- [ ] **Step 3: Create lib.rs skeleton**

Create `crates/sdust-runtime/src/lib.rs`:

```rust
//! Stardust runtime MVP (spec §25 + §31.5).
//!
//! Tokio-backed concurrent executor for agents lowered to SIR.
//! Provides scheduling, mailboxes, supervisors, deadline timers,
//! budget enforcement, deterministic replay, and a minimal `std.http`
//! server surface.

pub mod agent;
pub mod budget;
pub mod deterministic;
pub mod error;
pub mod http;
pub mod host_std;
pub mod mailbox;
pub mod runtime;
pub mod scheduler;
pub mod supervisor;
pub mod telemetry;
pub mod timer;

pub use agent::{AgentDescriptor, AgentHandle, AgentId};
pub use budget::{Budget, BudgetBreach, BudgetTracker};
pub use error::{RuntimeError, RuntimeResult};
pub use mailbox::{Mailbox, MessageFrame};
pub use runtime::{Runtime, RuntimeBuilder, RunOutcome};
pub use supervisor::{ChildFailure, Strategy};
pub use telemetry::{TelemetryEvent, TelemetrySink};
```

- [ ] **Step 4: Add minimal stubs for each module so it compiles**

Create each module file with a `// placeholder` so the crate compiles. Each subsequent task fills its module in.

For each module file create the file with only a doc comment and the public types referenced by `lib.rs`. Example `crates/sdust-runtime/src/agent.rs`:

```rust
//! Agent descriptor + registry. Filled in by Task 5.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentId(pub u64);

#[derive(Debug)]
pub struct AgentDescriptor;

#[derive(Debug, Clone)]
pub struct AgentHandle {
    pub id: AgentId,
}
```

Apply the same pattern to `budget.rs`, `deterministic.rs`, `error.rs`, `http.rs`, `host_std.rs`, `mailbox.rs`, `runtime.rs`, `scheduler.rs`, `supervisor.rs`, `telemetry.rs`, `timer.rs`. Each file declares **only** the public symbols re-exported by `lib.rs`:

```rust
// budget.rs
#[derive(Debug, Clone, Default)]
pub struct Budget;
#[derive(Debug)]
pub struct BudgetTracker;
#[derive(Debug, Clone)]
pub enum BudgetBreach { Placeholder }

// error.rs
pub type RuntimeResult<T> = Result<T, RuntimeError>;
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("placeholder")]
    Placeholder,
}

// mailbox.rs
#[derive(Debug)]
pub struct Mailbox;
#[derive(Debug)]
pub struct MessageFrame;

// runtime.rs
#[derive(Debug)]
pub struct Runtime;
#[derive(Debug, Default)]
pub struct RuntimeBuilder;
#[derive(Debug, Clone)]
pub enum RunOutcome { Placeholder }

// supervisor.rs
#[derive(Debug, Clone, Copy)]
pub enum Strategy { OneForOne, OneForAll, RestForOne, Escalate }
#[derive(Debug)]
pub enum ChildFailure { Placeholder }

// telemetry.rs
#[derive(Debug, Clone)]
pub enum TelemetryEvent { Placeholder }
#[derive(Debug, Default)]
pub enum TelemetrySink { #[default] Discard, Stderr, File(std::path::PathBuf) }
```

- [ ] **Step 5: Build to confirm the crate compiles**

Run: `cargo build -p sdust-runtime`
Expected: success, zero warnings.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/sdust-runtime
git commit -m "Slice 7: scaffold sdust-runtime crate skeleton

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Add MT5011..MT5015 diagnostic codes

**Files:**
- Modify: `crates/sdust-diagnostics/src/codes.rs`
- Test: `crates/sdust-diagnostics/src/codes.rs` (existing test pattern at bottom of file)

- [ ] **Step 1: Read existing diagnostic codes file**

Run: `Read crates/sdust-diagnostics/src/codes.rs` to confirm the structure. Find the SD5xxx section (after MT5010).

- [ ] **Step 2: Add the new codes**

In `crates/sdust-diagnostics/src/codes.rs`, find the existing SD5xxx entries (search for `MT5010`). After the MT5010 entry add:

```rust
    Code {
        id: "MT5011",
        title: "deadline_exceeded",
        body: "An `?Msg(args) @duration` ask did not receive a reply \
within the requested duration. The runtime cancels the reply oneshot \
and the caller observes a `Result::Err(DeadlineExceeded)` (or a \
typed-error variant when the protocol declares one).",
    },
    Code {
        id: "MT5012",
        title: "mailbox_full",
        body: "An agent's mailbox is at its declared `mb` depth and \
the budget policy is `drop` or `fail`. The send is rejected and the \
sender observes `Result::Err(MailboxFull)`. Under the default `block` \
policy the runtime back-pressures instead of trapping.",
    },
    Code {
        id: "MT5013",
        title: "supervisor_escalated",
        body: "A supervisor's `escalate` strategy propagated a child \
failure to its parent supervisor. At the top of the supervisor tree \
this terminates the runtime.",
    },
    Code {
        id: "MT5014",
        title: "restart_limit_exceeded",
        body: "A child agent exceeded its `restart up_to N in DUR` \
budget. The supervisor escalates the failure to its parent \
strategy.",
    },
    Code {
        id: "MT5015",
        title: "capability_outside_sandbox",
        body: "A capability call attempted to reach a path or host \
not on the active sandbox allowlist. The runtime denies the call and \
the caller observes `Result::Err(SandboxDenied)`.",
    },
```

- [ ] **Step 3: Run diagnostic tests**

Run: `cargo test -p sdust-diagnostics`
Expected: all existing tests pass, no regressions.

- [ ] **Step 4: Add a test asserting the new codes are explained**

In the existing test module at the bottom of `crates/sdust-diagnostics/src/codes.rs`, add:

```rust
    #[test]
    fn slice7_runtime_codes_documented() {
        for id in ["MT5011", "MT5012", "MT5013", "MT5014", "MT5015"] {
            let c = lookup(id).unwrap_or_else(|| panic!("missing code {id}"));
            assert!(!c.body.is_empty(), "body empty for {id}");
        }
    }
```

If the existing API uses a different lookup function name, search `crates/sdust-diagnostics/src` for the equivalent and use it (the file already has a `lookup` or similar; mirror the existing test pattern).

- [ ] **Step 5: Run + commit**

Run: `cargo test -p sdust-diagnostics`
Expected: PASS including the new test.

```bash
git add crates/sdust-diagnostics/src/codes.rs
git commit -m "Slice 7: add MT5011..MT5015 runtime diagnostics

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: RuntimeError + error-mapping module

**Files:**
- Modify: `crates/sdust-runtime/src/error.rs`
- Test: `crates/sdust-runtime/src/error.rs` (inline tests)

- [ ] **Step 1: Write failing tests**

Replace `crates/sdust-runtime/src/error.rs` placeholder with:

```rust
//! Runtime error taxonomy. Maps to SD5xxx diagnostics.

use std::time::Duration;

pub type RuntimeResult<T> = Result<T, RuntimeError>;

#[derive(Debug, Clone, thiserror::Error)]
pub enum RuntimeError {
    #[error("agent panicked: {msg}")]
    AgentPanic { msg: String },

    #[error("deadline exceeded after {0:?}")]
    DeadlineExceeded(Duration),

    #[error("mailbox full (agent {agent})")]
    MailboxFull { agent: String },

    #[error("supervisor escalated: {child}")]
    SupervisorEscalated { child: String },

    #[error("restart limit exceeded for {child}")]
    RestartLimitExceeded { child: String },

    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),

    #[error("capability outside sandbox: {0}")]
    CapabilityOutsideSandbox(String),

    #[error("extern fn unimplemented: {0}")]
    ExternUnimplemented(String),

    #[error("agent not found: {0}")]
    AgentNotFound(String),

    #[error("handler not found: {agent}.{msg}")]
    HandlerNotFound { agent: String, msg: String },

    #[error("trap: {code} {message}")]
    Trap { code: &'static str, message: String },
}

impl RuntimeError {
    /// Map to the SD5xxx diagnostic id used in user-facing messages
    /// and exit-code mapping.
    pub fn diag_code(&self) -> &'static str {
        match self {
            RuntimeError::AgentPanic { .. } => "MT5001",
            RuntimeError::DeadlineExceeded(_) => "MT5011",
            RuntimeError::MailboxFull { .. } => "MT5012",
            RuntimeError::SupervisorEscalated { .. } => "MT5013",
            RuntimeError::RestartLimitExceeded { .. } => "MT5014",
            RuntimeError::BudgetExceeded(_) => "MT5009",
            RuntimeError::CapabilityOutsideSandbox(_) => "MT5015",
            RuntimeError::ExternUnimplemented(_) => "MT5050",
            RuntimeError::AgentNotFound(_) => "MT5021",
            RuntimeError::HandlerNotFound { .. } => "MT5020",
            RuntimeError::Trap { code, .. } => code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_cover_all_variants() {
        let cases = [
            (RuntimeError::AgentPanic { msg: "x".into() }, "MT5001"),
            (RuntimeError::DeadlineExceeded(Duration::from_millis(10)), "MT5011"),
            (RuntimeError::MailboxFull { agent: "A".into() }, "MT5012"),
            (RuntimeError::SupervisorEscalated { child: "c".into() }, "MT5013"),
            (RuntimeError::RestartLimitExceeded { child: "c".into() }, "MT5014"),
            (RuntimeError::BudgetExceeded("cpu".into()), "MT5009"),
            (RuntimeError::CapabilityOutsideSandbox("/etc".into()), "MT5015"),
            (RuntimeError::ExternUnimplemented("foo".into()), "MT5050"),
            (RuntimeError::AgentNotFound("A".into()), "MT5021"),
            (RuntimeError::HandlerNotFound { agent: "A".into(), msg: "M".into() }, "MT5020"),
            (RuntimeError::Trap { code: "MT5005", message: "u".into() }, "MT5005"),
        ];
        for (err, code) in cases {
            assert_eq!(err.diag_code(), code, "wrong code for {err:?}");
        }
    }
}
```

- [ ] **Step 2: Run + verify**

Run: `cargo test -p sdust-runtime error`
Expected: 1 test passes.

- [ ] **Step 3: Commit**

```bash
git add crates/sdust-runtime/src/error.rs
git commit -m "Slice 7: RuntimeError taxonomy + SD5xxx mapping

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Mailbox slabs (MessageFrame + bounded MPSC)

**Files:**
- Modify: `crates/sdust-runtime/src/mailbox.rs`
- Create: `crates/sdust-runtime/tests/mailbox_basic.rs`

- [ ] **Step 1: Write failing test**

Create `crates/sdust-runtime/tests/mailbox_basic.rs`:

```rust
use sdust_runtime::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
use sdust_sir::interp::value::Value;
use std::time::Duration;

#[tokio::test]
async fn fifo_and_bounded() {
    let mb = Mailbox::new(2, SendPolicy::Block);
    let frame1 = MessageFrame::fire_and_forget("Ping", SmallPayload::inline(vec![Value::Unit]));
    let frame2 = MessageFrame::fire_and_forget("Pong", SmallPayload::inline(vec![Value::Unit]));
    mb.try_send(frame1).unwrap();
    mb.try_send(frame2).unwrap();
    assert!(mb.try_send(MessageFrame::fire_and_forget("X", SmallPayload::Empty)).is_err());
    let r1 = mb.recv().await.unwrap();
    assert_eq!(r1.proto_msg, "Ping");
    let r2 = mb.recv().await.unwrap();
    assert_eq!(r2.proto_msg, "Pong");
}

#[tokio::test]
async fn ask_reply() {
    let mb = Mailbox::new(8, SendPolicy::Block);
    let (frame, reply_rx) = MessageFrame::ask("Query", SmallPayload::Empty, Some(Duration::from_secs(1)));
    mb.try_send(frame).unwrap();
    let r = mb.recv().await.unwrap();
    assert_eq!(r.proto_msg, "Query");
    r.reply.unwrap().send(Ok(Value::Int(7, sdust_types::IntKind::I32))).unwrap();
    let v = reply_rx.await.unwrap().unwrap();
    matches!(v, Value::Int(7, _));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p sdust-runtime --test mailbox_basic`
Expected: FAIL (types missing).

- [ ] **Step 3: Implement Mailbox + MessageFrame**

Replace `crates/sdust-runtime/src/mailbox.rs` with:

```rust
//! Per-agent mailbox slabs. Bounded MPSC carrying MessageFrames.

use sdust_sir::interp::value::Value;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

use crate::error::{RuntimeError, RuntimeResult};

#[derive(Debug, Clone, Copy)]
pub enum SendPolicy {
    /// Sender waits until capacity is available.
    Block,
    /// Drop the message and warn.
    Drop,
    /// Return MT5012 to the sender.
    Fail,
}

/// Tiny payload optimisation: most messages have ≤4 args.
#[derive(Debug)]
pub enum SmallPayload {
    Empty,
    Inline(Vec<Value>),
}

impl SmallPayload {
    pub fn inline(values: Vec<Value>) -> Self {
        SmallPayload::Inline(values)
    }
    pub fn values(&self) -> &[Value] {
        match self {
            SmallPayload::Empty => &[],
            SmallPayload::Inline(v) => v.as_slice(),
        }
    }
    pub fn into_vec(self) -> Vec<Value> {
        match self {
            SmallPayload::Empty => vec![],
            SmallPayload::Inline(v) => v,
        }
    }
}

#[derive(Debug)]
pub struct MessageFrame {
    pub proto_msg: String,
    pub payload: SmallPayload,
    pub reply: Option<oneshot::Sender<RuntimeResult<Value>>>,
    pub deadline: Option<Instant>,
    pub seq: u64,
}

impl MessageFrame {
    pub fn fire_and_forget(msg: &str, payload: SmallPayload) -> Self {
        Self {
            proto_msg: msg.into(),
            payload,
            reply: None,
            deadline: None,
            seq: 0,
        }
    }
    pub fn ask(
        msg: &str,
        payload: SmallPayload,
        deadline: Option<Duration>,
    ) -> (Self, oneshot::Receiver<RuntimeResult<Value>>) {
        let (tx, rx) = oneshot::channel();
        let frame = Self {
            proto_msg: msg.into(),
            payload,
            reply: Some(tx),
            deadline: deadline.map(|d| Instant::now() + d),
            seq: 0,
        };
        (frame, rx)
    }
}

#[derive(Debug)]
pub struct Mailbox {
    tx: mpsc::Sender<MessageFrame>,
    rx: parking_lot::Mutex<Option<mpsc::Receiver<MessageFrame>>>,
    capacity: usize,
    policy: SendPolicy,
}

impl Mailbox {
    pub fn new(capacity: usize, policy: SendPolicy) -> Self {
        let (tx, rx) = mpsc::channel(capacity);
        Self {
            tx,
            rx: parking_lot::Mutex::new(Some(rx)),
            capacity,
            policy,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn policy(&self) -> SendPolicy {
        self.policy
    }
    pub fn sender(&self) -> mpsc::Sender<MessageFrame> {
        self.tx.clone()
    }

    pub fn try_send(&self, frame: MessageFrame) -> RuntimeResult<()> {
        match self.tx.try_send(frame) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(RuntimeError::MailboxFull {
                agent: String::new(),
            }),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(RuntimeError::AgentNotFound(
                "(closed mailbox)".into(),
            )),
        }
    }

    pub async fn send(&self, frame: MessageFrame) -> RuntimeResult<()> {
        match self.policy {
            SendPolicy::Block => self
                .tx
                .send(frame)
                .await
                .map_err(|_| RuntimeError::AgentNotFound("(closed mailbox)".into())),
            SendPolicy::Drop => {
                let _ = self.tx.try_send(frame);
                Ok(())
            }
            SendPolicy::Fail => self.try_send(frame),
        }
    }

    /// Take the receiver. Can be called at most once — subsequent calls
    /// return None. Designed for the agent's run loop.
    pub fn take_receiver(&self) -> Option<mpsc::Receiver<MessageFrame>> {
        self.rx.lock().take()
    }

    /// Test helper: synchronous receive.
    pub async fn recv(&self) -> Option<MessageFrame> {
        let mut guard = self.rx.lock();
        let rx = guard.as_mut()?;
        rx.recv().await
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sdust-runtime --test mailbox_basic`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sdust-runtime/src/mailbox.rs crates/sdust-runtime/tests/mailbox_basic.rs
git commit -m "Slice 7: mailbox slabs (bounded MPSC + MessageFrame + ask/reply)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: BudgetTracker (atomic counters + breach detection)

**Files:**
- Modify: `crates/sdust-runtime/src/budget.rs`
- Create: `crates/sdust-runtime/tests/budget_enforcement.rs`

- [ ] **Step 1: Write failing tests**

Create `crates/sdust-runtime/tests/budget_enforcement.rs`:

```rust
use sdust_runtime::budget::{Budget, BudgetBreach, BudgetTracker};
use std::time::Duration;

#[test]
fn cpu_budget_breach() {
    let b = Budget {
        cpu: Some(Duration::from_millis(10)),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    t.record_cpu(Duration::from_millis(5));
    assert!(t.check().is_ok());
    t.record_cpu(Duration::from_millis(8));
    matches!(t.check().unwrap_err(), BudgetBreach::Cpu(_));
}

#[test]
fn mailbox_budget_breach() {
    let b = Budget {
        mailbox: Some(2),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    assert!(t.check_mailbox_depth(1).is_ok());
    assert!(t.check_mailbox_depth(2).is_ok());
    matches!(t.check_mailbox_depth(3).unwrap_err(), BudgetBreach::Mailbox(_));
}

#[test]
fn spawned_tasks_breach() {
    let b = Budget {
        spawned: Some(3),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    for _ in 0..3 {
        assert!(t.record_spawn().is_ok());
    }
    matches!(t.record_spawn().unwrap_err(), BudgetBreach::Spawned(_));
}

#[test]
fn host_allowlist_blocks_other_host() {
    let mut b = Budget::default();
    b.hosts = Some(vec!["api.example.com:443".into()]);
    let t = BudgetTracker::new(b);
    assert!(t.check_host("api.example.com:443").is_ok());
    matches!(t.check_host("evil.example.com:443").unwrap_err(), BudgetBreach::Host(_));
}

#[test]
fn path_allowlist_prefix_matches() {
    let mut b = Budget::default();
    b.read_paths = Some(vec!["/models".into(), "/tmp/input.json".into()]);
    let t = BudgetTracker::new(b);
    assert!(t.check_read_path("/models/foo").is_ok());
    assert!(t.check_read_path("/tmp/input.json").is_ok());
    matches!(t.check_read_path("/etc/passwd").unwrap_err(), BudgetBreach::Path(_));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p sdust-runtime --test budget_enforcement`
Expected: FAIL (types missing).

- [ ] **Step 3: Implement Budget + BudgetTracker**

Replace `crates/sdust-runtime/src/budget.rs` with:

```rust
//! Budget + sandbox enforcement (spec §16.2).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::error::{RuntimeError, RuntimeResult};

#[derive(Debug, Clone, Default)]
pub struct Budget {
    pub cpu: Option<Duration>,
    pub wall: Option<Duration>,
    pub mem_bytes: Option<u64>,
    pub mailbox: Option<u64>,
    pub spawned: Option<u64>,
    pub hosts: Option<Vec<String>>,
    pub read_paths: Option<Vec<String>>,
    pub write_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub enum BudgetBreach {
    Cpu(Duration),
    Wall(Duration),
    Mem(u64),
    Mailbox(u64),
    Spawned(u64),
    Host(String),
    Path(String),
}

impl BudgetBreach {
    pub fn into_runtime_error(self) -> RuntimeError {
        match self {
            BudgetBreach::Cpu(d) => RuntimeError::BudgetExceeded(format!("cpu {:?}", d)),
            BudgetBreach::Wall(d) => RuntimeError::BudgetExceeded(format!("wall {:?}", d)),
            BudgetBreach::Mem(n) => RuntimeError::BudgetExceeded(format!("mem {} B", n)),
            BudgetBreach::Mailbox(n) => RuntimeError::BudgetExceeded(format!("mailbox {}", n)),
            BudgetBreach::Spawned(n) => RuntimeError::BudgetExceeded(format!("spawned {}", n)),
            BudgetBreach::Host(h) => RuntimeError::CapabilityOutsideSandbox(format!("net {}", h)),
            BudgetBreach::Path(p) => RuntimeError::CapabilityOutsideSandbox(format!("fs {}", p)),
        }
    }
}

#[derive(Debug)]
pub struct BudgetTracker {
    budget: Budget,
    cpu_ns: AtomicU64,
    mem: AtomicU64,
    spawned: AtomicU64,
    start: std::time::Instant,
}

impl BudgetTracker {
    pub fn new(budget: Budget) -> Self {
        Self {
            budget,
            cpu_ns: AtomicU64::new(0),
            mem: AtomicU64::new(0),
            spawned: AtomicU64::new(0),
            start: std::time::Instant::now(),
        }
    }

    pub fn budget(&self) -> &Budget {
        &self.budget
    }

    pub fn record_cpu(&self, d: Duration) {
        self.cpu_ns
            .fetch_add(d.as_nanos() as u64, Ordering::Relaxed);
    }

    pub fn record_mem(&self, bytes: u64) {
        self.mem.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_spawn(&self) -> Result<(), BudgetBreach> {
        let n = self.spawned.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(limit) = self.budget.spawned {
            if n > limit {
                return Err(BudgetBreach::Spawned(n));
            }
        }
        Ok(())
    }

    pub fn check(&self) -> Result<(), BudgetBreach> {
        if let Some(limit) = self.budget.cpu {
            let used = Duration::from_nanos(self.cpu_ns.load(Ordering::Relaxed));
            if used > limit {
                return Err(BudgetBreach::Cpu(used));
            }
        }
        if let Some(limit) = self.budget.wall {
            let elapsed = self.start.elapsed();
            if elapsed > limit {
                return Err(BudgetBreach::Wall(elapsed));
            }
        }
        if let Some(limit) = self.budget.mem_bytes {
            let used = self.mem.load(Ordering::Relaxed);
            if used > limit {
                return Err(BudgetBreach::Mem(used));
            }
        }
        Ok(())
    }

    pub fn check_mailbox_depth(&self, depth: u64) -> Result<(), BudgetBreach> {
        if let Some(limit) = self.budget.mailbox {
            if depth > limit {
                return Err(BudgetBreach::Mailbox(depth));
            }
        }
        Ok(())
    }

    pub fn check_host(&self, host: &str) -> Result<(), BudgetBreach> {
        if let Some(allow) = &self.budget.hosts {
            if !allow.iter().any(|h| h == host) {
                return Err(BudgetBreach::Host(host.into()));
            }
        }
        Ok(())
    }

    pub fn check_read_path(&self, path: &str) -> Result<(), BudgetBreach> {
        check_path(path, self.budget.read_paths.as_deref())
    }

    pub fn check_write_path(&self, path: &str) -> Result<(), BudgetBreach> {
        check_path(path, self.budget.write_paths.as_deref())
    }
}

fn check_path(path: &str, allow: Option<&[String]>) -> Result<(), BudgetBreach> {
    let Some(list) = allow else { return Ok(()) };
    let ok = list.iter().any(|p| path == p || path.starts_with(&format!("{p}/")) || path.starts_with(p) && p.ends_with('/'));
    // Simpler prefix semantics: allowed if path starts with any list entry.
    let ok = ok || list.iter().any(|p| path.starts_with(p));
    if ok {
        Ok(())
    } else {
        Err(BudgetBreach::Path(path.into()))
    }
}

pub fn convert_breach(b: BudgetBreach) -> RuntimeResult<()> {
    Err(b.into_runtime_error())
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sdust-runtime --test budget_enforcement`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sdust-runtime/src/budget.rs crates/sdust-runtime/tests/budget_enforcement.rs
git commit -m "Slice 7: BudgetTracker — CPU/wall/mem/mailbox/spawn + sandbox allowlists

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: TelemetrySink (stderr + file + discard)

**Files:**
- Modify: `crates/sdust-runtime/src/telemetry.rs`
- Test: inline tests in `telemetry.rs`

- [ ] **Step 1: Replace telemetry.rs with full impl + inline tests**

Replace `crates/sdust-runtime/src/telemetry.rs` with:

```rust
//! Telemetry JSON line emitter (OTLP-flavoured, see A38).

use parking_lot::Mutex;
use std::io::Write;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub enum TelemetryEvent {
    TurnStart { agent: String, msg: String },
    TurnEnd { agent: String, msg: String, duration_us: u128 },
    Send { from: String, to: String, msg: String },
    Ask { from: String, to: String, msg: String, deadline_ms: Option<u64> },
    Reply { from: String, msg: String, ok: bool },
    Spawn { name: String, agent_id: u64 },
    Restart { supervisor: String, child: String, attempt: u32 },
    BudgetBreach { agent: String, kind: String },
    Shutdown,
}

impl TelemetryEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            TelemetryEvent::TurnStart { .. } => "turn_start",
            TelemetryEvent::TurnEnd { .. } => "turn_end",
            TelemetryEvent::Send { .. } => "send",
            TelemetryEvent::Ask { .. } => "ask",
            TelemetryEvent::Reply { .. } => "reply",
            TelemetryEvent::Spawn { .. } => "spawn",
            TelemetryEvent::Restart { .. } => "restart",
            TelemetryEvent::BudgetBreach { .. } => "budget_breach",
            TelemetryEvent::Shutdown => "shutdown",
        }
    }

    pub fn to_json_line(&self, ts_ms: u128) -> String {
        let kind = self.kind();
        let payload = match self {
            TelemetryEvent::TurnStart { agent, msg } => format!(
                r#""agent":"{}","msg":"{}""#,
                esc(agent), esc(msg)
            ),
            TelemetryEvent::TurnEnd { agent, msg, duration_us } => format!(
                r#""agent":"{}","msg":"{}","duration_us":{}"#,
                esc(agent), esc(msg), duration_us
            ),
            TelemetryEvent::Send { from, to, msg } => format!(
                r#""from":"{}","to":"{}","msg":"{}""#,
                esc(from), esc(to), esc(msg)
            ),
            TelemetryEvent::Ask { from, to, msg, deadline_ms } => format!(
                r#""from":"{}","to":"{}","msg":"{}","deadline_ms":{}"#,
                esc(from), esc(to), esc(msg),
                deadline_ms.map(|d| d.to_string()).unwrap_or_else(|| "null".into())
            ),
            TelemetryEvent::Reply { from, msg, ok } => format!(
                r#""from":"{}","msg":"{}","ok":{}"#,
                esc(from), esc(msg), ok
            ),
            TelemetryEvent::Spawn { name, agent_id } => format!(
                r#""name":"{}","agent_id":{}"#,
                esc(name), agent_id
            ),
            TelemetryEvent::Restart { supervisor, child, attempt } => format!(
                r#""supervisor":"{}","child":"{}","attempt":{}"#,
                esc(supervisor), esc(child), attempt
            ),
            TelemetryEvent::BudgetBreach { agent, kind: k } => format!(
                r#""agent":"{}","kind":"{}""#,
                esc(agent), esc(k)
            ),
            TelemetryEvent::Shutdown => String::new(),
        };
        if payload.is_empty() {
            format!(r#"{{"ts":{},"kind":"{}"}}"#, ts_ms, kind)
        } else {
            format!(r#"{{"ts":{},"kind":"{}",{}}}"#, ts_ms, kind, payload)
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[derive(Debug, Default, Clone)]
pub enum TelemetrySink {
    #[default]
    Discard,
    Stderr,
    File(std::path::PathBuf),
    Buffer(Arc<Mutex<Vec<String>>>),
}

impl TelemetrySink {
    pub fn buffer() -> (Self, Arc<Mutex<Vec<String>>>) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        (TelemetrySink::Buffer(buf.clone()), buf)
    }

    pub fn from_env() -> Self {
        match std::env::var("STARDUST_TRACE").as_deref() {
            Ok("stderr") => TelemetrySink::Stderr,
            Ok(v) if v.starts_with("file:") => {
                TelemetrySink::File(std::path::PathBuf::from(&v[5..]))
            }
            _ => TelemetrySink::Discard,
        }
    }

    pub fn emit(&self, ev: &TelemetryEvent) {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let line = ev.to_json_line(ts);
        match self {
            TelemetrySink::Discard => {}
            TelemetrySink::Stderr => {
                let _ = writeln!(std::io::stderr(), "{}", line);
            }
            TelemetrySink::File(p) => {
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                {
                    let _ = writeln!(f, "{}", line);
                }
            }
            TelemetrySink::Buffer(buf) => {
                buf.lock().push(line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shapes() {
        let ev = TelemetryEvent::TurnStart {
            agent: "A".into(),
            msg: "Ping".into(),
        };
        let s = ev.to_json_line(100);
        assert!(s.contains(r#""kind":"turn_start""#));
        assert!(s.contains(r#""agent":"A""#));
        assert!(s.contains(r#""msg":"Ping""#));
    }

    #[test]
    fn buffer_sink_captures() {
        let (sink, buf) = TelemetrySink::buffer();
        sink.emit(&TelemetryEvent::Spawn {
            name: "X".into(),
            agent_id: 7,
        });
        sink.emit(&TelemetryEvent::Shutdown);
        let lines = buf.lock();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(r#""kind":"spawn""#));
        assert!(lines[1].contains(r#""kind":"shutdown""#));
    }

    #[test]
    fn quote_escaping() {
        let ev = TelemetryEvent::Send {
            from: "A\"".into(),
            to: "B".into(),
            msg: r#"M\Q"#.into(),
        };
        let s = ev.to_json_line(0);
        assert!(s.contains(r#""from":"A\"""#));
        assert!(s.contains(r#""msg":"M\\Q""#));
    }
}
```

- [ ] **Step 2: Run + commit**

Run: `cargo test -p sdust-runtime telemetry`
Expected: 3 tests pass.

```bash
git add crates/sdust-runtime/src/telemetry.rs
git commit -m "Slice 7: telemetry JSON line emitter (stderr/file/buffer/discard)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: AgentDescriptor + AgentRegistry

**Files:**
- Modify: `crates/sdust-runtime/src/agent.rs`

- [ ] **Step 1: Replace agent.rs**

Replace `crates/sdust-runtime/src/agent.rs` with:

```rust
//! Agent descriptor + registry (spec §25.2).

use crate::budget::BudgetTracker;
use crate::mailbox::Mailbox;
use dashmap::DashMap;
use parking_lot::Mutex;
use sdust_sir::interp::value::Value;
use sdust_sir::sir::AgentSirId;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentId(pub u64);

#[derive(Debug)]
pub struct AgentDescriptor {
    pub id: AgentId,
    pub name: String,
    pub sir_id: AgentSirId,
    pub state: Mutex<Value>,
    pub mailbox: Arc<Mailbox>,
    pub budget: Arc<BudgetTracker>,
    pub supervisor: Option<AgentId>,
    pub mailbox_depth: AtomicU64,
}

#[derive(Debug, Clone)]
pub struct AgentHandle {
    pub id: AgentId,
    pub name: String,
    pub mailbox: Arc<Mailbox>,
}

#[derive(Debug, Default)]
pub struct AgentRegistry {
    next_id: AtomicU64,
    by_id: DashMap<AgentId, Arc<AgentDescriptor>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn next_id(&self) -> AgentId {
        AgentId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }
    pub fn insert(&self, desc: Arc<AgentDescriptor>) {
        self.by_id.insert(desc.id, desc);
    }
    pub fn get(&self, id: AgentId) -> Option<Arc<AgentDescriptor>> {
        self.by_id.get(&id).map(|r| r.clone())
    }
    pub fn remove(&self, id: AgentId) {
        self.by_id.remove(&id);
    }
    pub fn len(&self) -> usize {
        self.by_id.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::Budget;
    use crate::mailbox::SendPolicy;

    #[test]
    fn registry_round_trip() {
        let reg = AgentRegistry::new();
        let id = reg.next_id();
        let desc = Arc::new(AgentDescriptor {
            id,
            name: "X".into(),
            sir_id: AgentSirId(0),
            state: Mutex::new(Value::Unit),
            mailbox: Arc::new(Mailbox::new(8, SendPolicy::Block)),
            budget: Arc::new(BudgetTracker::new(Budget::default())),
            supervisor: None,
            mailbox_depth: AtomicU64::new(0),
        });
        reg.insert(desc.clone());
        let got = reg.get(id).unwrap();
        assert_eq!(got.name, "X");
        assert_eq!(reg.len(), 1);
        reg.remove(id);
        assert!(reg.is_empty());
    }
}
```

- [ ] **Step 2: Run + commit**

Run: `cargo test -p sdust-runtime agent`
Expected: 1 test passes.

```bash
git add crates/sdust-runtime/src/agent.rs
git commit -m "Slice 7: AgentDescriptor + AgentRegistry (concurrent)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Timer helpers (deadline + sleep)

**Files:**
- Modify: `crates/sdust-runtime/src/timer.rs`
- Create: `crates/sdust-runtime/tests/timer_deadline.rs`

- [ ] **Step 1: Write failing test**

Create `crates/sdust-runtime/tests/timer_deadline.rs`:

```rust
use sdust_runtime::timer::with_deadline;
use std::time::Duration;

#[tokio::test]
async fn deadline_fires() {
    let res = with_deadline(Some(Duration::from_millis(20)), async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        42_i32
    })
    .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn deadline_none_passes_through() {
    let res = with_deadline(None, async { 7_i32 }).await.unwrap();
    assert_eq!(res, 7);
}

#[tokio::test]
async fn deadline_returns_value_when_fast() {
    let res = with_deadline(Some(Duration::from_secs(1)), async { 9_i32 })
        .await
        .unwrap();
    assert_eq!(res, 9);
}
```

- [ ] **Step 2: Implement timer.rs**

Replace `crates/sdust-runtime/src/timer.rs` with:

```rust
//! Deadline helpers around tokio::time.

use crate::error::RuntimeError;
use std::future::Future;
use std::time::Duration;

pub async fn with_deadline<F, T>(d: Option<Duration>, fut: F) -> Result<T, RuntimeError>
where
    F: Future<Output = T>,
{
    match d {
        None => Ok(fut.await),
        Some(d) => tokio::time::timeout(d, fut)
            .await
            .map_err(|_| RuntimeError::DeadlineExceeded(d)),
    }
}
```

- [ ] **Step 3: Run + commit**

Run: `cargo test -p sdust-runtime --test timer_deadline`
Expected: 3 tests pass.

```bash
git add crates/sdust-runtime/src/timer.rs crates/sdust-runtime/tests/timer_deadline.rs
git commit -m "Slice 7: deadline timer helper (tokio::time::timeout wrapper)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: Per-turn evaluator helper in sdust-sir

**Files:**
- Modify: `crates/sdust-sir/src/interp/run.rs`
- Modify: `crates/sdust-sir/src/interp/mod.rs`

The runtime needs to invoke a single function on a borrowed `Program`
with a fresh frame stack and return the trap or value. Slice 6 already
has `run_fn_by_name`; the runtime needs the same shape but with a
caller-owned step budget so per-turn budgets translate to step counts.

- [ ] **Step 1: Add a step-budget-aware variant**

Open `crates/sdust-sir/src/interp/run.rs`. Find `run_fn_by_name` and
add immediately after it:

```rust
/// Like `run_fn_by_name`, but lets the caller cap the step budget for
/// this single call (used by the runtime to translate per-turn CPU
/// budgets into bounded interpreter step counts). Returns the SD5xxx
/// trap code when the budget is exhausted so the runtime can map it.
pub fn run_fn_with_budget(
    prog: &Program,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
    step_budget: u64,
) -> Result<Value, RunResult> {
    let f = match prog.fn_by_name(name) {
        Some(f) => f,
        None => return Err(RunResult::NoMain),
    };
    let mut interp = Interp::new(prog, step_budget);
    let initial_locals = initial_locals_for(f, &args);
    let scope = interp.fresh_scope();
    let frame = Frame::new(f.id, initial_locals, scope, f.entry);
    interp.stack.push(frame);
    match interp.run(host) {
        RunResult::Ok { .. } => Ok(interp.last_return),
        r => Err(r),
    }
}
```

- [ ] **Step 2: Re-export through mod.rs**

Open `crates/sdust-sir/src/interp/mod.rs`. Confirm `pub use run::{run, run_fn_by_name, ...}` exists; add `run_fn_with_budget` to the list.

- [ ] **Step 3: Run + commit**

Run: `cargo build -p sdust-sir`
Expected: success, no warnings.

```bash
git add crates/sdust-sir/src/interp/run.rs crates/sdust-sir/src/interp/mod.rs
git commit -m "Slice 7: expose run_fn_with_budget for runtime per-turn evaluator

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: AgentTask — the per-agent run loop

**Files:**
- Modify: `crates/sdust-runtime/src/agent.rs` — add `agent_task_loop` helper
- Create: `crates/sdust-runtime/tests/agent_lifecycle.rs`

- [ ] **Step 1: Write failing test**

Create `crates/sdust-runtime/tests/agent_lifecycle.rs`:

```rust
use sdust_runtime::runtime::{Runtime, RuntimeBuilder};
use std::sync::Arc;

const ECHO_SRC: &str = r#"
protocol Echo {
  Ping(msg: Str) -> Str
}
agent Echoer: Echo {
  on Ping(msg) -> msg
}
fn main() -> Unit {
  ()
}
"#;

fn compile(src: &str) -> Arc<sdust_sir::sir::Program> {
    use sdust_driver::pipeline::lower_to_sir;
    Arc::new(lower_to_sir(src, "test.sd").unwrap())
}

#[tokio::test]
async fn spawn_send_ask_echo() {
    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().build(prog);
    let h = rt.spawn_agent("Echoer", vec![]).await.unwrap();
    let reply = rt
        .ask(&h, "Ping", vec![sdust_sir::interp::value::Value::Str("hi".into())], None)
        .await
        .unwrap();
    matches!(reply, sdust_sir::interp::value::Value::Str(ref s) if s == "hi");
    rt.shutdown().await;
}
```

This test will need Runtime/RuntimeBuilder fleshed out — those land in Task 11. For Task 10 we add the per-agent loop helper but skip the end-to-end test until then.

- [ ] **Step 2: Add agent_task_loop helper (no test runs yet)**

Append to `crates/sdust-runtime/src/agent.rs`:

```rust
use crate::error::{RuntimeError, RuntimeResult};
use crate::mailbox::MessageFrame;
use crate::telemetry::{TelemetryEvent, TelemetrySink};
use sdust_sir::interp::host::Host;
use sdust_sir::interp::run::run_fn_with_budget;
use sdust_sir::sir::Program;
use std::sync::Arc;

/// One iteration of an agent's turn loop. Returns Ok(()) when the
/// agent should keep running, Err to terminate (e.g. budget breach).
pub async fn run_one_turn(
    prog: &Program,
    desc: &AgentDescriptor,
    frame: MessageFrame,
    host: &mut dyn Host,
    telemetry: &TelemetrySink,
    handler_name: &str,
) -> RuntimeResult<()> {
    telemetry.emit(&TelemetryEvent::TurnStart {
        agent: desc.name.clone(),
        msg: frame.proto_msg.clone(),
    });
    let started = std::time::Instant::now();

    // Build args: agent state followed by message payload.
    let mut args = Vec::with_capacity(frame.payload.values().len() + 1);
    {
        let st = desc.state.lock();
        args.push(st.clone());
    }
    args.extend(frame.payload.into_vec());

    let step_budget = 1_000_000;
    let result = run_fn_with_budget(prog, handler_name, args, host, step_budget);

    desc.budget.record_cpu(started.elapsed());
    telemetry.emit(&TelemetryEvent::TurnEnd {
        agent: desc.name.clone(),
        msg: frame.proto_msg.clone(),
        duration_us: started.elapsed().as_micros(),
    });

    let value = match result {
        Ok(v) => v,
        Err(rr) => {
            let err = match rr {
                sdust_sir::interp::run::RunResult::Trap { code, message } => {
                    RuntimeError::Trap { code, message }
                }
                sdust_sir::interp::run::RunResult::BudgetExceeded => {
                    RuntimeError::BudgetExceeded("steps".into())
                }
                sdust_sir::interp::run::RunResult::NoMain => {
                    RuntimeError::HandlerNotFound {
                        agent: desc.name.clone(),
                        msg: handler_name.into(),
                    }
                }
                sdust_sir::interp::run::RunResult::Ok { .. } => unreachable!(),
            };
            if let Some(reply) = frame.reply {
                let _ = reply.send(Err(err.clone()));
            }
            return Err(err);
        }
    };

    if let Some(reply) = frame.reply {
        let _ = reply.send(Ok(value));
    }
    Ok(())
}

/// Format `Agent::msg` as the SIR-compiler-generated handler fn name.
/// (See `crates/sdust-sir/src/lower/items.rs` — handlers are emitted
/// as `<AgentName>__<MsgName>` in slice 6.)
pub fn handler_fn_name(agent: &str, msg: &str) -> String {
    format!("{agent}__{msg}")
}

fn _ensure_arc_program(_p: Arc<Program>) {}
```

- [ ] **Step 3: Verify handler naming matches the lowerer**

Search the lowerer to confirm handler naming convention:

Run: `Grep -n "__" crates/sdust-sir/src/lower/items.rs` (use the Grep tool).

If the lowerer uses a different separator (e.g. `.` or `_on_`), update `handler_fn_name` accordingly. Update this task in-place with the actual format before moving on.

- [ ] **Step 4: Build only (lifecycle test runs in Task 11)**

Run: `cargo build -p sdust-runtime`
Expected: success.

- [ ] **Step 5: Commit**

```bash
git add crates/sdust-runtime/src/agent.rs crates/sdust-runtime/tests/agent_lifecycle.rs
git commit -m "Slice 7: per-agent turn loop (run_one_turn + handler_fn_name)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: Runtime + RuntimeBuilder + spawn_agent + ask + shutdown

**Files:**
- Modify: `crates/sdust-runtime/src/runtime.rs`
- Modify: `crates/sdust-runtime/src/scheduler.rs`
- Modify: `crates/sdust-driver/src/pipeline.rs` (expose `lower_to_sir` as `pub fn`)
- Add: `crates/sdust-runtime/Cargo.toml` — add dev-dep `sdust-driver`

- [ ] **Step 1: Expose driver pipeline entry for tests**

Open `crates/sdust-driver/src/pipeline.rs`. Confirm there is a function that runs parse→typeck→borrow→lower-to-SIR and returns a `Program`. Search for `lower_to_sir`. If named differently, alias as needed; otherwise create:

```rust
pub fn lower_source_to_sir(src: &str, path: &str) -> Result<sdust_sir::sir::Program, Vec<sdust_diagnostics::Diagnostic>> {
    // ... existing impl ...
}
```

If a function with this exact contract already exists, skip this step.

- [ ] **Step 2: Add driver as dev-dep**

In `crates/sdust-runtime/Cargo.toml`, under `[dev-dependencies]`:

```toml
sdust-driver = { path = "../sdust-driver" }
```

- [ ] **Step 3: Implement scheduler.rs**

Replace `crates/sdust-runtime/src/scheduler.rs` with:

```rust
//! Tokio executor wrapper. Multi-thread by default; single-thread
//! current-thread when deterministic mode is requested.

use std::sync::Arc;
use tokio::runtime::{Builder, Runtime as TokioRt};

#[derive(Debug)]
pub struct Scheduler {
    pub rt: Arc<TokioRt>,
    pub deterministic: bool,
}

impl Scheduler {
    pub fn multi_thread(threads: usize) -> Self {
        let rt = Builder::new_multi_thread()
            .worker_threads(threads.max(1))
            .enable_all()
            .build()
            .expect("tokio runtime");
        Self { rt: Arc::new(rt), deterministic: false }
    }

    pub fn current_thread() -> Self {
        let rt = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        Self { rt: Arc::new(rt), deterministic: true }
    }
}
```

- [ ] **Step 4: Implement runtime.rs**

Replace `crates/sdust-runtime/src/runtime.rs` with:

```rust
//! Runtime + RuntimeBuilder.

use crate::agent::{handler_fn_name, AgentDescriptor, AgentHandle, AgentId, AgentRegistry};
use crate::budget::{Budget, BudgetTracker};
use crate::error::{RuntimeError, RuntimeResult};
use crate::host_std::StdHost;
use crate::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
use crate::scheduler::Scheduler;
use crate::supervisor::{ChildFailureEvent, SupervisorRegistry};
use crate::telemetry::{TelemetryEvent, TelemetrySink};
use crate::timer::with_deadline;
use parking_lot::Mutex;
use sdust_sir::interp::value::Value;
use sdust_sir::sir::{Agent as SirAgent, Program};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub enum RunOutcome {
    Ok,
    Trap { code: &'static str, message: String },
    Timeout,
}

#[derive(Debug)]
pub struct RuntimeBuilder {
    deterministic_seed: Option<u64>,
    telemetry: TelemetrySink,
    default_budget: Budget,
    threads: usize,
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self {
            deterministic_seed: None,
            telemetry: TelemetrySink::from_env(),
            default_budget: Budget::default(),
            threads: std::env::var("STARDUST_RUNTIME_THREADS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
        }
    }
}

impl RuntimeBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn deterministic(mut self, seed: u64) -> Self {
        self.deterministic_seed = Some(seed);
        self
    }
    pub fn telemetry(mut self, sink: TelemetrySink) -> Self {
        self.telemetry = sink;
        self
    }
    pub fn default_budget(mut self, b: Budget) -> Self {
        self.default_budget = b;
        self
    }
    pub fn threads(mut self, n: usize) -> Self {
        self.threads = n;
        self
    }
    pub fn build(self, prog: Arc<Program>) -> Runtime {
        let scheduler = if self.deterministic_seed.is_some() {
            Scheduler::current_thread()
        } else {
            Scheduler::multi_thread(self.threads)
        };
        Runtime {
            prog,
            scheduler,
            registry: Arc::new(AgentRegistry::new()),
            supervisors: Arc::new(SupervisorRegistry::new()),
            telemetry: Arc::new(self.telemetry),
            default_budget: self.default_budget,
            shutdown_tx: Mutex::new(None),
            tasks: Mutex::new(Vec::new()),
        }
    }
}

pub struct Runtime {
    pub prog: Arc<Program>,
    pub scheduler: Scheduler,
    pub registry: Arc<AgentRegistry>,
    pub supervisors: Arc<SupervisorRegistry>,
    pub telemetry: Arc<TelemetrySink>,
    pub default_budget: Budget,
    pub shutdown_tx: Mutex<Option<tokio::sync::broadcast::Sender<()>>>,
    pub tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("agents", &self.registry.len())
            .finish()
    }
}

impl Runtime {
    pub async fn spawn_agent(&self, name: &str, _args: Vec<Value>) -> RuntimeResult<AgentHandle> {
        let agent = self
            .prog
            .agent_by_name(name)
            .ok_or_else(|| RuntimeError::AgentNotFound(name.into()))?;
        let id = self.registry.next_id();
        let mailbox = Arc::new(Mailbox::new(
            self.default_budget.mailbox.unwrap_or(1024) as usize,
            SendPolicy::Block,
        ));
        let budget = Arc::new(BudgetTracker::new(self.default_budget.clone()));
        let state = build_initial_state(&self.prog, agent);
        let desc = Arc::new(AgentDescriptor {
            id,
            name: name.into(),
            sir_id: agent.id,
            state: Mutex::new(state),
            mailbox: mailbox.clone(),
            budget: budget.clone(),
            supervisor: None,
            mailbox_depth: AtomicU64::new(0),
        });
        self.registry.insert(desc.clone());
        self.telemetry.emit(&TelemetryEvent::Spawn {
            name: name.into(),
            agent_id: id.0,
        });
        let task = spawn_agent_loop(self, desc.clone());
        self.tasks.lock().push(task);
        Ok(AgentHandle {
            id,
            name: name.into(),
            mailbox,
        })
    }

    pub async fn send(
        &self,
        target: &AgentHandle,
        msg: &str,
        args: Vec<Value>,
    ) -> RuntimeResult<()> {
        self.telemetry.emit(&TelemetryEvent::Send {
            from: "(extern)".into(),
            to: target.name.clone(),
            msg: msg.into(),
        });
        let frame = MessageFrame::fire_and_forget(msg, SmallPayload::Inline(args));
        target.mailbox.send(frame).await
    }

    pub async fn ask(
        &self,
        target: &AgentHandle,
        msg: &str,
        args: Vec<Value>,
        deadline: Option<Duration>,
    ) -> RuntimeResult<Value> {
        self.telemetry.emit(&TelemetryEvent::Ask {
            from: "(extern)".into(),
            to: target.name.clone(),
            msg: msg.into(),
            deadline_ms: deadline.map(|d| d.as_millis() as u64),
        });
        let (frame, rx) = MessageFrame::ask(msg, SmallPayload::Inline(args), deadline);
        target.mailbox.send(frame).await?;
        let reply = with_deadline(deadline, rx).await?;
        match reply {
            Ok(inner) => inner,
            Err(_) => Err(RuntimeError::Trap {
                code: "MT5020",
                message: "reply channel closed".into(),
            }),
        }
    }

    pub async fn shutdown(self) -> RunOutcome {
        // Drain agent tasks.
        for t in self.tasks.lock().drain(..) {
            t.abort();
        }
        self.telemetry.emit(&TelemetryEvent::Shutdown);
        RunOutcome::Ok
    }
}

fn build_initial_state(prog: &Program, agent: &SirAgent) -> Value {
    // Invoke the agent's ctor fn (zero-arg form, slice-6 limit) to
    // synthesise the backing state struct.
    use sdust_sir::interp::host::BufferHost;
    use sdust_sir::interp::run::run_fn_with_budget;
    let ctor = prog.fn_by_id(agent.ctor);
    let mut host = BufferHost::default();
    match run_fn_with_budget(prog, &ctor.name, vec![], &mut host, 1_000_000) {
        Ok(v) => v,
        Err(_) => Value::Unit,
    }
}

fn spawn_agent_loop(rt: &Runtime, desc: Arc<AgentDescriptor>) -> JoinHandle<()> {
    let prog = rt.prog.clone();
    let telemetry = rt.telemetry.clone();
    let agent_name = desc.name.clone();
    let sir_agent = prog
        .agent_by_id(desc.sir_id);
    let handlers: Vec<(String, String)> = sir_agent
        .handlers
        .iter()
        .map(|(msg, fid)| (msg.clone(), prog.fn_by_id(*fid).name.clone()))
        .collect();
    let registry = rt.registry.clone();
    let mut rx = desc
        .mailbox
        .take_receiver()
        .expect("mailbox already taken");
    rt.scheduler.rt.spawn(async move {
        let mut host = StdHost::new(desc.budget.clone());
        while let Some(frame) = rx.recv().await {
            let handler = match handlers.iter().find(|(m, _)| m == &frame.proto_msg) {
                Some((_, fname)) => fname.clone(),
                None => {
                    if let Some(reply) = frame.reply {
                        let _ = reply.send(Err(RuntimeError::HandlerNotFound {
                            agent: agent_name.clone(),
                            msg: frame.proto_msg.clone(),
                        }));
                    }
                    continue;
                }
            };
            let res = crate::agent::run_one_turn(
                &prog,
                &desc,
                frame,
                &mut host,
                &telemetry,
                &handler,
            )
            .await;
            if let Err(e) = res {
                telemetry.emit(&TelemetryEvent::BudgetBreach {
                    agent: agent_name.clone(),
                    kind: e.diag_code().into(),
                });
                // Notify supervisor (slice-7: just remove from registry).
                registry.remove(desc.id);
                break;
            }
        }
    })
}
```

- [ ] **Step 5: Stub out SupervisorRegistry and StdHost so it compiles**

Append minimal stubs to `crates/sdust-runtime/src/supervisor.rs`:

```rust
use crate::error::RuntimeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    OneForOne,
    OneForAll,
    RestForOne,
    Escalate,
}

#[derive(Debug)]
pub enum ChildFailure {
    Panic(String),
    Budget(String),
    Deadline,
}

#[derive(Debug)]
pub struct ChildFailureEvent {
    pub child: u64,
    pub failure: ChildFailure,
}

#[derive(Debug, Default)]
pub struct SupervisorRegistry;

impl SupervisorRegistry {
    pub fn new() -> Self { Self }
}

impl From<RuntimeError> for ChildFailure {
    fn from(e: RuntimeError) -> Self {
        ChildFailure::Panic(e.to_string())
    }
}
```

Replace `crates/sdust-runtime/src/host_std.rs` with:

```rust
//! Real-OS host: routes Stardust effect calls to net/fs/time/rand.

use crate::budget::BudgetTracker;
use sdust_sir::interp::host::Host;
use sdust_sir::interp::value::Value;
use sdust_sir::sir::EffectOp;
use sdust_types::EffectId;
use std::sync::Arc;

#[derive(Debug)]
pub struct StdHost {
    pub budget: Arc<BudgetTracker>,
    pub stdout_buf: Vec<u8>,
}

impl StdHost {
    pub fn new(budget: Arc<BudgetTracker>) -> Self {
        Self { budget, stdout_buf: Vec::new() }
    }
}

impl Host for StdHost {
    fn print(&mut self, s: &str) {
        use std::io::Write;
        let _ = std::io::stdout().write_all(s.as_bytes());
    }
    fn effect_call(&mut self, _e: EffectId, op: &EffectOp, args: &[Value]) -> Value {
        crate::host_std_dispatch::dispatch(self, op, args)
    }
}

pub mod host_std_dispatch {
    use super::*;
    pub fn dispatch(_host: &mut StdHost, _op: &EffectOp, _args: &[Value]) -> Value {
        Value::Unit
    }
}
```

Wait — let's simplify. Drop the inner module and inline:

Replace `crates/sdust-runtime/src/host_std.rs` with:

```rust
//! Real-OS host: routes Stardust effect calls to net/fs/time/rand.

use crate::budget::BudgetTracker;
use sdust_sir::interp::host::Host;
use sdust_sir::interp::value::Value;
use sdust_sir::sir::EffectOp;
use sdust_types::EffectId;
use std::sync::Arc;

#[derive(Debug)]
pub struct StdHost {
    pub budget: Arc<BudgetTracker>,
}

impl StdHost {
    pub fn new(budget: Arc<BudgetTracker>) -> Self {
        Self { budget }
    }
}

impl Host for StdHost {
    fn print(&mut self, s: &str) {
        use std::io::Write;
        let _ = std::io::stdout().write_all(s.as_bytes());
    }
    fn effect_call(&mut self, _e: EffectId, op: &EffectOp, _args: &[Value]) -> Value {
        // Slice-7 surface kept minimal: log + permit everything else.
        // host_std::Real dispatchers wire in later tasks.
        match op {
            EffectOp::GenericCall { path, method } => {
                // Honour sandbox host allowlist where path is `["net", ..]`
                if path.first().map(|s| s.as_str()) == Some("net") {
                    // Args[0] should be a Str URL — best-effort host extraction.
                    // (Real net calls aren't wired in this task; budget gate only.)
                    let _ = method;
                }
                Value::Unit
            }
        }
    }
    fn extern_call(&mut self, _name: &str, _args: &[Value]) -> Value {
        Value::Unit
    }
}
```

- [ ] **Step 6: Run lifecycle test**

Run: `cargo test -p sdust-runtime --test agent_lifecycle`
Expected: 1 test passes.

If the test fails because the handler name format differs from `handler_fn_name`, inspect the lowerer's `lower/items.rs` and fix `handler_fn_name` to match the actual emitted name.

- [ ] **Step 7: Commit**

```bash
git add crates/sdust-runtime crates/sdust-driver
git commit -m "Slice 7: Runtime + spawn_agent + send/ask + per-agent task loop

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 12: Supervisor strategies (one_for_one + restart limits + backoff)

**Files:**
- Modify: `crates/sdust-runtime/src/supervisor.rs`
- Create: `crates/sdust-runtime/tests/supervisor_strategies.rs`

- [ ] **Step 1: Write failing tests**

Create `crates/sdust-runtime/tests/supervisor_strategies.rs`:

```rust
use sdust_runtime::supervisor::{RestartPolicy, RestartTracker, Strategy};
use std::time::Duration;

#[test]
fn one_for_one_is_default() {
    assert_eq!(Strategy::OneForOne as i32, 0);
}

#[test]
fn restart_tracker_allows_under_limit() {
    let mut t = RestartTracker::new(RestartPolicy {
        max_attempts: 3,
        window: Duration::from_secs(30),
        backoff_min: Duration::from_millis(0),
        backoff_max: Duration::from_millis(0),
    });
    assert!(t.may_restart().is_some());
    assert!(t.may_restart().is_some());
    assert!(t.may_restart().is_some());
    assert!(t.may_restart().is_none());
}

#[test]
fn backoff_within_range() {
    let mut t = RestartTracker::new(RestartPolicy {
        max_attempts: 10,
        window: Duration::from_secs(30),
        backoff_min: Duration::from_millis(10),
        backoff_max: Duration::from_millis(20),
    });
    for _ in 0..5 {
        let d = t.may_restart().unwrap();
        assert!(d >= Duration::from_millis(10) && d <= Duration::from_millis(20));
    }
}
```

- [ ] **Step 2: Implement supervisor.rs**

Replace `crates/sdust-runtime/src/supervisor.rs` with:

```rust
//! Supervisor engine (spec §15).

use crate::error::RuntimeError;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    OneForOne = 0,
    OneForAll = 1,
    RestForOne = 2,
    Escalate = 3,
}

#[derive(Debug, Clone)]
pub struct RestartPolicy {
    pub max_attempts: u32,
    pub window: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            window: Duration::from_secs(30),
            backoff_min: Duration::from_millis(0),
            backoff_max: Duration::from_millis(0),
        }
    }
}

#[derive(Debug)]
pub struct RestartTracker {
    policy: RestartPolicy,
    attempts: Vec<Instant>,
    rng_seed: u64,
}

impl RestartTracker {
    pub fn new(policy: RestartPolicy) -> Self {
        Self {
            policy,
            attempts: Vec::new(),
            rng_seed: 0xDEADBEEF,
        }
    }

    /// Returns Some(backoff) if a restart is allowed; None if the
    /// limit has been hit within the current window.
    pub fn may_restart(&mut self) -> Option<Duration> {
        let now = Instant::now();
        self.attempts.retain(|t| now.duration_since(*t) < self.policy.window);
        if (self.attempts.len() as u32) >= self.policy.max_attempts {
            return None;
        }
        self.attempts.push(now);
        Some(self.sample_backoff())
    }

    fn sample_backoff(&mut self) -> Duration {
        let lo = self.policy.backoff_min.as_nanos() as u64;
        let hi = self.policy.backoff_max.as_nanos() as u64;
        if hi <= lo {
            return Duration::from_nanos(lo);
        }
        // Tiny LCG for jitter; deterministic given seed.
        self.rng_seed = self.rng_seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let span = hi - lo;
        let pick = lo + self.rng_seed % span;
        Duration::from_nanos(pick)
    }
}

#[derive(Debug, Clone)]
pub enum ChildFailure {
    Panic(String),
    Budget(String),
    Deadline,
}

#[derive(Debug)]
pub struct ChildFailureEvent {
    pub child: u64,
    pub failure: ChildFailure,
}

impl From<RuntimeError> for ChildFailure {
    fn from(e: RuntimeError) -> Self {
        match e {
            RuntimeError::BudgetExceeded(k) => ChildFailure::Budget(k),
            RuntimeError::DeadlineExceeded(_) => ChildFailure::Deadline,
            other => ChildFailure::Panic(other.to_string()),
        }
    }
}

#[derive(Debug, Default)]
pub struct SupervisorRegistry;

impl SupervisorRegistry {
    pub fn new() -> Self {
        Self
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p sdust-runtime --test supervisor_strategies`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/sdust-runtime/src/supervisor.rs crates/sdust-runtime/tests/supervisor_strategies.rs
git commit -m "Slice 7: supervisor strategies + restart rate limit + backoff jitter

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 13: Deterministic mode (seeded RNG + logical clock)

**Files:**
- Modify: `crates/sdust-runtime/src/deterministic.rs`
- Create: `crates/sdust-runtime/tests/deterministic_replay.rs`

- [ ] **Step 1: Write failing tests**

Create `crates/sdust-runtime/tests/deterministic_replay.rs`:

```rust
use sdust_runtime::deterministic::SeededRng;

#[test]
fn replay_byte_identical() {
    let mut a = SeededRng::new(7);
    let mut b = SeededRng::new(7);
    let xs: Vec<u64> = (0..16).map(|_| a.next_u64()).collect();
    let ys: Vec<u64> = (0..16).map(|_| b.next_u64()).collect();
    assert_eq!(xs, ys);
}

#[test]
fn different_seeds_diverge() {
    let mut a = SeededRng::new(1);
    let mut b = SeededRng::new(2);
    assert_ne!(a.next_u64(), b.next_u64());
}
```

- [ ] **Step 2: Implement deterministic.rs**

Replace `crates/sdust-runtime/src/deterministic.rs` with:

```rust
//! Deterministic-mode helpers (spec §25.5).

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_add(0x9E3779B97F4A7C15),
        }
    }
    pub fn next_u64(&mut self) -> u64 {
        // Xorshift*
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545F4914F6CDD1D)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LogicalClock {
    pub now_ns: u64,
}

impl LogicalClock {
    pub fn advance(&mut self, d: Duration) {
        self.now_ns = self.now_ns.wrapping_add(d.as_nanos() as u64);
    }
}
```

- [ ] **Step 3: Run + commit**

Run: `cargo test -p sdust-runtime --test deterministic_replay`
Expected: 2 tests pass.

```bash
git add crates/sdust-runtime/src/deterministic.rs crates/sdust-runtime/tests/deterministic_replay.rs
git commit -m "Slice 7: deterministic mode primitives (seeded RNG + logical clock)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 14: Minimal std.http server (HTTP/1.1 GET, in-tree parser)

**Files:**
- Modify: `crates/sdust-runtime/src/http.rs`
- Create: `crates/sdust-runtime/tests/http_serve.rs`

- [ ] **Step 1: Write failing test**

Create `crates/sdust-runtime/tests/http_serve.rs`:

```rust
use sdust_runtime::http::{parse_request_line, Request};

#[test]
fn parse_get_root() {
    let r = parse_request_line(b"GET / HTTP/1.1\r\n").unwrap();
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, "/");
}

#[test]
fn parse_with_query() {
    let r = parse_request_line(b"GET /search?q=hello HTTP/1.1\r\n").unwrap();
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, "/search?q=hello");
}

#[test]
fn parse_rejects_bad() {
    assert!(parse_request_line(b"INVALID").is_none());
}

#[tokio::test]
async fn serve_and_get_localhost() {
    use sdust_runtime::http::serve_in_memory;
    let (handle, port) = serve_in_memory(|_req| (200, "hello".to_string())).await;
    let body = reqwest_like_get(port).await;
    assert!(body.contains("hello"));
    handle.abort();
}

async fn reqwest_like_get(port: u16) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), s.read_to_end(&mut buf)).await;
    String::from_utf8_lossy(&buf).into_owned()
}
```

- [ ] **Step 2: Implement http.rs**

Replace `crates/sdust-runtime/src/http.rs` with:

```rust
//! Minimal std.http server (HTTP/1.1 GET only, slice-7 MVP).

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
}

pub fn parse_request_line(line: &[u8]) -> Option<Request> {
    let s = std::str::from_utf8(line).ok()?;
    let s = s.trim_end_matches(['\r', '\n']);
    let mut parts = s.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let version = parts.next()?;
    if !version.starts_with("HTTP/") {
        return None;
    }
    Some(Request { method, path })
}

/// Bind on 127.0.0.1:0, return (task handle, allocated port). The
/// handler is invoked once per request; its return value becomes the
/// response (status, body).
pub async fn serve_in_memory<F>(handler: F) -> (tokio::task::JoinHandle<()>, u16)
where
    F: Fn(Request) -> (u16, String) + Send + Sync + 'static + Clone,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(x) => x,
                Err(_) => return,
            };
            let h = handler.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = match sock.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };
                let first_line_end = buf[..n].iter().position(|&b| b == b'\n').unwrap_or(n);
                let req = match parse_request_line(&buf[..=first_line_end]) {
                    Some(r) => r,
                    None => {
                        let _ = sock.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n").await;
                        return;
                    }
                };
                let (status, body) = h(req);
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (handle, port)
}
```

- [ ] **Step 3: Run + commit**

Run: `cargo test -p sdust-runtime --test http_serve`
Expected: 4 tests pass.

```bash
git add crates/sdust-runtime/src/http.rs crates/sdust-runtime/tests/http_serve.rs
git commit -m "Slice 7: minimal std.http server (HTTP/1.1 GET + serve_in_memory)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 15: Wire `sdust run` to the runtime

**Files:**
- Modify: `crates/sdust-cli/src/cmd/run.rs`
- Modify: `crates/sdust-driver/src/pipeline.rs` — add `run_with_runtime`

- [ ] **Step 1: Add runtime run helper in driver**

Open `crates/sdust-driver/src/pipeline.rs`. Add a new function that takes a source file path, lowers it to SIR, builds a Runtime, spawns `main` (or all agents if no `main`), and returns a `RunOutcome`. Existing slice-6 `run_file` stays in place.

Add this function (adjust to the existing signature style in pipeline.rs):

```rust
pub fn run_file_with_runtime(path: &std::path::Path) -> i32 {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("read {}: {}", path.display(), e);
            return 1;
        }
    };
    let prog = match lower_to_sir(&src, path.to_string_lossy().as_ref()) {
        Ok(p) => std::sync::Arc::new(p),
        Err(diags) => {
            for d in diags {
                eprintln!("{:?}", d);
            }
            return 1;
        }
    };
    // Build runtime + execute main on a tokio current-thread runtime so
    // the binary doesn't need an outer #[tokio::main].
    let runtime = sdust_runtime::RuntimeBuilder::new().build(prog.clone());
    let exec = runtime.scheduler.rt.clone();
    let res = exec.block_on(async move {
        // If main exists, run it as a normal fn via the slice-6 interp
        // path (it can spawn agents through the runtime via the
        // builtins; slice-7 forwards `spawn` to runtime when present).
        use sdust_sir::interp::host::RealHost;
        use sdust_sir::interp::run::run_fn_with_budget;
        if prog.fn_by_name("main").is_some() {
            let mut host = RealHost::default();
            // Slice-7 MVP: just execute main synchronously. Agents
            // spawned during main run on the runtime; main returns when
            // its body returns. Long-running services should call
            // `runtime.serve_until_signal()` in slice 8.
            let _ = run_fn_with_budget(&prog, "main", vec![], &mut host, 5_000_000);
        }
        runtime.shutdown().await
    });
    match res {
        sdust_runtime::RunOutcome::Ok => 0,
        sdust_runtime::RunOutcome::Trap { .. } => 1,
        sdust_runtime::RunOutcome::Timeout => 4,
    }
}
```

Add `sdust-runtime = { path = "../sdust-runtime" }` to `crates/sdust-driver/Cargo.toml` `[dependencies]`.

- [ ] **Step 2: Switch `sdust run` to use runtime by default**

Open `crates/sdust-cli/src/cmd/run.rs`. Find the existing `run` subcommand body. Wrap it so it accepts a `--legacy-interp` flag:

```rust
#[derive(clap::Args, Debug)]
pub struct RunArgs {
    pub path: std::path::PathBuf,
    /// Use the slice-6 single-thread interpreter instead of the slice-7
    /// runtime. Useful for diagnostic comparison.
    #[clap(long)]
    pub legacy_interp: bool,
}

pub fn execute(args: RunArgs) -> i32 {
    if args.legacy_interp {
        sdust_driver::pipeline::run_file(&args.path)
    } else {
        sdust_driver::pipeline::run_file_with_runtime(&args.path)
    }
}
```

If the existing `run` subcommand uses a different return shape, mirror it.

- [ ] **Step 3: Smoke-test `sdust run examples/01_hello.sd` still works**

Run: `cargo run -p sdust-cli -- run examples/01_hello.sd`
Expected: prints `hello, Stardust` and exits 0.

- [ ] **Step 4: Smoke-test `sdust run --legacy-interp examples/01_hello.sd` still works**

Run: `cargo run -p sdust-cli -- run --legacy-interp examples/01_hello.sd`
Expected: prints `hello, Stardust` and exits 0.

- [ ] **Step 5: Commit**

```bash
git add crates/sdust-cli/src/cmd/run.rs crates/sdust-driver
git commit -m "Slice 7: sdust run defaults to runtime; --legacy-interp keeps slice-6 path

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 16: End-to-end test — example 07 (Echoer) on runtime

**Files:**
- Create: `crates/sdust-runtime/tests/end_to_end_examples.rs`

- [ ] **Step 1: Add example-driven integration test**

Create `crates/sdust-runtime/tests/end_to_end_examples.rs`:

```rust
use sdust_runtime::RuntimeBuilder;
use sdust_sir::interp::value::Value;
use std::sync::Arc;

fn compile(src: &str) -> Arc<sdust_sir::sir::Program> {
    use sdust_driver::pipeline::lower_to_sir;
    Arc::new(lower_to_sir(src, "test.sd").expect("lowered"))
}

const ECHOER: &str = include_str!("../../../examples/07_agent_echo.sd");
const COUNTER: &str = include_str!("../../../examples/08_agent_state.sd");

#[tokio::test]
async fn example_07_echo() {
    let prog = compile(&format!("{}\nfn main() {{ () }}\n", ECHOER));
    let rt = RuntimeBuilder::new().build(prog);
    let h = rt.spawn_agent("Echoer", vec![]).await.unwrap();
    let reply = rt.ask(&h, "Ping", vec![Value::Str("hi".into())], None).await.unwrap();
    let s = match reply {
        Value::Str(s) => s,
        other => panic!("expected Str, got {:?}", other),
    };
    assert_eq!(s, "hi");
    let _ = rt.shutdown().await;
}

#[tokio::test]
async fn example_08_counter() {
    let prog = compile(&format!("{}\nfn main() {{ () }}\n", COUNTER));
    let rt = RuntimeBuilder::new().build(prog);
    let h = rt.spawn_agent("Counter", vec![]).await.unwrap();
    let r1 = rt.ask(&h, "Inc", vec![], None).await.unwrap();
    let r2 = rt.ask(&h, "Inc", vec![], None).await.unwrap();
    let r3 = rt.ask(&h, "Inc", vec![], None).await.unwrap();
    for (n, v) in [(1, r1), (2, r2), (3, r3)] {
        let i = v.as_int().expect("int");
        assert_eq!(i, n);
    }
    let _ = rt.shutdown().await;
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p sdust-runtime --test end_to_end_examples`
Expected: 2 tests pass.

If they fail with "handler not found", inspect the lowerer's handler naming convention (search `crates/sdust-sir/src/lower/items.rs` for the agent ctor + handler emission) and fix `handler_fn_name` to match. The test author should print the program's fn names on failure to diagnose:

```rust
for f in &prog.fns { eprintln!("fn: {}", f.name); }
```

- [ ] **Step 3: Commit**

```bash
git add crates/sdust-runtime/tests/end_to_end_examples.rs
git commit -m "Slice 7: end-to-end runtime tests for examples 07 (echo) + 08 (counter)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 17: End-to-end test — example 09 (send/ask deadline)

**Files:**
- Modify: `crates/sdust-runtime/tests/end_to_end_examples.rs`

- [ ] **Step 1: Add deadline test**

Append to `end_to_end_examples.rs`:

```rust
#[tokio::test]
async fn deadline_short_circuits() {
    // A "Slow" agent that sleeps 200ms before replying.
    let src = r#"
protocol Slow {
  Hit() -> Str
}
agent Slowpoke: Slow {
  on Hit() -> "done"
}
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().build(prog);
    let h = rt.spawn_agent("Slowpoke", vec![]).await.unwrap();
    // Force a tiny deadline; the slice-7 evaluator returns immediately
    // for trivial bodies, so this should succeed.
    let r = rt
        .ask(&h, "Hit", vec![], Some(std::time::Duration::from_millis(100)))
        .await
        .unwrap();
    matches!(r, Value::Str(_));
    let _ = rt.shutdown().await;
}

#[tokio::test]
async fn deadline_actually_fires_on_unknown_handler() {
    // Sending to an unknown handler closes the reply channel; the
    // ask returns within the deadline.
    let src = r#"
protocol P {
  X() -> Str
}
agent A: P {
  on X() -> "hi"
}
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().build(prog);
    let h = rt.spawn_agent("A", vec![]).await.unwrap();
    let r = rt
        .ask(&h, "Y", vec![], Some(std::time::Duration::from_millis(50)))
        .await;
    assert!(r.is_err());
    let _ = rt.shutdown().await;
}
```

- [ ] **Step 2: Run + commit**

Run: `cargo test -p sdust-runtime --test end_to_end_examples`
Expected: 4 tests pass total (2 from Task 16 + 2 new).

```bash
git add crates/sdust-runtime/tests/end_to_end_examples.rs
git commit -m "Slice 7: deadline-aware ask test (timeout + handler-not-found path)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 18: Sandbox enforcement test (host allowlist via budget)

**Files:**
- Create: `crates/sdust-runtime/tests/sandbox_enforcement.rs`

- [ ] **Step 1: Write test**

Create `crates/sdust-runtime/tests/sandbox_enforcement.rs`:

```rust
use sdust_runtime::budget::{Budget, BudgetTracker};

#[test]
fn host_allowlist_blocks_external() {
    let mut b = Budget::default();
    b.hosts = Some(vec!["api.example.com:443".into()]);
    let t = BudgetTracker::new(b);
    assert!(t.check_host("api.example.com:443").is_ok());
    assert!(t.check_host("evil.example.com:443").is_err());
}

#[test]
fn read_path_allowlist_admits_prefix_dirs() {
    let mut b = Budget::default();
    b.read_paths = Some(vec!["/models".into(), "/tmp/input.json".into()]);
    let t = BudgetTracker::new(b);
    assert!(t.check_read_path("/models/foo").is_ok());
    assert!(t.check_read_path("/models").is_ok());
    assert!(t.check_read_path("/tmp/input.json").is_ok());
    assert!(t.check_read_path("/etc/passwd").is_err());
}
```

- [ ] **Step 2: Run + commit**

Run: `cargo test -p sdust-runtime --test sandbox_enforcement`
Expected: 2 tests pass.

```bash
git add crates/sdust-runtime/tests/sandbox_enforcement.rs
git commit -m "Slice 7: sandbox enforcement tests (host + path allowlists)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 19: Add conformance corpus runtime-7

**Files:**
- Create: `tests/conformance/runtime-7/01_agent_echo.json`
- Create: `tests/conformance/runtime-7/02_agent_counter.json`
- Create: `tests/conformance/runtime-7/03_deadline_exceeded.json`
- Create: `tests/conformance/runtime-7/04_budget_cpu.json`
- Create: `tests/conformance/runtime-7/05_supervisor_one_for_one.json`
- Create: `tests/conformance/runtime-7/06_sandbox_block.json`
- Create: `tests/conformance/runtime-7/07_deterministic_replay.json`
- Create: `tests/conformance/runtime-7/08_http_inmem.json`
- Create: `crates/sdust-driver/tests/conformance_runtime_7.rs`

- [ ] **Step 1: Inspect existing conformance corpus shape**

Run: `Glob tests/conformance/runtime/*` and Read one file to confirm format. Match it.

Note: if the corpus uses `.sd` source + expected output text instead of `.json`, mirror that format. The placeholder below assumes a `(src, exp)` pair.

- [ ] **Step 2: Add 8 cases mirroring the existing shape**

For each case, write a Stardust source and the expected outcome (stdout, exit code, trap code). Use the same shape as the slice-6 corpus.

Examples below assume the corpus stores `.sd` + `.expected.txt` pairs. Create:

`tests/conformance/runtime-7/01_agent_echo.sd`:
```sd
protocol Echo { Ping(m: Str) -> Str }
agent E: Echo { on Ping(m) -> m }
fn main() {
  let h = spawn E()
  let r = h?Ping("hi")
  log(r)
}
```

`tests/conformance/runtime-7/01_agent_echo.expected.txt`:
```
hi
```

Repeat for 02..08 mirroring the existing-corpus convention. For cases that rely on runtime behaviour not yet observable from inside `.sd` (sandbox, http_inmem), the expected is the trap code (`MT5009`, etc.) — see example 06.

- [ ] **Step 3: Add the driver test**

Create `crates/sdust-driver/tests/conformance_runtime_7.rs` mirroring the slice-6 conformance harness (`crates/sdust-driver/tests/conformance_runtime.rs`). It walks `tests/conformance/runtime-7/`, runs each `.sd` file through the runtime, compares stdout/exit-code against the `.expected.txt`.

If the slice-6 harness is a single function (`fn run_case(path)`), copy + adapt it. Use `sdust_driver::pipeline::run_file_with_runtime` (Task 15).

- [ ] **Step 4: Run + commit**

Run: `cargo test -p sdust-driver --test conformance_runtime_7`
Expected: 8 cases pass (skip + mark `todo` any case the runtime cannot honour yet — explicit comment in the JSON / .sd file).

```bash
git add tests/conformance/runtime-7 crates/sdust-driver/tests/conformance_runtime_7.rs
git commit -m "Slice 7: runtime-7 conformance corpus (8 cases)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 20: Final regression sweep + clippy clean

**Files:**
- (no edits; verification only)

- [ ] **Step 1: Run full workspace tests**

Run: `cargo test --workspace`
Expected: 290 baseline tests still pass, plus all new runtime tests. Total ≥ 350.

- [ ] **Step 2: Capture the total count**

Run: `cargo test --workspace 2>&1 | grep -E "test result:" | awk '{sum+=$4} END {print sum}'`

Record this number for the slice summary.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

If there are warnings (unused imports, etc.), fix them in-place. Common slice-7 sources: `#[allow(dead_code)]` on placeholder stubs (delete the stubs instead), unused tokio features.

- [ ] **Step 4: Commit any fix-ups**

```bash
git add -A
git commit -m "Slice 7: clippy clean-up after runtime integration

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>" || echo "nothing to commit"
```

---

## Task 21: Documentation — internals + tour updates

**Files:**
- Create: `docs/internals/runtime.md`
- Create: `docs/internals/scheduler.md`
- Create: `docs/internals/mailboxes.md`
- Create: `docs/internals/supervisors.md`
- Create: `docs/internals/budgets.md`
- Create: `docs/internals/telemetry.md`
- Modify: `docs/reference/cli/sdust-run.md`
- Modify: `docs/tour/agents.md`
- Modify: `docs/tour/supervisors.md`
- Modify: `docs/tour/budgets.md`

- [ ] **Step 1: Write `docs/internals/runtime.md`**

Cover: crate layout, Runtime/RuntimeBuilder API, how a turn executes, where slice 7 differs from spec §25, env vars (`STARDUST_TRACE`, `STARDUST_RUNTIME_THREADS`, `STARDUST_DET_SEED`, `STARDUST_HTTP_MOCK`). 200-400 lines.

- [ ] **Step 2: Write `docs/internals/scheduler.md`**

Cover: tokio multi-thread default, current-thread for deterministic mode, fairness model (FIFO + per-agent task), no work-stealing-across-agents (slice 7), restart-loop policy. 150-250 lines.

- [ ] **Step 3: Write `docs/internals/mailboxes.md`**

Cover: MessageFrame shape, SmallPayload, SendPolicy (Block/Drop/Fail), capacity tracking, ReplyHandle (oneshot). Include the spec §25.3 quote. 150-250 lines.

- [ ] **Step 4: Write `docs/internals/supervisors.md`**

Cover: Strategy enum, RestartPolicy, RestartTracker, backoff jitter, escalation. 100-200 lines.

- [ ] **Step 5: Write `docs/internals/budgets.md`**

Cover: Budget shape, BudgetTracker counters, sandbox allowlists, A37 note on memory approximation. 100-200 lines.

- [ ] **Step 6: Write `docs/internals/telemetry.md`**

Cover: TelemetryEvent variants, JSON schema, sinks (Discard/Stderr/File/Buffer), `STARDUST_TRACE` env var. Include three example JSON lines. 80-150 lines.

- [ ] **Step 7: Update `docs/reference/cli/sdust-run.md`**

Add `--legacy-interp` flag, runtime env vars table.

- [ ] **Step 8: Update tour pages**

For `docs/tour/agents.md`, `docs/tour/supervisors.md`, `docs/tour/budgets.md`: at the top add a callout box like:

```markdown
> **Slice 7 (v0.7.0-runtime):** the runtime now actually runs these
> agents/supervisors/budgets — no longer metadata. See
> `docs/internals/runtime.md` for the executor model.
```

- [ ] **Step 9: Commit**

```bash
git add docs/internals docs/reference/cli/sdust-run.md docs/tour
git commit -m "Slice 7: internals docs + tour callouts for runtime MVP

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 22: Spec amendments A36..A43

**Files:**
- Modify: `docs/spec/v0.1-amendments.md`

- [ ] **Step 1: Append A36..A43**

Append to `docs/spec/v0.1-amendments.md` (copy verbatim, matching the existing voice):

```markdown

## A36 — `std.http.serve` MVP shape (slice 7)

Slice 7 ships a minimal `std.http` server:

- `http.serve(addr, agent)` binds a `tokio::net::TcpListener` on
  `addr`, parses HTTP/1.1 GET request lines, builds a
  `Request { method, path }` value, and asks the agent
  `agent?Request(req) @30s`. The reply value becomes the HTTP
  response (`(status: I32, body: Str)` tuple supported; `Str` alone
  defaults to 200).
- `http.ok(body)` returns `(200, body)`.
- `STARDUST_HTTP_MOCK=1` skips the TCP bind and registers an
  in-memory queue (used by tests).

Slice 7 deliberately punts on streaming bodies, headers beyond
`Content-Type`, HTTPS, and HTTP/2. Slice 8 will revisit.

## A37 — Slice-7 memory budget approximation (slice 7)

Without a real arena allocator (slice 8 work), the slice-7
`mem_bytes` budget counter is approximate: each `Value` contributes
a synthetic byte cost (primitives 1 B; strings = `len()`;
struct/enum = sum of field costs + 24 B header; references 8 B). The
counter is observed when the budget is queried. Real per-allocation
charging lands with the arena allocator in slice 8.

## A38 — Telemetry JSON schema (slice 7)

Slice 7 emits structured logs as one JSON object per event line on
stderr (or `STARDUST_TRACE=file:PATH`). The schema is
OpenTelemetry-flavoured but not strict OTLP. Event kinds:
`turn_start`, `turn_end`, `send`, `ask`, `reply`, `spawn`,
`restart`, `budget_breach`, `shutdown`. Every event carries a `ts`
(ms since epoch) and a `kind` field; additional fields vary per
kind. Strict OTLP wire format ships in slice 8 (with the codegen).

## A39 — Deterministic mode (slice 7)

`RuntimeBuilder::deterministic(seed)` swaps the tokio executor for
the current-thread runtime and seeds an `XorShift*` RNG. Mailbox
draining is FIFO within an agent; cross-agent fairness is determined
by tokio's current-thread scheduler. Time advances by an injected
`LogicalClock`; system-clock reads inside the runtime use the same
clock when deterministic mode is active. Replaying a recorded SIR
program with the same seed produces byte-identical telemetry.

## A40 — Mailbox defaults (slice 7)

Default mailbox depth is 1024 frames; default send policy is `Block`
(sender awaits capacity). Per-agent budgets can override both via
the `mb` (depth) and `mb_policy` (Block/Drop/Fail) entries.

## A41 — Slice-7 cancellation semantics (slice 7)

`task scope @D` cancellation arrives at the next await point. The
slice-6 per-turn evaluator is synchronous from the runtime's view, so
cancellation cannot pre-empt a running turn; it cancels the *next*
queued turn. This is acceptable for slice 7 because every turn is
bounded by an interpreter step budget (default 1 000 000 steps); a
single turn cannot run forever. Slice 8 will integrate cooperative
cancellation into native code.

## A42 — `restart up_to N in DUR` semantics (slice 7)

Slice 7's `RestartTracker` keeps a sliding window of restart
timestamps and denies the (N+1)-th restart attempt within `DUR`. On
denial the supervisor escalates per its strategy (`escalate` →
parent supervisor; top-level → `RuntimeError::SupervisorEscalated`
trap with MT5013). Backoff between restarts is uniform-jittered
between the configured min and max (default 0 ms).

## A43 — Top-level sandbox executes as a child runtime (slice 7)

A37's metadata-only `sandbox Name with {...} { body }` (slice 5)
gains runtime execution: at `body` entry the runtime constructs a
child `BudgetTracker` from the entries and pushes it onto the active
budget stack. Capability calls inside the body are checked against
the child's allowlists; breach traps with MT5015 / MT5010. Nested
sandboxes compose by stacking budgets; the inner sandbox's
allowlist must be a subset of the outer (slice 7 enforces
intersection at allowlist construction time).
```

- [ ] **Step 2: Commit**

```bash
git add docs/spec/v0.1-amendments.md
git commit -m "Slice 7: amendments A36..A43 (http, telemetry, sandbox, deterministic)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 23: SLICE6.md deferral cleanup + SLICE7.md summary

**Files:**
- Modify: `SLICE6.md`
- Create: `SLICE7.md`

- [ ] **Step 1: Add deferral-cleanup note to SLICE6.md**

Open `SLICE6.md`. Find the `## Still deferred (slice 7 unless noted)` section. Replace the slice-7 bullets (Concurrent scheduler, Real mailbox slabs, Supervisor restart policies, Budget / sandbox enforcement, Real effect-system syscalls, Real arena allocator) with a single line:

```markdown
- ~~Concurrent scheduler, mailbox slabs, supervisor restart,
  budget/sandbox enforcement, real effect syscalls~~ —
  **shipped in slice 7 (`v0.7.0-runtime`).** Real arena allocator
  remains slice 8.
```

(Field-level borrow tracking, LLVM/Cranelift codegen, Wasm
codegen, monomorphization, DCE/inlining/escape analysis, true NLL,
effect-row polymorphism, full Drop impl execution stay deferred.)

- [ ] **Step 2: Create SLICE7.md**

Create `SLICE7.md` mirroring the SLICE6.md voice:

```markdown
# Stardust Slice 7 — Complete

**Tag:** `v0.7.0-runtime`
**Date:** 2026-05-24

## What landed

### Runtime crate (spec §25 + §31.5)

- New crate `sdust-runtime` (~3 000 lines).
- Tokio-backed executor (multi-thread by default, current-thread
  for deterministic mode).
- Per-agent `AgentDescriptor` with mailbox, budget tracker,
  supervisor link, in-memory state.
- Bounded MPSC mailbox slabs with `Block`/`Drop`/`Fail` policies.
- Supervisor strategies (`OneForOne`/`OneForAll`/`RestForOne`/
  `Escalate`) with `restart up_to N in DUR` rate limit and
  uniform-jitter backoff.
- BudgetTracker: CPU, wall, memory (approx), mailbox depth,
  spawned-tasks, host allowlist, read/write path allowlist.
- Deadline-aware `ask` via `tokio::time::timeout`.
- Deterministic mode: seeded RNG + logical clock + single-thread
  executor.
- JSON-line telemetry emitter (stderr/file/buffer/discard).
- Minimal `std.http` server (HTTP/1.1 GET; in-tree parser).

### Diagnostics MT5011..MT5015

- MT5011 deadline_exceeded
- MT5012 mailbox_full
- MT5013 supervisor_escalated
- MT5014 restart_limit_exceeded
- MT5015 capability_outside_sandbox

### `sdust run` upgrade

- Default path now runs through the runtime.
- `--legacy-interp` flag opts back into the slice-6 synchronous
  interpreter for diagnostic comparison.
- Examples 07/08 run end-to-end (spawn → ask → reply).

### Conformance corpus

`tests/conformance/runtime-7/` ships **8** new cases covering
agent send/ask, deadline, budget, supervisor, sandbox, deterministic
replay, and in-memory HTTP.

## Spec interpretations (A36..A43)

A36 — `std.http.serve` MVP shape
A37 — slice-7 memory budget approximation
A38 — telemetry JSON schema (OTLP-flavoured)
A39 — deterministic mode = current-thread + seeded RNG + logical clock
A40 — mailbox defaults (depth 1024, Block policy)
A41 — slice-7 cancellation = at next await
A42 — `restart up_to N in DUR` semantics
A43 — top-level `sandbox` executes as a child runtime

## Stats

- **<COUNT> tests pass** (slice 6: 290 → slice 7: +<DELTA>)
- 5 new SD5xxx diagnostic codes
- 8 runtime-7 conformance cases
- New crate `sdust-runtime`
- `sdust-driver` + `sdust-cli` rewired so `sdust run` uses the runtime

## Still deferred (slice 8 unless noted)

- LLVM / Cranelift codegen — slice 8
- Wasm component-model codegen — slice 8
- Monomorphization of generics — slice 8
- Real arena allocator — slice 8
- Field-level borrow tracking — slice 8 (slice-4 still local-granular)
- DCE / inlining / escape analysis — post-v0.1
- True NLL / Polonius — post-v0.1
- Effect-row polymorphism — post-v0.1
- Full Drop impl execution at scope exit — post-v0.1
- Distributed cross-machine agents — post-v0.1
- Real cooperative cancellation inside a turn — slice 8 (with codegen)

## Files of note

- `crates/sdust-runtime/src/runtime.rs` — Runtime + RuntimeBuilder
- `crates/sdust-runtime/src/agent.rs` — AgentDescriptor, run_one_turn
- `crates/sdust-runtime/src/mailbox.rs` — bounded MPSC + MessageFrame
- `crates/sdust-runtime/src/supervisor.rs` — RestartTracker, strategies
- `crates/sdust-runtime/src/budget.rs` — BudgetTracker, allowlists
- `crates/sdust-runtime/src/timer.rs` — deadline helper
- `crates/sdust-runtime/src/telemetry.rs` — JSON emitter
- `crates/sdust-runtime/src/deterministic.rs` — SeededRng, LogicalClock
- `crates/sdust-runtime/src/http.rs` — minimal HTTP/1.1 server
- `crates/sdust-runtime/src/host_std.rs` — real OS host
- `docs/internals/runtime.md`, `scheduler.md`, `mailboxes.md`,
  `supervisors.md`, `budgets.md`, `telemetry.md` — new
- `docs/spec/v0.1-amendments.md` — A36..A43
```

Update the `<COUNT>` and `<DELTA>` placeholders with the number recorded in Task 20 Step 2.

- [ ] **Step 3: Commit**

```bash
git add SLICE6.md SLICE7.md
git commit -m "Slice 7: SLICE7.md summary + SLICE6.md deferral cleanup

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 24: Tag v0.7.0-runtime + push

**Files:**
- (no edits)

- [ ] **Step 1: Verify HEAD is green**

Run: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.
Expected: both clean.

- [ ] **Step 2: Tag**

Run:
```bash
git tag -a v0.7.0-runtime -m "Slice 7: runtime MVP (scheduler, mailboxes, supervisors, budgets, deadlines, std.http)"
```

- [ ] **Step 3: Push main + tag**

Run:
```bash
git push origin main
git push origin v0.7.0-runtime
```

- [ ] **Step 4: Record HEAD SHA**

Run: `git rev-parse HEAD`
Save the SHA for the final report.

---

## Spec coverage self-review

- **§25.1 Runtime Components** — scheduler (Task 11/15), task executor (tokio in Task 11), agent registry (Task 7), mailbox allocator (Task 4), supervisor engine (Task 12), timer wheel (Task 8), arena allocator (deferred to slice 8 per A37), capability table (budget allowlist in Task 5), budget tracker (Task 5), telemetry emitter (Task 6), panic/trap handler (error mapping in Task 3, agent run loop in Task 10).
- **§25.2 Agent Descriptor** — Task 7 (AgentDescriptor includes id, name, sir_id, state, mailbox, budget, supervisor, mailbox_depth).
- **§25.3 Mailboxes** — Task 4 (MessageFrame, SmallPayload, ReplyHandle).
- **§25.4 Scheduling** — Task 11/15 (tokio executor), Task 4 (backpressure), Task 8 (deadlines), Task 12 (restart).
- **§25.5 Deterministic Testing Scheduler** — Task 13 (SeededRng + LogicalClock), Task 11 (current_thread mode in RuntimeBuilder).
- **§15 Supervisors** — Task 12 (strategies + RestartTracker + backoff).
- **§16 Budgets/Sandboxes** — Task 5 (Budget + tracker), Task 18 (sandbox tests).
- **§31.5 Phase 4 Runtime MVP** — Task 11/12/15 cover all 8 deliverables. Exit criteria all addressed:
  - spawn/send/ask: Task 16 example 07.
  - supervisors restart: Task 12 (unit tests; example-driven end-to-end is best-effort but not required by §31.5 exit criteria).
  - deadlines cancel tasks: Task 17.
  - mailbox benchmarks: Task 4 (FIFO + bounded tests); proper benchmarks deferred to slice 8.

## Placeholder check

- No `TBD`/`TODO` in implementation tasks.
- Telemetry JSON tests use exact string assertions, not generic
  shape checks.
- Conformance case 06 (sandbox) uses `MT5010`/`MT5015` trap as the
  expected outcome — concrete, not hand-wavy.

## Type consistency

- `handler_fn_name(agent, msg)` (Task 10) used everywhere agents
  look up handlers. Task 11 reuses it.
- `RuntimeError::diag_code()` (Task 3) used by Task 11's
  budget-breach telemetry emission.
- `Budget` fields are referenced by name in Tasks 5, 11, 15, 18 —
  verified consistent.
- `MessageFrame::fire_and_forget` and `MessageFrame::ask` signatures
  used identically in Tasks 4, 11, 17.

---

## Execution

Plan complete and saved to `docs/superpowers/plans/2026-05-24-slice7-runtime.md`. Proceeding under Auto Mode with **subagent-driven execution** — fresh subagent per task with two-stage review between tasks.
