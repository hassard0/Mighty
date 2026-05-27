//! Wire format for deterministic-replay traces.
//!
//! ## Wire-version policy
//!
//! [`TRACE_WIRE_VERSION`] is bumped only for breaking changes; additive
//! variant fields use serde defaults so old readers still decode new
//! writers (and vice-versa for tail-only events). The replayer refuses
//! traces with `version > TRACE_WIRE_VERSION` to keep the contract
//! one-way-stable.
//!
//! ### v0.19 — wire version 2 (byte-identical replay)
//!
//! v1 payloads were [`Vec<u8>`] of the `format!("{:?}", args)` Debug
//! rendering — opaque, lossy, and unable to drive byte-identical
//! re-execution. v2 introduces [`ReplayValue`], a structural mirror of
//! the runtime's `Value` enum, serialized into [`ReplayPayload::Values`]
//! on the [`TraceEvent::MessageSent`] event. The existing
//! [`ReplayPayload::Opaque`] arm is kept so the runtime hot path can
//! still emit the cheap Debug-formatted bytes when only summary
//! inspection is wanted.
//!
//! v0.19 retains **backwards-compat read** of v1 traces: the [`decode`]
//! path (see [`super::recorder::decode`]) detects `version == 1` on the
//! disk struct and lifts the legacy `Vec<u8>` payload field into a
//! [`ReplayPayload::Opaque`] arm. v1 traces therefore continue to load
//! cleanly with the v0.19 replayer.
//!
//! ### v0.29 — wire version 3 (structural LLM turns)
//!
//! v3 adds a new [`TraceEvent::LlmCall`] variant that captures one LLM
//! request+response *structurally* (`prompt`, `system`, `tools`,
//! `reply`, `tool_uses`) instead of the v2 opaque-bytes approximation.
//! v3 is purely additive on top of v2: every v2 event variant is
//! preserved verbatim, only new variants and new optional fields are
//! introduced.
//!
//! v2 readers that don't know about `LlmCall` will get a serde error
//! on that one variant; the in-tree decoder upgrades cleanly because
//! the v3 binary always lists `LlmCall` in its `TraceEvent` enum. A
//! v2 trace (no `LlmCall` events) still deserialises fine under v3:
//! the [`decode`] entry point accepts `version <= TRACE_WIRE_VERSION`
//! and treats the missing-LLM-call case as `iter_llm_calls() -> empty`.
//!
//! ## Format choice
//!
//! Postcard is the eventual on-disk codec for its compact varint
//! encoding; the v0.17 codec is JSON-after-magic so the file is
//! human-readable for debugging. The wire types themselves are
//! codec-agnostic — every field implements `Serialize` + `Deserialize`.
//!
//! ## Privacy
//!
//! [`TraceEvent::MessageSent`] / [`TraceEvent::IoRead`] capture raw
//! bytes. Recording is opt-in via the `MTY_RECORD_TRACE` environment
//! variable (see [`super::recorder`]). Documented in
//! `docs/reference/cli/mty-replay.md`.

use serde::{Deserialize, Serialize};

/// Current wire-format version.
///
/// * **1** — v0.17/v0.18: `MessageSent.payload` is `Vec<u8>` of the
///   Debug-formatted args.
/// * **2** — v0.19: `MessageSent.payload` is [`ReplayPayload`], either
///   structural [`ReplayValue`]s (byte-identical) or [`Vec<u8>`]
///   Opaque bytes (legacy / cheap-record).
/// * **3** — v0.29: adds [`TraceEvent::LlmCall`] capturing one LLM
///   turn structurally (prompt + system + tools + reply text +
///   tool_uses). Additive: v2 traces still decode cleanly because the
///   new variant only fires when the recorder explicitly captures an
///   LLM call.
pub const TRACE_WIRE_VERSION: u32 = 3;

/// Magic bytes prefix for trace files. Lets `mty replay` reject random
/// binaries before attempting full decode.
pub const TRACE_MAGIC: &[u8; 8] = b"MTYTRACE";

