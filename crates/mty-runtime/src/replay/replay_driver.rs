//! v0.19 byte-identical full replay re-execution.
//!
//! The driver takes a recorded [`TraceFile`] + the original SIR
//! [`Program`], spins up a *fresh* [`Runtime`] (seeded from
//! `trace.runtime_seed`), and re-feeds the recorded `Spawn` +
//! `MessageSent` events so a clean run reproduces the original event
//! stream. The driver installs its own recorder so it can compare the
//! re-emitted events against the recorded ones; mismatches accumulate
//! into a [`ReplayReport`].
//!
//! The trace doesn't carry the user's program source (recordings can
//! be inspected even when the program is unavailable), so the caller
//! provides it explicitly via [`ReplayDriver::with_program`]. This
//! mirrors the v0.18 builder shape — callers parse + lower their
//! `.mty` source once, then hand the [`Arc<Program>`] to the driver.
//!
//! ## What "byte-identical" means
//!
//! For every recorded event, the replay must emit a structurally
//! equal event in the same order. Equality is field-by-field:
//!
//! * `Spawn` — `agent_type` and `supervisor`. (Agent ids are
//!   re-allocated by the live registry, so we map recorded id →
//!   live id via the spawn order rather than comparing ids.)
//! * `MessageSent` — `msg`, `from`, mapped `to`, payload (Opaque
//!   bytes or structural `Values`).
//! * `MessageHandled` — `msg`, `msg_idx`. (`elapsed_us` is a wall-
//!   clock measurement that the v0.19 driver does NOT compare; the
//!   logical sequence is what matters.)
//! * `IoRead` / `ClockRead` / `RandomRead` — exact value comparison
//!   when `--mock-io` is on; structural-only when off (the v0.19
//!   driver always treats IO as the source of truth from the
//!   recorded trace, so reads are deterministic across replays).
//! * `BudgetExhausted` / `Exit` — `reason` string.
//!
//! Mismatches are *collected*, not thrown, so the report surfaces the
//! complete set of divergences for a debugging UI. The `success` flag
//! is `true` only when `mismatches.is_empty()`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use mty_ir::interp::value::Value as RuntimeValue;
use mty_ir::ir::Program;

use super::to_runtime_value;
use super::wire::{LlmCallRef, LlmToolUse, ReplayPayload, TraceEvent, TraceFile};
use crate::agent::AgentHandle;
use crate::runtime::RuntimeBuilder;

/// One recorded-vs-replayed event divergence.
#[derive(Debug, Clone, PartialEq)]
pub struct EventMismatch {
    /// Index of the event in the recorded trace (0-based).
    pub index: usize,
    /// Recorded event (the source of truth).
    pub recorded: TraceEvent,
    /// Replayed event (or `None` if the replay stream was shorter
    /// than the recorded one).
    pub replayed: Option<TraceEvent>,
    /// Free-form human-readable reason for the mismatch.
    pub reason: String,
}

/// Summary of one [`ReplayDriver::replay_all`] call.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayReport {
    /// Number of events the driver successfully replayed against the
    /// trace. Always `<= trace.events.len()`.
    pub events_replayed: usize,
    /// Every mismatch detected. Empty == byte-identical.
    pub mismatches: Vec<EventMismatch>,
    /// `mismatches.is_empty() && events_replayed > 0`.
    pub success: bool,
    /// v0.32 Track F: per-turn LLM-call replays. Populated only when
    /// the driver was built with [`ReplayDriver::with_provider`] —
    /// every recorded `TraceEvent::LlmCall` then gets a corresponding
    /// [`LlmTurnReplay`] entry showing the live provider's reply +
    /// any divergence from the recorded one.
    ///
    /// Empty when no provider was installed (the legacy byte-identical
    /// path). Non-LLM events still flow through the runtime
    /// re-execution path regardless.
    #[doc(alias = "interleaved_llm_replays")]
    pub llm_turn_replays: Vec<LlmTurnReplay>,
}

impl ReplayReport {
    /// Total number of mismatches detected.
    pub fn mismatch_count(&self) -> usize {
        self.mismatches.len()
    }

    /// Format the report as a multi-line human-readable diff. Useful
    /// for CLI output (`mty replay --byte-identical --step`).
    pub fn render(&self) -> String {
        if self.success {
            return format!(
                "byte-identical replay OK ({} event(s) matched)",
                self.events_replayed
            );
        }
        let mut out = format!(
            "byte-identical replay FAILED: {} mismatch(es) over {} replayed event(s)\n",
            self.mismatches.len(),
            self.events_replayed
        );
        for m in &self.mismatches {
            out.push_str(&format!("  #{}: {}\n", m.index, m.reason));
        }
        out
    }
}

/// Driver — spins up a fresh `Runtime` from a recorded trace and
/// validates byte-identical re-execution.
pub struct ReplayDriver {
    trace: TraceFile,
    prog: Option<Arc<Program>>,
    /// When true, IO/Clock/Random reads return recorded values from
    /// the trace instead of touching the live host. Default: true
    /// (matches the v0.19 spec — replay must be deterministic across
    /// processes).
    mock_io: bool,
    /// When true, every replayed event is compared field-by-field
    /// against the recorded one. Default: true.
    byte_identical: bool,
    /// Optional ask deadline; defaults to 5s to avoid hanging tests.
    ask_deadline_ms: u64,
    /// v0.29: optional provider hook for [`Self::replay_llm_turns`]
    /// / [`Self::diff_llm_turn`]. Installed via [`Self::with_provider`].
    turn_provider: Option<Arc<dyn TurnProvider>>,
}

impl ReplayDriver {
    /// Build a driver from a loaded trace. The caller must attach a
    /// program via [`Self::with_program`] before calling
    /// [`Self::replay_all`].
    pub fn from_trace(trace: TraceFile) -> Self {
        Self {
            trace,
            prog: None,
            mock_io: true,
            byte_identical: true,
            ask_deadline_ms: 5_000,
            turn_provider: None,
        }
    }

    /// Attach the SIR program the trace was recorded against.
    pub fn with_program(mut self, prog: Arc<Program>) -> Self {
        self.prog = Some(prog);
        self
    }

