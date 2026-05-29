//! Telemetry JSON line emitter + OTLP wire-format bridge.
//!
//! Slice 7 shipped JSON lines only (A38). v0.3 adds [`TelemetrySink::Otlp`]
//! which forwards every event to an OpenTelemetry collector via the
//! `opentelemetry-otlp` crate (see [`crate::otlp`]). The legacy JSON
//! sinks remain so tests and local dev work without a collector
//! running.
//!
//! Selection precedence in `from_env()`:
//!
//! 1. `STARDUST_OTLP_ENDPOINT=<url>` → `TelemetrySink::Otlp` (or
//!    fallback to `Discard` if OTLP init fails — never breaks runtime).
//! 2. `STARDUST_TRACE=stderr|file:<p>` → JSON line emitter.
//! 3. Otherwise `Discard`.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub enum TelemetryEvent {
    TurnStart {
        agent: String,
        msg: String,
    },
    TurnEnd {
        agent: String,
        msg: String,
        duration_us: u128,
    },
    Send {
        from: String,
        to: String,
        msg: String,
    },
    Ask {
        from: String,
        to: String,
        msg: String,
        deadline_ms: Option<u64>,
    },
    Reply {
        from: String,
        msg: String,
        ok: bool,
    },
    Spawn {
        name: String,
        agent_id: u64,
    },
    Restart {
        supervisor: String,
        child: String,
        attempt: u32,
    },
    BudgetBreach {
        agent: String,
        kind: String,
    },
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
            TelemetryEvent::TurnStart { agent, msg } => {
                format!(r#""agent":"{}","msg":"{}""#, esc(agent), esc(msg))
            }
            TelemetryEvent::TurnEnd {
                agent,
                msg,
                duration_us,
            } => format!(
                r#""agent":"{}","msg":"{}","duration_us":{}"#,
                esc(agent),
                esc(msg),
                duration_us
            ),
            TelemetryEvent::Send { from, to, msg } => format!(
                r#""from":"{}","to":"{}","msg":"{}""#,
                esc(from),
                esc(to),
                esc(msg)
            ),
            TelemetryEvent::Ask {
                from,
                to,
                msg,
                deadline_ms,
            } => format!(
                r#""from":"{}","to":"{}","msg":"{}","deadline_ms":{}"#,
                esc(from),
                esc(to),
                esc(msg),
                deadline_ms
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "null".into())
            ),
            TelemetryEvent::Reply { from, msg, ok } => {
                format!(r#""from":"{}","msg":"{}","ok":{}"#, esc(from), esc(msg), ok)
            }
            TelemetryEvent::Spawn { name, agent_id } => {
                format!(r#""name":"{}","agent_id":{}"#, esc(name), agent_id)
            }
            TelemetryEvent::Restart {
                supervisor,
                child,
                attempt,
            } => format!(
                r#""supervisor":"{}","child":"{}","attempt":{}"#,
                esc(supervisor),
                esc(child),
                attempt
            ),
            TelemetryEvent::BudgetBreach { agent, kind: k } => {
                format!(r#""agent":"{}","kind":"{}""#, esc(agent), esc(k))
            }
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
    /// OTLP wire-format export (v0.3, A38 closure). Active when the
    /// `otlp` feature is enabled AND `MTY_OTLP_ENDPOINT` (or the
    /// legacy `STARDUST_OTLP_ENDPOINT`) is set; otherwise we fall
    /// back to one of the other sinks at construction time.
    #[cfg(feature = "otlp")]
    Otlp(Arc<crate::otlp::OtlpHandle>),
}

impl TelemetrySink {
    pub fn buffer() -> (Self, Arc<Mutex<Vec<String>>>) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        (TelemetrySink::Buffer(buf.clone()), buf)
    }

    /// Build the sink from environment variables:
    ///
    /// - `MTY_OTLP_ENDPOINT=<url>` (or legacy `STARDUST_OTLP_ENDPOINT`)
    ///   → OTLP exporter (best effort).
    /// - `MTY_TRACE=stderr` (or legacy `STARDUST_TRACE`)
    ///   → JSON to stderr.
    /// - `MTY_TRACE=file:<path>` (or legacy `STARDUST_TRACE`)
    ///   → JSON appended to file.
    /// - (anything else) → Discard.
    ///
    /// v0.36 T4 — `MTY_*` is the new primary spelling. The legacy
    /// `STARDUST_*` spelling continues to work but emits a one-shot
    /// deprecation warning on stderr.
    pub fn from_env() -> Self {
        #[cfg(feature = "otlp")]
        {
            if let Some(ep) = crate::env_compat::lookup_env("OTLP_ENDPOINT") {
                if let Some(h) = crate::otlp::OtlpHandle::try_init(&ep) {
                    return TelemetrySink::Otlp(h);
                }
            }
        }
        match crate::env_compat::lookup_env("TRACE").as_deref() {
            Some("stderr") => TelemetrySink::Stderr,
            Some(v) if v.starts_with("file:") => {
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
            #[cfg(feature = "otlp")]
            TelemetrySink::Otlp(h) => {
                h.emit(ev);
            }
        }
    }

    /// Force-flush pending records, where the sink supports it.
    pub fn flush(&self) {
        match self {
            #[cfg(feature = "otlp")]
            TelemetrySink::Otlp(h) => h.flush(),
            _ => {}
        }
    }
}

// ----------------------------------------------------------------------
// v0.22 — `worker.steals_total{src,dst}` OTel-shaped counter.
//
// The work-stealing loop in `crate::scheduler::work_stealing` calls
// `record_worker_steal(src, dst)` exactly once per successful steal.
// `src = SRC_GLOBAL_INJECTOR (usize::MAX)` represents "stolen from the
// global injector"; any other value is a sibling worker id. We don't
// allocate per-pair structs; the storage is a plain
// `HashMap<(usize,usize), AtomicU64>` behind a `Mutex` because:
//
// 1. Steals are rare relative to executes (a worker that's fully
//    busy never enters the steal path), so contention is low.
// 2. The map cardinality is bounded — `n * n` pairs maximum, and
//    most production deployments run `n <= 64`. That's 4 KiB worst
//    case, far cheaper than a per-pair `AtomicU64` allocation.
// 3. Reading the counter (for tests / introspect / OTel export) is
//    a snapshot — we clone into a `Vec<(src, dst, value)>` and the
//    consumer iterates that.
//
// The counter is `pub`-accessible via [`steal_counter_snapshot`] so
// the integration test in `tests/work_stealing.rs` can assert that
// successful steals were recorded. An OTel-export bridge (out of
// scope for v0.22) would observe this same counter and forward the
// labelled values to the global meter provider.
// ----------------------------------------------------------------------

/// Global process-wide steal counter. Lazy-initialised on first use.
/// `Mutex<HashMap>` because (a) steals are rare, (b) we don't want a
/// fixed-size array — worker count is configurable — and (c) a
/// `DashMap` would add a workspace dep for no real win on this
/// low-frequency path.
pub static WORKER_STEAL_COUNTER: OnceLock<Mutex<HashMap<(usize, usize), AtomicU64>>> =
    OnceLock::new();

fn steal_counter_map() -> &'static Mutex<HashMap<(usize, usize), AtomicU64>> {
    WORKER_STEAL_COUNTER.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Increment `worker.steals_total{src=<src>, dst=<dst>}` by 1.
///
/// `src = usize::MAX` is reserved for "stolen from the global injector"
/// (see `crate::scheduler::work_stealing::SRC_GLOBAL_INJECTOR`).
pub fn record_worker_steal(src: usize, dst: usize) {
    let map = steal_counter_map().lock();
    if let Some(c) = map.get(&(src, dst)) {
        c.fetch_add(1, Ordering::Relaxed);
        return;
    }
    // Slow path: insert. Drop the borrow first because we need to
    // re-take the lock with a `&mut` view to insert.
    drop(map);
    let mut map = steal_counter_map().lock();
    map.entry((src, dst))
        .or_insert_with(|| AtomicU64::new(0))
        .fetch_add(1, Ordering::Relaxed);
}

/// Snapshot every recorded `(src, dst, count)` triple. Order is
/// unspecified. Used by tests + introspect surfaces. Counts that
/// haven't been touched are absent (no zero rows).
pub fn steal_counter_snapshot() -> Vec<(usize, usize, u64)> {
    let map = steal_counter_map().lock();
    map.iter()
        .map(|((s, d), c)| (*s, *d, c.load(Ordering::Relaxed)))
        .collect()
}

/// Sum across all `(src, dst)` pairs. Cheap helper for tests that
/// only care that *some* steal happened.
pub fn steal_counter_total() -> u64 {
    let map = steal_counter_map().lock();
    map.values().map(|c| c.load(Ordering::Relaxed)).sum()
}

/// Reset the counter to zero. **Test-only** — production code should
/// never call this because it would silently drop counter history
/// observed by an OTel exporter mid-stream.
#[doc(hidden)]
pub fn _reset_steal_counter_for_tests() {
    let mut map = steal_counter_map().lock();
    map.clear();
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

    #[test]
    fn steal_counter_increments_and_snapshots() {
        // Use unique labels so this test doesn't depend on other
        // tests in the same process clearing the counter first.
        let src = 8888;
        let dst = 9999;
        let before = steal_counter_snapshot()
            .into_iter()
            .find(|(s, d, _)| *s == src && *d == dst)
            .map(|(_, _, c)| c)
            .unwrap_or(0);
        record_worker_steal(src, dst);
        record_worker_steal(src, dst);
        record_worker_steal(src, dst);
        let after = steal_counter_snapshot()
            .into_iter()
            .find(|(s, d, _)| *s == src && *d == dst)
            .map(|(_, _, c)| c)
            .unwrap_or(0);
        assert_eq!(after - before, 3);
    }

    #[test]
    fn steal_counter_total_aggregates() {
        record_worker_steal(1, 2);
        record_worker_steal(2, 1);
        record_worker_steal(3, 1);
        let total = steal_counter_total();
        assert!(total >= 3, "expected at least 3 events, got {}", total);
    }
}