/// Top-level container serialized to disk.
///
/// Layout on disk (JSON-after-magic):
/// ```text
/// [8 bytes MAGIC] [JSON-encoded TraceFile]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceFile {
    /// Wire-format version. See [`TRACE_WIRE_VERSION`].
    pub version: u32,
    /// Unix milliseconds when the recording started.
    pub created_at_ms: u64,
    /// Seed used to derive every deterministic-rand stream in the
    /// recorded run. Replay re-seeds from this so RNG draws line up.
    pub runtime_seed: u64,
    /// Number of worker threads in the recorded run. Replay normally
    /// re-runs with `workers(1)` for determinism, but the field is
    /// preserved for diagnostics.
    pub worker_count: u32,
    /// Ordered event log. Append-only during recording; iterated in
    /// order during replay.
    pub events: Vec<TraceEvent>,
}

/// One captured runtime event. Variants are append-only — never
/// rename, repurpose, or reorder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TraceEvent {
    /// A new agent was spawned.
    Spawn {
        agent_id: u64,
        agent_type: String,
        /// Optional supervisor parent (the spawning agent), if any.
        #[serde(default)]
        supervisor: Option<u64>,
    },
    /// A message was placed on the target's mailbox.
    MessageSent {
        from: u64,
        to: u64,
        /// Protocol message name (e.g. `"Ping"`).
        msg: String,
        /// Structural or opaque payload. See [`ReplayPayload`].
        #[serde(default)]
        payload: ReplayPayload,
    },
    /// A message was dispatched to its handler.
    MessageHandled {
        agent: u64,
        /// Sequence number within this agent's handled stream. Used by
        /// the replayer to detect skipped messages.
        msg_idx: u64,
        msg: String,
        /// Wall-clock microseconds the handler took. Replay uses this
        /// to advance the logical clock.
        elapsed_us: u64,
    },
    /// External IO read (file / network / stdin). Bytes are exactly
    /// what the runtime returned to user code.
    IoRead {
        agent: u64,
        /// Logical source label (`"file:/etc/foo"`, `"net:1.2.3.4"`).
        source: String,
        bytes: Vec<u8>,
    },
    /// `std.time.now_ms` (or equivalent) read.
    ClockRead { agent: u64, value_ms: u64 },
    /// `std.random.fill` (or equivalent) read.
    RandomRead { agent: u64, bytes: Vec<u8> },
    /// Agent's budget tripped during the run. Carries the human-
    /// readable reason for replayer/debugger display.
    BudgetExhausted { agent: u64, reason: String },
    /// Agent exited normally (terminated). Recorded so the replayer
    /// can step through full lifecycles.
    Exit {
        agent: u64,
        /// Free-form reason string (`"normal"`, `"trap:MT5020"`).
        reason: String,
    },
    /// v0.29 wire-v3: a single LLM turn captured structurally.
    ///
    /// Where `MessageSent` records the *agent* mailbox, `LlmCall`
    /// records the *LLM* request/response on the line below it — the
    /// prompt the agent gave the model, the model's reply text, and
    /// any tool-use blocks the assistant emitted. This lets
    /// `std.eval` reconstruct the recorded turn structurally and
    /// dispatch only the LLM half against a fresh provider.
    ///
    /// All fields are owned strings / vecs for serialization
    /// portability. Optional fields use `#[serde(default)]` so v2
    /// readers can still load a v3 trace with a partial `LlmCall`
    /// payload — and so a future v4 reader can drop new optional
    /// fields without breaking v3 writers.
    LlmCall {
        /// Agent that issued the call. `0` when the call came from the
        /// process bootstrap (e.g. a CLI eval), not from a spawned
        /// agent.
        #[serde(default)]
        agent: u64,
        /// Monotonic per-trace turn id. Stable across recordings so
        /// `mty replay --diff --turn <id>` can address one turn
        /// uniquely.
        turn_id: u64,
        /// User-facing prompt (the assistant `Message` content).
        prompt: String,
        /// System prompt prefix the agent paired with the call.
        /// `None` when the agent didn't set one.
        #[serde(default)]
        system: Option<String>,
        /// Tool names the agent advertised at call time. Order is
        /// stable across the recorded run.
        #[serde(default)]
        tools: Vec<String>,
        /// Plain-text reply the model returned (assistant content
        /// block, concatenated when streamed).
        #[serde(default)]
        reply: String,
        /// Tool-use blocks the assistant emitted in this turn.
        /// Per-block schema mirrors the v0.21 `LlmReply` shape.
        #[serde(default)]
        tool_uses: Vec<LlmToolUse>,
        /// Optional cost in cents reported by the provider. `0` when
        /// the provider didn't surface a cost.
        #[serde(default)]
        cost_cents: u64,
    },
}