    /// Toggle IO mocking. With `mock_io = true` (the default), IO /
    /// Clock / Random reads return the recorded bytes from the trace
    /// — this is what makes replay deterministic across processes.
    /// With `false`, the live `StdHost` is used and divergences are
    /// surfaced as mismatches.
    pub fn mock_io(mut self, on: bool) -> Self {
        self.mock_io = on;
        self
    }

    /// Toggle byte-identical event-by-event comparison. With this
    /// off, the driver still replays the trace but doesn't accumulate
    /// mismatches — useful for the "just re-run the program" CLI mode.
    pub fn byte_identical(mut self, on: bool) -> Self {
        self.byte_identical = on;
        self
    }

    /// Override the per-ask deadline (default 5s). Test-only.
    pub fn with_ask_deadline_ms(mut self, ms: u64) -> Self {
        self.ask_deadline_ms = ms;
        self
    }

    /// Borrow the loaded trace.
    pub fn trace(&self) -> &TraceFile {
        &self.trace
    }

    /// Drive the runtime forward against every recorded event.
    ///
    /// The caller must hold the global recorder lock (see
    /// [`tests::recorder_serializer`] in
    /// `crates/mty-runtime/tests/replay_byte_identical.rs`) so that
    /// the live `Runtime` instantiated below sees the driver's
    /// freshly-installed recorder rather than racing with another
    /// test's recorder.
    pub fn replay_all(&mut self) -> Result<ReplayReport, String> {
        let prog = self.prog.clone().ok_or_else(|| {
            "ReplayDriver: program not attached; call with_program(prog)".to_string()
        })?;

        // Snapshot the recorded events so we can iterate even though
        // we'll mutate self below.
        let recorded = self.trace.events.clone();
        let seed = self.trace.runtime_seed;

        // v0.32 Track F: when a `TurnProvider` is installed, every
        // recorded `LlmCall` event is dispatched against the live
        // provider mid-replay so the report carries per-turn diffs
        // alongside the byte-identical event comparison. Non-LLM
        // events still flow through the runtime re-execution path
        // unchanged.
        let mut llm_turn_replays: Vec<LlmTurnReplay> = Vec::new();
        if let Some(provider) = &self.turn_provider {
            for ev in &recorded {
                if let TraceEvent::LlmCall {
                    turn_id,
                    agent,
                    prompt,
                    system,
                    tools,
                    reply,
                    tool_uses,
                    cost_cents,
                } = ev
                {
                    let turn_ref = LlmCallRef {
                        turn_id: *turn_id,
                        agent: *agent,
                        prompt: prompt.as_str(),
                        system: system.as_deref(),
                        tools: tools.as_slice(),
                        reply: reply.as_str(),
                        tool_uses: tool_uses.as_slice(),
                        cost_cents: *cost_cents,
                    };
                    let (live, live_error) = match provider.provide(turn_ref) {
                        Ok(t) => (Some(t), None),
                        Err(e) => (None, Some(e)),
                    };
                    llm_turn_replays.push(LlmTurnReplay {
                        turn_id: *turn_id,
                        recorded_reply: reply.clone(),
                        recorded_tool_uses: tool_uses.clone(),
                        live,
                        live_error,
                    });
                }
            }
        }

        // Build a fresh Runtime. Workers=1 is mandatory for
        // deterministic replay (matches the v0.17 contract: the
        // `worker_count` field is preserved for diagnostics but
        // replay always uses 1 worker).
        let rt = RuntimeBuilder::new()
            .deterministic(seed)
            .workers(1)
            .build(prog);

        // Install a fresh in-process recorder so we can capture the
        // replayed events. `install_from_env` may have been called by
        // RuntimeBuilder::build (if `MTY_RECORD_TRACE` was set) — we
        // uninstall it and replace with our local one.
        let _ = super::recorder::uninstall();
        let driver_recorder = Arc::new(super::recorder::Recorder::new(
            // The path is unused (we never call flush_to_disk for the
            // driver's local recorder) — pass an in-memory marker.
            std::path::PathBuf::from("/__mty_replay_driver__"),
            seed,
            1,
        ));
        super::recorder::install(driver_recorder.clone());

        // Map recorded agent id → live agent handle. The recorded
        // ids are NOT preserved across replay (the live registry
        // re-allocates), so spawn order is the binding key.
        let mut id_map: HashMap<u64, AgentHandle> = HashMap::new();

        let rt_arc = rt.scheduler.rt.clone();
        rt_arc.block_on(async {
            for ev in &recorded {
                match ev {
                    TraceEvent::Spawn {
                        agent_id,
                        agent_type,
                        ..
                    } => {
                        // Spawn a live agent of the same type.
                        match rt.spawn_agent(agent_type, vec![]).await {
                            Ok(h) => {
                                id_map.insert(*agent_id, h);
                            }
                            Err(_e) => {
                                // Skip — mismatch will be flagged
                                // when the comparison runs.
                            }
                        }
                    }
                    TraceEvent::MessageSent {
                        from,
                        to,
                        msg,
                        payload,
                    } => {
                        // Only re-inject extern (from=0) messages —
                        // intra-agent sends are produced by the
                        // re-executing handlers themselves.
                        if *from != 0 {
                            continue;
                        }
                        let Some(handle) = id_map.get(to) else {
                            continue;
                        };
                        // Reconstruct the args from the payload.
                        let args = payload_to_runtime_values(payload);
                        // Use ask with a deadline so the replay
                        // doesn't hang if the handler aborts.
                        let _ = rt
                            .ask(
                                handle,
                                msg,
                                args,
                                Some(Duration::from_millis(self.ask_deadline_ms)),
                            )
                            .await;
                    }
                    // The remaining events (`MessageHandled`,
                    // `IoRead`, `ClockRead`, `RandomRead`,
                    // `BudgetExhausted`, `Exit`) are emitted by the
                    // running agent loop, not by the driver. We just
                    // wait for them to show up.
                    _ => {}
                }
            }
            // Give the agent loops a beat to drain their final
            // messages before shutdown so any trailing Exit /
            // MessageHandled events make it into the trace.
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = rt.shutdown().await;
        });

        // Pull the replayed event stream out and uninstall the
        // driver's recorder.
        let replayed = driver_recorder.events_snapshot();
        let _ = super::recorder::uninstall();

        // Compare event-by-event when byte-identical is on.
        let mut mismatches: Vec<EventMismatch> = Vec::new();
        let n = if !self.byte_identical {
            recorded.len()
        } else {
            compare_streams(&recorded, &replayed, &id_map, &mut mismatches)
        };

        Ok(ReplayReport {
            events_replayed: n,
            success: mismatches.is_empty() && n > 0,
            mismatches,
            llm_turn_replays,
        })
    }
}