impl TraceEvent {
    /// Stable short name for human-readable summary output.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Spawn { .. } => "spawn",
            Self::MessageSent { .. } => "message_sent",
            Self::MessageHandled { .. } => "message_handled",
            Self::IoRead { .. } => "io_read",
            Self::ClockRead { .. } => "clock_read",
            Self::RandomRead { .. } => "random_read",
            Self::BudgetExhausted { .. } => "budget_exhausted",
            Self::Exit { .. } => "exit",
            Self::LlmCall { .. } => "llm_call",
        }
    }

    /// Agent that owns the event, when meaningful. Used by the
    /// summary command + replayer for grouping. `MessageSent` returns
    /// the recipient (the agent whose mailbox grew).
    pub fn agent(&self) -> Option<u64> {
        match self {
            Self::Spawn { agent_id, .. } => Some(*agent_id),
            Self::MessageSent { to, .. } => Some(*to),
            Self::MessageHandled { agent, .. }
            | Self::IoRead { agent, .. }
            | Self::ClockRead { agent, .. }
            | Self::RandomRead { agent, .. }
            | Self::BudgetExhausted { agent, .. }
            | Self::Exit { agent, .. } => Some(*agent),
            Self::LlmCall { agent, .. } => Some(*agent),
        }
    }
}

/// One tool-use block from an assistant turn — the v3 structural
/// counterpart to the streaming `LlmReply::tool_uses` shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmToolUse {
    /// Tool name (`"search_web"`, `"read_file"`, …).
    pub name: String,
    /// Provider-assigned id for cross-referencing follow-up
    /// tool-result messages. Empty when the provider didn't supply
    /// one (e.g. the Bedrock adapter).
    #[serde(default)]
    pub id: String,
    /// Raw JSON-encoded arguments the assistant supplied. Stored as
    /// a string so the trace doesn't depend on
    /// `serde_json::Value`'s wire shape — readers parse it on demand.
    #[serde(default)]
    pub input_json: String,
}

/// v0.29 hook: a typed projection of one `LlmCall` event, returned by
/// [`TraceFile::iter_llm_calls`].
///
/// All fields are borrowed from the underlying [`TraceEvent::LlmCall`]
/// arm so iteration is allocation-free. `std.eval` uses this to drive
/// "rerun only the LLM turns against a fresh provider" without
/// spinning a full [`super::super::runtime::Runtime`].
#[derive(Debug, Clone, Copy)]
pub struct LlmCallRef<'a> {
    pub turn_id: u64,
    pub agent: u64,
    pub prompt: &'a str,
    pub system: Option<&'a str>,
    pub tools: &'a [String],
    pub reply: &'a str,
    pub tool_uses: &'a [LlmToolUse],
    pub cost_cents: u64,
}

/// Payload attached to a [`TraceEvent::MessageSent`].
///
/// v0.19 introduces this enum to let the recorder decide per-call
/// whether to capture the cheap Debug-formatted bytes (`Opaque`) or
/// the full structural payload tree (`Values`, byte-identical).
///
/// The runtime hot path emits `Opaque` (because it doesn't pay the
/// `Value` walk when no recorder is installed). The
/// [`super::Replayer`] / [`super::replay_driver`] surface records
/// `Values` when driving a re-execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReplayPayload {
    /// Opaque byte payload. v0.18 default — typically the bytes of
    /// `format!("{:?}", args)`. Cannot be re-fed into a fresh runtime
    /// without parsing; comparison is byte-equality.
    Opaque(Vec<u8>),
    /// Structural payload — the recorded args as a tree of
    /// [`ReplayValue`]s. Byte-identical replay re-constructs runtime
    /// `Value`s from this.
    Values(Vec<ReplayValue>),
}

impl Default for ReplayPayload {
    fn default() -> Self {
        ReplayPayload::Opaque(Vec::new())
    }
}

impl ReplayPayload {
    /// Build an `Opaque` payload from a byte buffer. The v0.18 hot-
    /// path callers feed in `format!("{:?}", args).into_bytes()`.
    pub fn from_bytes(b: Vec<u8>) -> Self {
        ReplayPayload::Opaque(b)
    }