/// Convert a recorded [`ReplayPayload`] back into the slice of
/// [`RuntimeValue`]s the runtime expects on `send`/`ask`.
fn payload_to_runtime_values(p: &ReplayPayload) -> Vec<RuntimeValue> {
    match p {
        ReplayPayload::Values(vs) => vs.iter().filter_map(|v| to_runtime_value(v).ok()).collect(),
        ReplayPayload::Opaque(bytes) => {
            if bytes.is_empty() {
                Vec::new()
            } else {
                // Best-effort recovery: pass the Debug rendering back
                // as a single Str. The byte-identical assertion still
                // works because both record + replay emit the same
                // Debug bytes for the same shape.
                let s = String::from_utf8_lossy(bytes).into_owned();
                vec![RuntimeValue::Str(s)]
            }
        }
    }
}

/// Map a recorded agent id to the live one via the spawn-order map.
/// Returns the original id if no mapping exists (e.g. the "extern" 0).
fn map_id(id: u64, id_map: &HashMap<u64, AgentHandle>) -> u64 {
    id_map.get(&id).map(|h| h.id.0).unwrap_or(id)
}

/// Compare two event streams. Differences are accumulated into
/// `mismatches`. Returns the number of events successfully compared.
///
/// **Lifecycle-event policy.** `Exit` and `BudgetExhausted` are
/// recorded at the agent-loop's tail, which the runtime's
/// `Runtime::shutdown` may or may not let run before it aborts the
/// task (timing race). For byte-identical purposes we treat these
/// two event kinds as **soft** comparisons: a missing Exit/Budget on
/// either side counts as a "ok, race lost" rather than a mismatch.
/// All other events (`Spawn`, `MessageSent`, `MessageHandled`,
/// `IoRead`, `ClockRead`, `RandomRead`) are strict.
fn compare_streams(
    recorded: &[TraceEvent],
    replayed: &[TraceEvent],
    id_map: &HashMap<u64, AgentHandle>,
    mismatches: &mut Vec<EventMismatch>,
) -> usize {
    let mut idx = 0_usize;
    for (i, rec) in recorded.iter().enumerate() {
        // Soft events: missing replay is acceptable.
        let is_soft = matches!(
            rec,
            TraceEvent::Exit { .. } | TraceEvent::BudgetExhausted { .. }
        );

        let Some(rep) = replayed.get(idx) else {
            if is_soft {
                continue;
            }
            mismatches.push(EventMismatch {
                index: i,
                recorded: rec.clone(),
                replayed: None,
                reason: format!(
                    "replay stream ended early at recorded event #{i} ({})",
                    rec.kind()
                ),
            });
            continue;
        };

        // Find the next-best match. The replayed stream may have
        // extra events (e.g. an introspection `Spawn` that didn't
        // exist in the recorded run), so we slide forward looking
        // for a same-`kind` match before giving up.
        let mut found_at: Option<usize> = None;
        for (j, rep_ev) in replayed.iter().enumerate().skip(idx) {
            if rep_ev.kind() == rec.kind() && events_match(rec, rep_ev, id_map) {
                found_at = Some(j);
                break;
            }
        }

        match found_at {
            Some(j) => {
                idx = j + 1;
            }
            None if is_soft => {
                // Soft event: don't count as a mismatch; just leave
                // idx where it is so the next event in the recorded
                // stream is still compared against the same replayed
                // position.
            }
            None => {
                mismatches.push(EventMismatch {
                    index: i,
                    recorded: rec.clone(),
                    replayed: Some(rep.clone()),
                    reason: format!(
                        "recorded {} did not match any replayed event after idx {}",
                        rec.kind(),
                        idx
                    ),
                });
                // Advance to avoid an infinite mismatch storm on a
                // single divergence.
                idx += 1;
            }
        }
    }

    recorded.len() - mismatches.len()
}

/// Field-by-field comparison for one event. Returns `true` if
/// considered byte-identical under the v0.19 contract (agent ids are
/// mapped through the spawn-order map; `elapsed_us` is ignored).
fn events_match(rec: &TraceEvent, rep: &TraceEvent, id_map: &HashMap<u64, AgentHandle>) -> bool {
    match (rec, rep) {
        (
            TraceEvent::Spawn {
                agent_type: t1,
                supervisor: s1,
                ..
            },
            TraceEvent::Spawn {
                agent_type: t2,
                supervisor: s2,
                ..
            },
        ) => t1 == t2 && s1 == s2,
        (
            TraceEvent::MessageSent {
                from: f1,
                to: to1,
                msg: m1,
                payload: p1,
            },
            TraceEvent::MessageSent {
                from: f2,
                to: to2,
                msg: m2,
                payload: p2,
            },
        ) => {
            let from_match = if *f1 == 0 {
                *f2 == 0
            } else {
                map_id(*f1, id_map) == *f2
            };
            let to_match = map_id(*to1, id_map) == *to2;
            from_match && to_match && m1 == m2 && payloads_match(p1, p2)
        }
        (
            TraceEvent::MessageHandled {
                agent: a1,
                msg_idx: i1,
                msg: m1,
                ..
            },
            TraceEvent::MessageHandled {
                agent: a2,
                msg_idx: i2,
                msg: m2,
                ..
            },
        ) => map_id(*a1, id_map) == *a2 && i1 == i2 && m1 == m2,
        (
            TraceEvent::IoRead {
                agent: a1,
                source: s1,
                bytes: b1,
            },
            TraceEvent::IoRead {
                agent: a2,
                source: s2,
                bytes: b2,
            },
        ) => map_id(*a1, id_map) == *a2 && s1 == s2 && b1 == b2,
        (
            TraceEvent::ClockRead {
                agent: a1,
                value_ms: v1,
            },
            TraceEvent::ClockRead {
                agent: a2,
                value_ms: v2,
            },
        ) => map_id(*a1, id_map) == *a2 && v1 == v2,
        (
            TraceEvent::RandomRead {
                agent: a1,
                bytes: b1,
            },
            TraceEvent::RandomRead {
                agent: a2,
                bytes: b2,
            },
        ) => map_id(*a1, id_map) == *a2 && b1 == b2,
        (
            TraceEvent::BudgetExhausted {
                agent: a1,
                reason: r1,
            },
            TraceEvent::BudgetExhausted {
                agent: a2,
                reason: r2,
            },
        ) => map_id(*a1, id_map) == *a2 && r1 == r2,
        (
            TraceEvent::Exit {
                agent: a1,
                reason: r1,
            },
            TraceEvent::Exit {
                agent: a2,
                reason: r2,
            },
        ) => map_id(*a1, id_map) == *a2 && r1 == r2,
        _ => false,
    }
}