    /// Build a `Values` payload by structurally encoding the slice.
    /// Variants the codec can't represent are folded to
    /// [`ReplayValue::Opaque`] (the lossy fallback). Generic over any
    /// type that knows how to render itself as a [`ReplayValue`]
    /// (interpreter `Value` impls this in `super::from_runtime_value`).
    pub fn from_values<T: RuntimeValueLike>(values: &[T]) -> Self {
        ReplayPayload::Values(values.iter().map(|v| v.to_replay_value()).collect())
    }

    /// `true` if both payloads compare byte-for-byte equal. Used by
    /// the [`super::replay_driver`] in byte-identical assertion mode.
    pub fn bytes_equal(&self, other: &ReplayPayload) -> bool {
        self == other
    }

    /// View as opaque bytes if the payload is `Opaque`. Returns
    /// `None` for `Values`.
    pub fn as_opaque(&self) -> Option<&[u8]> {
        match self {
            ReplayPayload::Opaque(b) => Some(b),
            _ => None,
        }
    }

    /// View as values if the payload is `Values`.
    pub fn as_values(&self) -> Option<&[ReplayValue]> {
        match self {
            ReplayPayload::Values(v) => Some(v),
            _ => None,
        }
    }
}

/// Structural mirror of the SIR interpreter `Value` enum, encoded so a
/// trace can be re-executed byte-identically. Variants that hold
/// runtime-internal references (e.g. live `Ref`, `Fn`, `Agent`, `Cap`)
/// are folded into [`ReplayValue::Opaque`] — they're not portable
/// across processes and the byte-identical contract is **structural
/// equality of the recorded shape**, not pointer identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReplayValue {
    /// `()`.
    Unit,
    /// Boolean.
    Bool(bool),
    /// Integer. We always store as i128 to round-trip every numeric
    /// width the interpreter supports. The `kind` string is the
    /// textual rendering of the original `IntKind` so a v0.20 reader
    /// can re-bind it without depending on `mty_types`.
    Int { value: i128, kind: String },
    /// Float. Stored as bit-pattern to round-trip NaN payloads + sign-
    /// of-zero exactly. `kind` mirrors `Int.kind`.
    Float { bits: u64, kind: String },
    /// Owned string.
    Str(String),
    /// Single Unicode scalar.
    Char(char),
    /// Duration in milliseconds.
    Duration(u64),
    /// Size in bytes.
    Size(u64),
    /// Tuple — fixed-arity heterogeneous payload.
    Tuple(Vec<ReplayValue>),
    /// Array / list — same shape as tuple but ordered list semantics.
    Array(Vec<ReplayValue>),
    /// Struct / record. `adt` is the integer ADT id; `fields` are the
    /// child values in declaration order.
    Record { adt: u64, fields: Vec<ReplayValue> },
    /// Enum / variant. `adt` + `variant` is the discriminant; `payload`
    /// is the variant's argument list.
    Variant {
        adt: u64,
        variant: usize,
        payload: Vec<ReplayValue>,
    },
    /// Lossy fallback — the runtime value couldn't be represented
    /// structurally, so the codec stored its Debug rendering. Byte-
    /// identical replay still works because both record + replay
    /// produce the same Debug bytes.
    Opaque(String),
}

/// Tiny adapter trait so [`ReplayPayload::from_values`] can be called
/// with anything that knows how to render itself into a
/// [`ReplayValue`]. The interpreter `Value` impls this in
/// [`super::value_codec`].
pub trait RuntimeValueLike {
    fn to_replay_value(&self) -> ReplayValue;
}

/// Aggregate summary returned by [`TraceFile::summary`] — used by the
/// CLI for the default "no flags" mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceSummary {
    pub version: u32,
    pub created_at_ms: u64,
    pub runtime_seed: u64,
    pub worker_count: u32,
    pub event_count: usize,
    pub agent_count: usize,
    pub spawn_count: usize,
    pub message_sent_count: usize,
    pub message_handled_count: usize,
    pub io_read_count: usize,
    pub clock_read_count: usize,
    pub random_read_count: usize,
    pub budget_exhausted_count: usize,
    pub exit_count: usize,
    /// Total elapsed microseconds across recorded handler dispatches.
    pub total_handler_elapsed_us: u64,
    /// v0.29: number of [`TraceEvent::LlmCall`] events in the trace.
    /// `0` for v2 traces (no LLM events were recorded).
    #[serde(default)]
    pub llm_call_count: usize,
}