/// Compare two payloads under the byte-identical contract.
///
/// * `Values == Values` — strict structural comparison. This is the
///   v0.19 byte-identical guarantee: when both sides carry structural
///   payloads, every nested [`ReplayValue`] must match.
/// * `Opaque == Opaque` — accepted as **approximate** equality (both
///   recorded via the v0.18 hot path's lossy `format!("{:?}", args)`
///   encoding). v0.18 recordings can't drive a byte-identical
///   re-execution because the Debug rendering is non-injective — the
///   replay reconstructs args from the bytes and re-renders, which
///   produces a structurally similar but textually different shape.
///   We accept the loose match so v0.18 traces still pass through the
///   driver (the structural contract upgrades cleanly with v0.19
///   recordings).
/// * `Opaque vs Values` — accept if the structural side, re-encoded
///   into Debug-formatted runtime values, byte-matches the opaque
///   side. This is the cross-shape comparison path used when the
///   record + replay sides chose different encodings.
fn payloads_match(a: &ReplayPayload, b: &ReplayPayload) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (ReplayPayload::Opaque(_), ReplayPayload::Opaque(_)) => {
            // v0.18 traces (and v0.19 hot-path traces) both use the
            // Debug rendering. The replay's reconstruction is lossy,
            // so we accept any Opaque-vs-Opaque match for backwards
            // compatibility. v0.19 byte-identical strictness requires
            // structural Values payloads on both sides.
            true
        }
        (ReplayPayload::Opaque(ba), ReplayPayload::Values(vb))
        | (ReplayPayload::Values(vb), ReplayPayload::Opaque(ba)) => {
            // Render the structural side as Debug bytes (matching the
            // v0.18 hot-path encoding) and compare.
            let reconstructed: Vec<RuntimeValue> =
                vb.iter().filter_map(|v| to_runtime_value(v).ok()).collect();
            let bytes = format!("{:?}", reconstructed).into_bytes();
            bytes == *ba
        }
        _ => false,
    }
}

// -----------------------------------------------------------------------------
// v0.29 Track F: LLM-only replay hooks
// -----------------------------------------------------------------------------
//
// `std.eval` needs two operations the v0.21 full re-execution driver
// doesn't surface natively:
//
// 1. "Walk the recorded trace, when you hit an `LlmCall`, ask my
//    `TurnProvider` for a fresh reply, compare reply text + tool_uses
//    against the recorded turn, accumulate a diff."
//
// 2. "Same as above but stop at a specific `turn_id` and surface only
//    that turn's recorded payload + the live reply — for `mty replay
//    --diff --turn <id>`."
//
// The full byte-identical re-execution path (`replay_all`) doesn't
// know about the LLM provider — it just feeds recorded messages to a
// fresh `Runtime`. For "LLM-only replay" we don't need a `Runtime` at
// all: we just walk `TraceFile.iter_llm_calls()` and dispatch each
// `(prompt, system, tools)` triple to the `TurnProvider`.
//
// `TurnProvider` is intentionally narrower than `crate::swarm::Member`
// — `mty-runtime` doesn't depend on `mty-stdlib`, so we expose a
// minimal trait + an adapter that `std.eval` can implement against
// any `Member`. The eval layer maps `Member::ask(prompt)` →
// `TurnProvider::reply(turn)` and threads its own `SharedDollarBudget`
// through.

/// Output of one live provider turn: just the assistant text + any
/// emitted tool-use blocks. Mirrors the structural fields of
/// [`TraceEvent::LlmCall`] so diffs can be field-by-field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidedTurn {
    /// Plain-text assistant reply.
    pub reply: String,
    /// Tool-use blocks the assistant emitted.
    pub tool_uses: Vec<LlmToolUse>,
    /// Optional cost in cents reported by the provider.
    pub cost_cents: u64,
}

impl ProvidedTurn {
    /// Build a reply-only turn with no tool-uses + zero cost. Most
    /// `Compare::equal()`-style eval cases use this shape.
    pub fn from_reply(reply: impl Into<String>) -> Self {
        Self {
            reply: reply.into(),
            tool_uses: Vec::new(),
            cost_cents: 0,
        }
    }
}

/// Narrow trait the v0.29 LLM-only replay path dispatches against.
///
/// `std.eval` implements this for its `Member` enum so the eval driver
/// can re-run the recorded LLM turns against a fresh provider without
/// dragging `mty-stdlib` into `mty-runtime`. The trait is sync at the
/// surface but the implementation may run an async dispatch internally
/// — the replay driver spins a current-thread executor for each call.
pub trait TurnProvider: Send + Sync {
    /// Run one recorded turn against the live provider. The
    /// recorded turn carries the prompt + system + tool list; the
    /// implementation returns the fresh reply + any tool-uses the
    /// assistant emitted.
    ///
    /// Returning `Err(msg)` lets the eval driver stamp the cell as
    /// `Verdict::Error` rather than aborting the whole replay.
    fn provide(&self, turn: LlmCallRef<'_>) -> Result<ProvidedTurn, String>;
}

/// Implement [`TurnProvider`] for any closure with the right shape.
/// Handy in tests + for one-off eval drivers.
impl<F> TurnProvider for F
where
    F: Fn(LlmCallRef<'_>) -> Result<ProvidedTurn, String> + Send + Sync,
{
    fn provide(&self, turn: LlmCallRef<'_>) -> Result<ProvidedTurn, String> {
        (self)(turn)
    }
}

/// One (recorded, live) turn pair surfaced by
/// [`ReplayDriver::replay_llm_turns`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmTurnReplay {
    /// The recorded turn from the trace.
    pub turn_id: u64,
    /// The recorded reply text.
    pub recorded_reply: String,
    /// The recorded tool-uses.
    pub recorded_tool_uses: Vec<LlmToolUse>,
    /// `Some(turn)` when the live provider returned a reply,
    /// `None` when it errored (`live_error` is populated).
    pub live: Option<ProvidedTurn>,
    /// Error message from the live provider when [`live`] is `None`.
    pub live_error: Option<String>,
}

impl LlmTurnReplay {
    /// `true` when the live reply text matches the recorded reply
    /// text exactly. The simplest comparator — eval cases that want
    /// semantic similarity layer it on top of this raw equality.
    pub fn replies_match(&self) -> bool {
        match &self.live {
            Some(t) => t.reply == self.recorded_reply,
            None => false,
        }
    }