impl TraceFile {
    /// Build a fresh, empty trace anchored to the given seed + clock.
    pub fn new(runtime_seed: u64, created_at_ms: u64, worker_count: u32) -> Self {
        Self {
            version: TRACE_WIRE_VERSION,
            created_at_ms,
            runtime_seed,
            worker_count,
            events: Vec::new(),
        }
    }

    /// Compute an aggregate summary in one pass. Used by `mty replay`
    /// in its default validate-and-summarize mode.
    pub fn summary(&self) -> TraceSummary {
        use std::collections::BTreeSet;
        let mut spawn = 0;
        let mut sent = 0;
        let mut handled = 0;
        let mut io = 0;
        let mut clock = 0;
        let mut random = 0;
        let mut budget = 0;
        let mut exit = 0;
        let mut llm = 0;
        let mut elapsed_us = 0_u64;
        let mut agents: BTreeSet<u64> = BTreeSet::new();
        for e in &self.events {
            if let Some(id) = e.agent() {
                agents.insert(id);
            }
            match e {
                TraceEvent::Spawn { .. } => spawn += 1,
                TraceEvent::MessageSent { .. } => sent += 1,
                TraceEvent::MessageHandled { elapsed_us: us, .. } => {
                    handled += 1;
                    elapsed_us = elapsed_us.saturating_add(*us);
                }
                TraceEvent::IoRead { .. } => io += 1,
                TraceEvent::ClockRead { .. } => clock += 1,
                TraceEvent::RandomRead { .. } => random += 1,
                TraceEvent::BudgetExhausted { .. } => budget += 1,
                TraceEvent::Exit { .. } => exit += 1,
                TraceEvent::LlmCall { .. } => llm += 1,
            }
        }
        TraceSummary {
            version: self.version,
            created_at_ms: self.created_at_ms,
            runtime_seed: self.runtime_seed,
            worker_count: self.worker_count,
            event_count: self.events.len(),
            agent_count: agents.len(),
            spawn_count: spawn,
            message_sent_count: sent,
            message_handled_count: handled,
            io_read_count: io,
            clock_read_count: clock,
            random_read_count: random,
            budget_exhausted_count: budget,
            exit_count: exit,
            total_handler_elapsed_us: elapsed_us,
            llm_call_count: llm,
        }
    }

    /// v0.29 hook (item #2 of the v0.29 backlog): borrowed iterator
    /// over every [`TraceEvent::LlmCall`] event in the trace.
    ///
    /// `std.eval` calls this in its native fast-path to rerun only
    /// the recorded LLM turns against a fresh
    /// [`crate::swarm::Member`]-like provider, without spinning up a
    /// fresh [`super::super::runtime::Runtime`]. v2 traces (no LLM
    /// events) yield an empty iterator.
    pub fn iter_llm_calls(&self) -> impl Iterator<Item = LlmCallRef<'_>> + '_ {
        self.events.iter().filter_map(|e| match e {
            TraceEvent::LlmCall {
                turn_id,
                agent,
                prompt,
                system,
                tools,
                reply,
                tool_uses,
                cost_cents,
            } => Some(LlmCallRef {
                turn_id: *turn_id,
                agent: *agent,
                prompt: prompt.as_str(),
                system: system.as_deref(),
                tools: tools.as_slice(),
                reply: reply.as_str(),
                tool_uses: tool_uses.as_slice(),
                cost_cents: *cost_cents,
            }),
            _ => None,
        })
    }

    /// Find one recorded LLM turn by its `turn_id`. Used by
    /// `mty replay --diff --turn <id>` to address a single turn from
    /// the eval driver's divergence report.
    pub fn llm_call_by_turn(&self, turn_id: u64) -> Option<LlmCallRef<'_>> {
        self.iter_llm_calls().find(|c| c.turn_id == turn_id)
    }
}

// -----------------------------------------------------------------------------
// v1 backwards-read shim
// -----------------------------------------------------------------------------

/// V1-shape `TraceEvent::MessageSent` — `payload` was a flat `Vec<u8>`.
/// Used only by the decoder to lift v1 traces into the v2 shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub(crate) struct V1TraceFile {
    pub version: u32,
    pub created_at_ms: u64,
    pub runtime_seed: u64,
    pub worker_count: u32,
    pub events: Vec<V1TraceEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum V1TraceEvent {
    Spawn {
        agent_id: u64,
        agent_type: String,
        #[serde(default)]
        supervisor: Option<u64>,
    },
    MessageSent {
        from: u64,
        to: u64,
        msg: String,
        #[serde(default)]
        payload: Vec<u8>,
    },
    MessageHandled {
        agent: u64,
        msg_idx: u64,
        msg: String,
        elapsed_us: u64,
    },
    IoRead {
        agent: u64,
        source: String,
        bytes: Vec<u8>,
    },
    ClockRead {
        agent: u64,
        value_ms: u64,
    },
    RandomRead {
        agent: u64,
        bytes: Vec<u8>,
    },
    BudgetExhausted {
        agent: u64,
        reason: String,
    },
    Exit {
        agent: u64,
        reason: String,
    },
}

impl V1TraceFile {
    /// Lift a v1 trace into the current shape — `MessageSent.payload`
    /// becomes `ReplayPayload::Opaque(bytes)`. All other events pass
    /// through unchanged. The `version` field is rewritten to v1 so
    /// callers can branch on "this came from disk as v1".
    pub(crate) fn into_v2(self) -> TraceFile {
        let events = self
            .events
            .into_iter()
            .map(|e| match e {
                V1TraceEvent::Spawn {
                    agent_id,
                    agent_type,
                    supervisor,
                } => TraceEvent::Spawn {
                    agent_id,
                    agent_type,
                    supervisor,
                },
                V1TraceEvent::MessageSent {
                    from,
                    to,
                    msg,
                    payload,
                } => TraceEvent::MessageSent {
                    from,
                    to,
                    msg,
                    payload: ReplayPayload::Opaque(payload),
                },
                V1TraceEvent::MessageHandled {
                    agent,
                    msg_idx,
                    msg,
                    elapsed_us,
                } => TraceEvent::MessageHandled {
                    agent,
                    msg_idx,
                    msg,
                    elapsed_us,
                },
                V1TraceEvent::IoRead {
                    agent,
                    source,
                    bytes,
                } => TraceEvent::IoRead {
                    agent,
                    source,
                    bytes,
                },
                V1TraceEvent::ClockRead { agent, value_ms } => {
                    TraceEvent::ClockRead { agent, value_ms }
                }
                V1TraceEvent::RandomRead { agent, bytes } => {
                    TraceEvent::RandomRead { agent, bytes }
                }
                V1TraceEvent::BudgetExhausted { agent, reason } => {
                    TraceEvent::BudgetExhausted { agent, reason }
                }
                V1TraceEvent::Exit { agent, reason } => TraceEvent::Exit { agent, reason },
            })
            .collect();
        TraceFile {
            // Preserve the source version on the in-memory struct so
            // tests / `mty replay` can surface "this was a v1 trace".
            version: self.version,
            created_at_ms: self.created_at_ms,
            runtime_seed: self.runtime_seed,
            worker_count: self.worker_count,
            events,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_version_is_three() {
        // v0.29 bumped wire version 2 → 3 to introduce the
        // `TraceEvent::LlmCall` structural event.
        assert_eq!(TRACE_WIRE_VERSION, 3);
    }

    #[test]
    fn magic_is_eight_bytes() {
        assert_eq!(TRACE_MAGIC.len(), 8);
    }

    #[test]
    fn empty_summary() {
        let t = TraceFile::new(42, 1_000, 4);
        let s = t.summary();
        assert_eq!(s.event_count, 0);
        assert_eq!(s.agent_count, 0);
        assert_eq!(s.runtime_seed, 42);
        assert_eq!(s.worker_count, 4);
    }

    #[test]
    fn summary_groups_by_agent_and_kind() {
        let mut t = TraceFile::new(0, 0, 1);
        t.events.push(TraceEvent::Spawn {
            agent_id: 1,
            agent_type: "Echo".into(),
            supervisor: None,
        });
        t.events.push(TraceEvent::Spawn {
            agent_id: 2,
            agent_type: "Echo".into(),
            supervisor: Some(1),
        });
        t.events.push(TraceEvent::MessageSent {
            from: 1,
            to: 2,
            msg: "Ping".into(),
            payload: ReplayPayload::default(),
        });
        t.events.push(TraceEvent::MessageHandled {
            agent: 2,
            msg_idx: 0,
            msg: "Ping".into(),
            elapsed_us: 250,
        });
        t.events.push(TraceEvent::Exit {
            agent: 2,
            reason: "normal".into(),
        });
        let s = t.summary();
        assert_eq!(s.event_count, 5);
        assert_eq!(s.agent_count, 2);
        assert_eq!(s.spawn_count, 2);
        assert_eq!(s.message_sent_count, 1);
        assert_eq!(s.message_handled_count, 1);
        assert_eq!(s.exit_count, 1);
        assert_eq!(s.total_handler_elapsed_us, 250);
    }

    #[test]
    fn event_kind_matches_variant() {
        let e = TraceEvent::ClockRead {
            agent: 7,
            value_ms: 12,
        };
        assert_eq!(e.kind(), "clock_read");
        assert_eq!(e.agent(), Some(7));
    }

    #[test]
    fn message_sent_agent_is_recipient() {
        let e = TraceEvent::MessageSent {
            from: 1,
            to: 99,
            msg: "X".into(),
            payload: ReplayPayload::default(),
        };
        // We want grouping by mailbox-owner.
        assert_eq!(e.agent(), Some(99));
    }

    #[test]
    fn replay_payload_opaque_round_trip() {
        let p = ReplayPayload::Opaque(b"hello".to_vec());
        let js = serde_json::to_string(&p).unwrap();
        let back: ReplayPayload = serde_json::from_str(&js).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn replay_payload_values_round_trip() {
        let p = ReplayPayload::Values(vec![
            ReplayValue::Str("hi".into()),
            ReplayValue::Int {
                value: 7,
                kind: "I64".into(),
            },
        ]);
        let js = serde_json::to_string(&p).unwrap();
        let back: ReplayPayload = serde_json::from_str(&js).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn replay_value_variants_serialize() {
        // Spot-check that every public variant survives a JSON round
        // trip. This is the v0.20 forward-compat invariant.
        let cases = vec![
            ReplayValue::Unit,
            ReplayValue::Bool(true),
            ReplayValue::Int {
                value: -42,
                kind: "I64".into(),
            },
            ReplayValue::Float {
                bits: 0x4008_0000_0000_0000,
                kind: "F64".into(),
            },
            ReplayValue::Str("x".into()),
            ReplayValue::Char('z'),
            ReplayValue::Duration(123),
            ReplayValue::Size(456),
            ReplayValue::Tuple(vec![ReplayValue::Unit]),
            ReplayValue::Array(vec![ReplayValue::Bool(false)]),
            ReplayValue::Record {
                adt: 0,
                fields: vec![ReplayValue::Unit],
            },
            ReplayValue::Variant {
                adt: 0,
                variant: 1,
                payload: vec![ReplayValue::Str("p".into())],
            },
            ReplayValue::Opaque("debug-shape".into()),
        ];
        for c in cases {
            let js = serde_json::to_string(&c).unwrap();
            let back: ReplayValue = serde_json::from_str(&js).unwrap();
            assert_eq!(c, back);
        }
    }

    // -------------------------------------------------------------------------
    // v0.29 wire-v3 tests — structural LLM-call events
    // -------------------------------------------------------------------------

    fn sample_llm_call(turn_id: u64) -> TraceEvent {
        TraceEvent::LlmCall {
            agent: 7,
            turn_id,
            prompt: format!("turn-{turn_id} prompt"),
            system: Some("you are a helpful assistant".into()),
            tools: vec!["search_web".into(), "read_file".into()],
            reply: format!("turn-{turn_id} reply"),
            tool_uses: vec![LlmToolUse {
                name: "search_web".into(),
                id: "tu-1".into(),
                input_json: "{\"q\":\"hi\"}".into(),
            }],
            cost_cents: 3,
        }
    }

    #[test]
    fn llm_call_round_trips_through_json() {
        let mut t = TraceFile::new(0, 0, 1);
        t.events.push(sample_llm_call(1));
        t.events.push(sample_llm_call(2));
        let js = serde_json::to_string(&t).unwrap();
        let back: TraceFile = serde_json::from_str(&js).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn iter_llm_calls_returns_only_llm_events() {
        let mut t = TraceFile::new(0, 0, 1);
        t.events.push(TraceEvent::Spawn {
            agent_id: 7,
            agent_type: "Eval".into(),
            supervisor: None,
        });
        t.events.push(sample_llm_call(10));
        t.events.push(TraceEvent::ClockRead {
            agent: 7,
            value_ms: 1,
        });
        t.events.push(sample_llm_call(11));
        let calls: Vec<_> = t.iter_llm_calls().collect();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].turn_id, 10);
        assert_eq!(calls[1].turn_id, 11);
        assert_eq!(calls[0].prompt, "turn-10 prompt");
        assert_eq!(calls[0].reply, "turn-10 reply");
        assert_eq!(calls[0].system, Some("you are a helpful assistant"));
        assert_eq!(calls[0].tools.len(), 2);
        assert_eq!(calls[0].tool_uses.len(), 1);
        assert_eq!(calls[0].tool_uses[0].name, "search_web");
    }

    #[test]
    fn llm_call_by_turn_finds_recorded_turn() {
        let mut t = TraceFile::new(0, 0, 1);
        t.events.push(sample_llm_call(1));
        t.events.push(sample_llm_call(42));
        t.events.push(sample_llm_call(7));
        let got = t.llm_call_by_turn(42).expect("turn 42 exists");
        assert_eq!(got.turn_id, 42);
        assert_eq!(got.prompt, "turn-42 prompt");
        assert!(t.llm_call_by_turn(999).is_none());
    }

    #[test]
    fn summary_counts_llm_calls() {
        let mut t = TraceFile::new(0, 0, 1);
        t.events.push(TraceEvent::Spawn {
            agent_id: 1,
            agent_type: "Eval".into(),
            supervisor: None,
        });
        t.events.push(sample_llm_call(1));
        t.events.push(sample_llm_call(2));
        t.events.push(sample_llm_call(3));
        let s = t.summary();
        assert_eq!(s.llm_call_count, 3);
        assert_eq!(s.event_count, 4);
        // The agent set folds in the LLM-call agent id.
        assert!(s.agent_count >= 1);
    }

    #[test]
    fn llm_call_kind_and_agent_helpers() {
        let e = sample_llm_call(99);
        assert_eq!(e.kind(), "llm_call");
        assert_eq!(e.agent(), Some(7));
    }

    #[test]
    fn llm_call_with_only_required_fields_round_trips() {
        // Every optional field at its serde default — exercising the
        // forward-compat path where a future writer may drop the
        // optional fields entirely.
        let e = TraceEvent::LlmCall {
            agent: 0,
            turn_id: 1,
            prompt: "p".into(),
            system: None,
            tools: vec![],
            reply: String::new(),
            tool_uses: vec![],
            cost_cents: 0,
        };
        let js = serde_json::to_string(&e).unwrap();
        let back: TraceEvent = serde_json::from_str(&js).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn v2_trace_with_no_llm_events_still_works_under_v3() {
        // Construct a v2-shape trace (no LLM events) — under wire-v3 it
        // must still deserialise, just with `iter_llm_calls()` empty.
        let mut t = TraceFile::new(0, 0, 1);
        t.version = 2;
        t.events.push(TraceEvent::Spawn {
            agent_id: 1,
            agent_type: "Echo".into(),
            supervisor: None,
        });
        t.events.push(TraceEvent::MessageSent {
            from: 0,
            to: 1,
            msg: "Ping".into(),
            payload: ReplayPayload::default(),
        });
        let js = serde_json::to_string(&t).unwrap();
        let back: TraceFile = serde_json::from_str(&js).unwrap();
        assert_eq!(back.version, 2);
        assert_eq!(back.iter_llm_calls().count(), 0);
        assert_eq!(back.summary().llm_call_count, 0);
    }

    #[test]
    fn v1_into_v2_lifts_opaque_payload() {
        let v1 = V1TraceFile {
            version: 1,
            created_at_ms: 100,
            runtime_seed: 7,
            worker_count: 1,
            events: vec![V1TraceEvent::MessageSent {
                from: 0,
                to: 1,
                msg: "Ping".into(),
                payload: b"old-bytes".to_vec(),
            }],
        };
        let v2 = v1.into_v2();
        assert_eq!(v2.version, 1, "preserves source-disk version field");
        assert_eq!(v2.events.len(), 1);
        match &v2.events[0] {
            TraceEvent::MessageSent { payload, .. } => match payload {
                ReplayPayload::Opaque(b) => assert_eq!(b, b"old-bytes"),
                _ => panic!("expected Opaque lift, got {payload:?}"),
            },
            other => panic!("expected MessageSent, got {other:?}"),
        }
    }
}