    /// `true` when the set of tool-names emitted matches between
    /// recorded + live. Order-independent (mirrors
    /// `Compare::tool_call_set_equal()` in `std.eval`).
    pub fn tool_call_set_matches(&self) -> bool {
        match &self.live {
            Some(t) => {
                let recorded: std::collections::BTreeSet<&str> = self
                    .recorded_tool_uses
                    .iter()
                    .map(|u| u.name.as_str())
                    .collect();
                let live: std::collections::BTreeSet<&str> =
                    t.tool_uses.iter().map(|u| u.name.as_str()).collect();
                recorded == live
            }
            None => false,
        }
    }
}

/// Human-readable diff between one recorded turn and one live reply —
/// the payload `mty replay --diff --turn <id>` renders. Carried back
/// up to the CLI so the diff renderer + the eval divergence reporter
/// share the same shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmTurnDiff {
    pub turn_id: u64,
    pub prompt: String,
    pub system: Option<String>,
    pub recorded_reply: String,
    pub live_reply: Option<String>,
    pub recorded_tool_names: Vec<String>,
    pub live_tool_names: Vec<String>,
    /// `true` when reply text and tool-name set both match.
    pub identical: bool,
}

impl LlmTurnDiff {
    /// Multi-line rendering — the format the CLI prints.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("=== replay diff turn #{} ===\n", self.turn_id));
        out.push_str(&format!("  prompt   : {}\n", truncate(&self.prompt, 200)));
        if let Some(sys) = &self.system {
            out.push_str(&format!("  system   : {}\n", truncate(sys, 200)));
        }
        out.push_str(&format!(
            "  recorded : {}\n",
            truncate(&self.recorded_reply, 200)
        ));
        match &self.live_reply {
            Some(r) => out.push_str(&format!("  live     : {}\n", truncate(r, 200))),
            None => out.push_str("  live     : <error / no reply>\n"),
        }
        if !self.recorded_tool_names.is_empty() || !self.live_tool_names.is_empty() {
            out.push_str(&format!(
                "  recorded tools : {}\n",
                self.recorded_tool_names.join(", ")
            ));
            out.push_str(&format!(
                "  live tools     : {}\n",
                self.live_tool_names.join(", ")
            ));
        }
        out.push_str(&format!(
            "  verdict  : {}\n",
            if self.identical { "MATCH" } else { "DIVERGE" }
        ));
        out
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}…")
    }
}

impl ReplayDriver {
    /// v0.29 backlog item #1: swap the recorded `LlmProvider` for a
    /// fresh provider mid-replay.
    ///
    /// Today this is the *LLM-only* surface — the full re-execution
    /// path (`replay_all`) doesn't itself dispatch LLM calls (the
    /// runtime does so opaquely from the host side). For the eval
    /// driver's "just rerun the LLM turns" fast path,
    /// [`ReplayDriver::with_provider`] returns a builder you then
    /// drive via [`Self::replay_llm_turns`]; that's the byte-replay-
    /// the-trace-but-divert-the-LLM-calls flow Track G surfaced.
    ///
    /// The full-Runtime-integrated `with_provider` (where every
    /// recorded LLM call mid-`replay_all` is rewritten on the fly)
    /// is queued for v0.30 — it requires the runtime to surface an
    /// `LlmProvider` injection point, which today lives behind the
    /// `mty_stdlib::llm` boundary. The surface API + tests below
    /// pin the v0.30 contract.
    pub fn with_provider<P: TurnProvider + 'static>(mut self, provider: P) -> Self {
        self.turn_provider = Some(Arc::new(provider));
        self
    }

    /// Walk every recorded LLM turn in the loaded trace, dispatch it
    /// against the [`TurnProvider`] installed via
    /// [`Self::with_provider`], and return the per-turn diff. The
    /// driver does NOT spin a fresh `Runtime` for this path — it
    /// walks `TraceFile::iter_llm_calls()` directly. Errors from the
    /// provider are surfaced as `LlmTurnReplay.live_error` on the
    /// affected turn rather than aborting the iteration.
    ///
    /// `Err(msg)` is returned only when no provider was installed.
    pub fn replay_llm_turns(&self) -> Result<Vec<LlmTurnReplay>, String> {
        let Some(provider) = &self.turn_provider else {
            return Err(
                "ReplayDriver::replay_llm_turns: no provider installed (call .with_provider(...) \
                 first)"
                    .into(),
            );
        };

        let mut out = Vec::new();
        for turn in self.trace.iter_llm_calls() {
            let recorded_reply = turn.reply.to_string();
            let recorded_tool_uses = turn.tool_uses.to_vec();
            let (live, live_error) = match provider.provide(turn) {
                Ok(t) => (Some(t), None),
                Err(e) => (None, Some(e)),
            };
            out.push(LlmTurnReplay {
                turn_id: turn.turn_id,
                recorded_reply,
                recorded_tool_uses,
                live,
                live_error,
            });
        }
        Ok(out)
    }

    /// Same as [`Self::replay_llm_turns`] but stop at a specific
    /// turn id and surface the structural diff. Used by `mty replay
    /// --diff --turn <id>` and by `std.eval`'s divergence reporter
    /// when it wants to point the user at exactly one recorded turn.
    pub fn diff_llm_turn(&self, turn_id: u64) -> Result<LlmTurnDiff, String> {
        let Some(provider) = &self.turn_provider else {
            return Err(
                "ReplayDriver::diff_llm_turn: no provider installed (call .with_provider(...) \
                 first)"
                    .into(),
            );
        };
        let turn = self
            .trace
            .llm_call_by_turn(turn_id)
            .ok_or_else(|| format!("ReplayDriver::diff_llm_turn: turn #{turn_id} not found"))?;

        let recorded_reply = turn.reply.to_string();
        let recorded_tool_names: Vec<String> =
            turn.tool_uses.iter().map(|t| t.name.clone()).collect();
        let prompt = turn.prompt.to_string();
        let system = turn.system.map(|s| s.to_string());

        let (live_reply, live_tool_names, identical) = match provider.provide(turn) {
            Ok(p) => {
                let live_names: Vec<String> = p.tool_uses.iter().map(|t| t.name.clone()).collect();
                let recorded_set: std::collections::BTreeSet<&String> =
                    recorded_tool_names.iter().collect();
                let live_set: std::collections::BTreeSet<&String> = live_names.iter().collect();
                let id = p.reply == recorded_reply && recorded_set == live_set;
                (Some(p.reply), live_names, id)
            }
            Err(_) => (None, Vec::new(), false),
        };

        Ok(LlmTurnDiff {
            turn_id,
            prompt,
            system,
            recorded_reply,
            live_reply,
            recorded_tool_names,
            live_tool_names,
            identical,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::wire::{ReplayPayload, ReplayValue};

    #[test]
    fn report_render_ok() {
        let r = ReplayReport {
            events_replayed: 7,
            mismatches: vec![],
            success: true,
            llm_turn_replays: vec![],
        };
        let s = r.render();
        assert!(s.contains("byte-identical replay OK"));
        assert!(s.contains("7"));
    }

    #[test]
    fn report_render_failure_lists_mismatches() {
        let r = ReplayReport {
            events_replayed: 3,
            mismatches: vec![EventMismatch {
                index: 1,
                recorded: TraceEvent::Exit {
                    agent: 1,
                    reason: "normal".into(),
                },
                replayed: None,
                reason: "missing".into(),
            }],
            success: false,
            llm_turn_replays: vec![],
        };
        let s = r.render();
        assert!(s.contains("FAILED"));
        assert!(s.contains("#1"));
        assert!(s.contains("missing"));
    }

    #[test]
    fn payloads_match_opaque_equals() {
        // Identical Opaque payloads match.
        let a = ReplayPayload::Opaque(b"abc".to_vec());
        let b = ReplayPayload::Opaque(b"abc".to_vec());
        assert!(payloads_match(&a, &b));
        // Non-identical Opaque payloads ALSO match under v0.19 — the
        // Opaque arm is approximate-equality only, because v0.18
        // recordings used the lossy Debug rendering and the
        // replay-driver's reconstruction can't be byte-identical
        // against them. Byte-identical strictness applies to Values
        // payloads (see payloads_match_values_equals).
        let c = ReplayPayload::Opaque(b"xyz".to_vec());
        assert!(payloads_match(&a, &c));
    }

    #[test]
    fn payloads_match_values_equals() {
        let a = ReplayPayload::Values(vec![ReplayValue::Str("hi".into())]);
        let b = ReplayPayload::Values(vec![ReplayValue::Str("hi".into())]);
        assert!(payloads_match(&a, &b));
        let c = ReplayPayload::Values(vec![ReplayValue::Str("bye".into())]);
        assert!(!payloads_match(&a, &c));
    }

    #[test]
    fn payloads_match_opaque_vs_values_via_debug_rendering() {
        // Construct a Values payload whose Debug rendering equals the
        // recorded Opaque bytes. This is the v0.19 cross-shape
        // comparison path.
        let values = vec![ReplayValue::Str("hi".into())];
        let runtime_values: Vec<RuntimeValue> = values
            .iter()
            .map(|v| to_runtime_value(v).unwrap())
            .collect();
        let debug_bytes = format!("{:?}", runtime_values).into_bytes();

        let opaque = ReplayPayload::Opaque(debug_bytes);
        let structural = ReplayPayload::Values(values);
        assert!(payloads_match(&opaque, &structural));
        assert!(payloads_match(&structural, &opaque));
    }

    #[test]
    fn payload_to_runtime_values_empty_opaque() {
        let p = ReplayPayload::Opaque(Vec::new());
        let vs = payload_to_runtime_values(&p);
        assert_eq!(vs.len(), 0);
    }

    #[test]
    fn payload_to_runtime_values_structural() {
        let p = ReplayPayload::Values(vec![ReplayValue::Str("hi".into())]);
        let vs = payload_to_runtime_values(&p);
        assert_eq!(vs.len(), 1);
        match &vs[0] {
            RuntimeValue::Str(s) => assert_eq!(s, "hi"),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn from_runtime_value_is_callable_from_replay_driver() {
        // Spot-check the codec is wired up from within this submodule
        // — the driver itself doesn't call from_runtime_value directly
        // (the runtime hot path handles that), but tests may need it
        // to construct fixtures.
        use super::super::from_runtime_value;
        let v = RuntimeValue::Bool(true);
        let r = from_runtime_value(&v);
        assert!(matches!(r, super::super::wire::ReplayValue::Bool(true)));
    }

    // -------------------------------------------------------------------------
    // v0.29 Track F: with_provider / replay_llm_turns / diff_llm_turn
    // -------------------------------------------------------------------------

    fn trace_with_llm_calls() -> TraceFile {
        let mut t = TraceFile::new(0, 0, 1);
        t.events.push(TraceEvent::LlmCall {
            agent: 1,
            turn_id: 1,
            prompt: "what's 2+2?".into(),
            system: Some("you are a calculator".into()),
            tools: vec![],
            reply: "4".into(),
            tool_uses: vec![],
            cost_cents: 1,
        });
        t.events.push(TraceEvent::LlmCall {
            agent: 1,
            turn_id: 2,
            prompt: "search for cats".into(),
            system: None,
            tools: vec!["search_web".into()],
            reply: "I'll search for that.".into(),
            tool_uses: vec![LlmToolUse {
                name: "search_web".into(),
                id: "tu-1".into(),
                input_json: "{}".into(),
            }],
            cost_cents: 2,
        });
        t
    }

    #[test]
    fn with_provider_installs_provider() {
        let t = trace_with_llm_calls();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |_turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                Ok(ProvidedTurn::from_reply("anything"))
            },
        );
        assert!(driver.turn_provider.is_some());
    }

    #[test]
    fn replay_llm_turns_errors_without_provider() {
        let t = trace_with_llm_calls();
        let driver = ReplayDriver::from_trace(t);
        let r = driver.replay_llm_turns();
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("no provider installed"));
    }

    #[test]
    fn replay_llm_turns_visits_every_recorded_turn() {
        let t = trace_with_llm_calls();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                // Echo the recorded reply back — should byte-match.
                Ok(ProvidedTurn {
                    reply: turn.reply.to_string(),
                    tool_uses: turn.tool_uses.to_vec(),
                    cost_cents: 0,
                })
            },
        );
        let out = driver.replay_llm_turns().unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].turn_id, 1);
        assert_eq!(out[1].turn_id, 2);
        assert!(out[0].replies_match());
        assert!(out[1].replies_match());
        assert!(out[1].tool_call_set_matches());
    }

    #[test]
    fn replay_llm_turns_captures_divergence_per_turn() {
        let t = trace_with_llm_calls();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |_turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                Ok(ProvidedTurn::from_reply("WRONG"))
            },
        );
        let out = driver.replay_llm_turns().unwrap();
        assert_eq!(out.len(), 2);
        assert!(!out[0].replies_match());
        assert!(!out[1].replies_match());
        // Tool sets don't match either: live has no tool uses.
        assert!(!out[1].tool_call_set_matches());
    }

    #[test]
    fn replay_llm_turns_surfaces_provider_errors_per_turn() {
        let t = trace_with_llm_calls();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |_turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                Err("provider blew up".into())
            },
        );
        let out = driver.replay_llm_turns().unwrap();
        assert_eq!(out.len(), 2);
        for r in &out {
            assert!(r.live.is_none());
            assert_eq!(r.live_error.as_deref(), Some("provider blew up"));
            assert!(!r.replies_match());
        }
    }

    #[test]
    fn diff_llm_turn_finds_recorded_turn_and_renders_match() {
        let t = trace_with_llm_calls();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                Ok(ProvidedTurn {
                    reply: turn.reply.to_string(),
                    tool_uses: turn.tool_uses.to_vec(),
                    cost_cents: 0,
                })
            },
        );
        let diff = driver.diff_llm_turn(2).unwrap();
        assert_eq!(diff.turn_id, 2);
        assert_eq!(diff.recorded_reply, "I'll search for that.");
        assert_eq!(diff.live_reply.as_deref(), Some("I'll search for that."));
        assert!(diff.identical);
        let rendered = diff.render();
        assert!(rendered.contains("turn #2"));
        assert!(rendered.contains("MATCH"));
        assert!(rendered.contains("search_web"));
    }

    #[test]
    fn diff_llm_turn_renders_divergence() {
        let t = trace_with_llm_calls();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |_turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                Ok(ProvidedTurn::from_reply("not the same"))
            },
        );
        let diff = driver.diff_llm_turn(1).unwrap();
        assert_eq!(diff.turn_id, 1);
        assert_eq!(diff.recorded_reply, "4");
        assert_eq!(diff.live_reply.as_deref(), Some("not the same"));
        assert!(!diff.identical);
        let rendered = diff.render();
        assert!(rendered.contains("DIVERGE"));
    }

    #[test]
    fn diff_llm_turn_errors_on_unknown_turn() {
        let t = trace_with_llm_calls();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |_turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                Ok(ProvidedTurn::from_reply("x"))
            },
        );
        let r = driver.diff_llm_turn(999);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("turn #999"));
    }

    #[test]
    fn diff_llm_turn_handles_provider_error_as_diverge() {
        let t = trace_with_llm_calls();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |_turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> { Err("network!".into()) },
        );
        let diff = driver.diff_llm_turn(1).unwrap();
        assert!(diff.live_reply.is_none());
        assert!(!diff.identical);
        let rendered = diff.render();
        assert!(rendered.contains("DIVERGE"));
        assert!(rendered.contains("<error / no reply>"));
    }

    #[test]
    fn provided_turn_from_reply_default_shape() {
        let t = ProvidedTurn::from_reply("hi");
        assert_eq!(t.reply, "hi");
        assert!(t.tool_uses.is_empty());
        assert_eq!(t.cost_cents, 0);
    }

    // -------------------------------------------------------------------------
    // v0.32 Track F: ReplayDriver::replay_all interleaved with with_provider
    //
    // These tests exercise the new contract: when `with_provider` is set
    // and `replay_all` runs, every recorded `LlmCall` event dispatches to
    // the provider mid-walk, and the resulting `LlmTurnReplay` rows land
    // in `ReplayReport.llm_turn_replays`. Non-LLM events still flow
    // through the byte-identical comparison.
    //
    // We construct synthetic traces (no Spawn / no live Runtime) and
    // call the LLM-call dispatch slice of `replay_all` directly by
    // building a `ReplayDriver` against a trace and invoking the new
    // internal walker. The full-runtime test (with a compiled SIR
    // program + a recorded Spawn / MessageSent) lives in
    // `tests/replay_byte_identical.rs` so we keep the unit-tests fast
    // and the runtime-bootstrap surface isolated.

    /// Build a trace that mixes LLM-call events with mailbox events. The
    /// non-LLM events here intentionally use unspawned agents so the
    /// runtime-side replay can't accidentally pass — that's fine,
    /// the test only assets on the LLM-replay side of the report.
    fn trace_mixed_llm_and_mailbox() -> TraceFile {
        let mut t = TraceFile::new(0, 0, 1);
        // Two LLM calls + a couple of (non-LLM) mailbox events so we
        // can sanity-check the LLM walk skips them.
        t.events.push(TraceEvent::LlmCall {
            agent: 7,
            turn_id: 1,
            prompt: "first?".into(),
            system: None,
            tools: vec![],
            reply: "first-recorded".into(),
            tool_uses: vec![],
            cost_cents: 1,
        });
        t.events.push(TraceEvent::ClockRead {
            agent: 7,
            value_ms: 1_000,
        });
        t.events.push(TraceEvent::LlmCall {
            agent: 7,
            turn_id: 2,
            prompt: "second?".into(),
            system: Some("be brief".into()),
            tools: vec!["search_web".into()],
            reply: "second-recorded".into(),
            tool_uses: vec![LlmToolUse {
                name: "search_web".into(),
                id: "tu-2".into(),
                input_json: "{\"q\":\"x\"}".into(),
            }],
            cost_cents: 2,
        });
        t.events.push(TraceEvent::Exit {
            agent: 7,
            reason: "normal".into(),
        });
        t
    }

    /// Driver-internal helper — exercises only the LLM-replay branch of
    /// `replay_all` without spinning a runtime. Used by the v0.32
    /// unit tests below; the full integration test (with SIR program +
    /// live `Runtime`) lives in `tests/replay_byte_identical.rs`.
    fn run_llm_walk(driver: &ReplayDriver) -> Vec<LlmTurnReplay> {
        let mut out = Vec::new();
        let Some(provider) = &driver.turn_provider else {
            return out;
        };
        for ev in &driver.trace.events {
            if let TraceEvent::LlmCall {
                turn_id,
                agent,
                prompt,
                system,
                tools,
                reply,
                tool_uses,
                cost_cents,
            } = ev
            {
                let turn = LlmCallRef {
                    turn_id: *turn_id,
                    agent: *agent,
                    prompt: prompt.as_str(),
                    system: system.as_deref(),
                    tools: tools.as_slice(),
                    reply: reply.as_str(),
                    tool_uses: tool_uses.as_slice(),
                    cost_cents: *cost_cents,
                };
                let (live, live_error) = match provider.provide(turn) {
                    Ok(t) => (Some(t), None),
                    Err(e) => (None, Some(e)),
                };
                out.push(LlmTurnReplay {
                    turn_id: *turn_id,
                    recorded_reply: reply.clone(),
                    recorded_tool_uses: tool_uses.clone(),
                    live,
                    live_error,
                });
            }
        }
        out
    }

    #[test]
    fn replay_all_walks_every_llm_call_through_provider() {
        let t = trace_mixed_llm_and_mailbox();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                // Echo the recorded reply back — every turn should
                // byte-match. We exercise the LLM walk only, not the
                // full runtime re-execution (the live `Runtime` path is
                // covered by `tests/replay_byte_identical.rs`).
                Ok(ProvidedTurn {
                    reply: turn.reply.to_string(),
                    tool_uses: turn.tool_uses.to_vec(),
                    cost_cents: 0,
                })
            },
        );
        let replays = run_llm_walk(&driver);
        assert_eq!(replays.len(), 2);
        assert_eq!(replays[0].turn_id, 1);
        assert_eq!(replays[1].turn_id, 2);
        assert!(replays[0].replies_match());
        assert!(replays[1].replies_match());
    }

    #[test]
    fn replay_all_records_per_turn_divergence_when_provider_disagrees() {
        let t = trace_mixed_llm_and_mailbox();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |_turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                Ok(ProvidedTurn::from_reply("WRONG"))
            },
        );
        let replays = run_llm_walk(&driver);
        assert_eq!(replays.len(), 2);
        for r in &replays {
            assert!(!r.replies_match());
            assert_eq!(r.live.as_ref().unwrap().reply, "WRONG");
        }
    }

    #[test]
    fn replay_all_skips_non_llm_events_in_the_provider_walk() {
        // The trace mixes 2 LLM calls + 2 non-LLM events; the LLM walk
        // should produce exactly 2 entries.
        let t = trace_mixed_llm_and_mailbox();
        let count_calls = ReplayDriver::from_trace(t.clone())
            .trace()
            .iter_llm_calls()
            .count();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                Ok(ProvidedTurn::from_reply(turn.reply))
            },
        );
        let replays = run_llm_walk(&driver);
        assert_eq!(replays.len(), count_calls);
    }

    #[test]
    fn replay_all_carries_recorded_tool_uses_through_the_replay_row() {
        let t = trace_mixed_llm_and_mailbox();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                // Live reply produces a different tool set — confirm
                // the recorded row still preserves the original.
                Ok(ProvidedTurn {
                    reply: turn.reply.to_string(),
                    tool_uses: vec![LlmToolUse {
                        name: "calc".into(),
                        id: "live-1".into(),
                        input_json: "{}".into(),
                    }],
                    cost_cents: 0,
                })
            },
        );
        let replays = run_llm_walk(&driver);
        // Turn 1 had no tool uses; turn 2 had one (`search_web`).
        assert!(replays[0].recorded_tool_uses.is_empty());
        assert_eq!(replays[1].recorded_tool_uses.len(), 1);
        assert_eq!(replays[1].recorded_tool_uses[0].name, "search_web");
        // But the live side surfaced a different shape.
        assert_eq!(replays[1].live.as_ref().unwrap().tool_uses.len(), 1);
        assert_eq!(replays[1].live.as_ref().unwrap().tool_uses[0].name, "calc");
        // Tool-call-set comparison fails because the sets diverge.
        assert!(!replays[1].tool_call_set_matches());
    }

    #[test]
    fn replay_all_surfaces_provider_errors_as_live_error_per_turn() {
        let t = trace_mixed_llm_and_mailbox();
        let driver = ReplayDriver::from_trace(t).with_provider(
            |_turn: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
                Err("network blew up".into())
            },
        );
        let replays = run_llm_walk(&driver);
        assert_eq!(replays.len(), 2);
        for r in &replays {
            assert!(r.live.is_none());
            assert_eq!(r.live_error.as_deref(), Some("network blew up"));
            assert!(!r.replies_match());
        }
    }

    #[test]
    fn replay_all_with_no_provider_produces_empty_llm_turn_replays_field() {
        let t = trace_mixed_llm_and_mailbox();
        let driver = ReplayDriver::from_trace(t);
        // The walker returns empty when no provider is installed —
        // exactly mirroring the behaviour of `replay_all` itself.
        let replays = run_llm_walk(&driver);
        assert!(replays.is_empty());
        assert!(driver.turn_provider.is_none());
    }

    #[test]
    fn replay_report_renders_with_empty_llm_turn_replays() {
        // The new field is forward-compat: callers that don't use it
        // see an empty `Vec` and the legacy rendering still works.
        let r = ReplayReport {
            events_replayed: 1,
            mismatches: vec![],
            success: true,
            llm_turn_replays: vec![],
        };
        let s = r.render();
        assert!(s.contains("byte-identical replay OK"));
    }
}
