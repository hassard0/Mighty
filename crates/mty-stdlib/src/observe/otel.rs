//! OTLP/HTTP exporter for [`LlmObservation`] (Phase 2 stub).
//!
//! v0.30 Track D ships this as a documented stub + one round-trip
//! test — the SQLite path is the must-ship, but the env hook is
//! wired end-to-end so v0.31 can flesh the wire format out without
//! a CLI break.
//!
//! ## Wire format (planned)
//!
//! The full exporter emits one OTLP/HTTP span per observation
//! against the configured `MTY_OBSERVE_OTEL=<endpoint>`. The span
//! shape mirrors the [OpenLLMetry conventions][ollm] so dashboards
//! that already understand LLM cost (LangSmith, Arize, Honeycomb's
//! LLM panel, etc.) light up out of the box:
//!
//! - `span.kind = client`
//! - `span.name = "llm.{provider}.complete"`
//! - `attributes.gen_ai.system = provider`
//! - `attributes.gen_ai.request.model = model`
//! - `attributes.gen_ai.usage.input_tokens`
//! - `attributes.gen_ai.usage.output_tokens`
//! - `attributes.gen_ai.usage.cost = cost_cents / 100.0`
//! - `attributes.mty.agent_id`
//! - `events[*].name = "tool_call"`
//!
//! [ollm]: https://github.com/traceloop/openllmetry
//!
//! ## v0.30 implementation
//!
//! The stub stores observations in an in-memory ring buffer so the
//! invariant test ("when `MTY_OBSERVE_OTEL` is set, records flow to
//! the OTel sink, not the SQLite sink") can run without spinning a
//! real OTel collector. Calling `flush()` returns the buffered
//! records — v0.31 replaces this with a real HTTP exporter.

use crate::observe::observation::LlmObservation;
use crate::observe::storage::ObservationStore;
use std::sync::Mutex;

/// OTLP/HTTP exporter shim. Implements [`ObservationStore`] so
/// `record_if_enabled` can route to it instead of SQLite when
/// `MTY_OBSERVE_OTEL=...` is set.
pub struct OtelStore {
    /// Configured endpoint (e.g. `http://otel-collector:4318`).
    /// Stored verbatim — v0.30 doesn't actually POST. The v0.31
    /// exporter will build `{endpoint}/v1/traces` and `{endpoint}/v1/metrics`.
    pub endpoint: String,
    /// In-memory ring of pending records. Bounded at 1024 so a
    /// long-running agent doesn't unbounded-grow.
    buffered: Mutex<Vec<LlmObservation>>,
}

impl OtelStore {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            buffered: Mutex::new(Vec::new()),
        }
    }

    /// v0.30 helper for tests + the future flush hook. Returns the
    /// buffered records and clears the buffer.
    pub fn flush(&self) -> Vec<LlmObservation> {
        let mut g = self.buffered.lock().expect("otel buffer mutex poisoned");
        std::mem::take(&mut *g)
    }

    /// Peek at the current buffer length without draining.
    pub fn pending_count(&self) -> usize {
        self.buffered
            .lock()
            .expect("otel buffer mutex poisoned")
            .len()
    }
}

impl ObservationStore for OtelStore {
    fn record(&self, obs: &LlmObservation) {
        let mut g = self.buffered.lock().expect("otel buffer mutex poisoned");
        if g.len() >= 1024 {
            // Drop the oldest to bound memory.
            g.remove(0);
        }
        g.push(obs.clone());
    }

    fn snapshot(&self) -> Option<Vec<LlmObservation>> {
        // The OTel exporter is logically write-only — `snapshot()`
        // returns the *pending* buffer so test code can verify what
        // would be exported, but production code shouldn't call it.
        let g = self.buffered.lock().expect("otel buffer mutex poisoned");
        Some(g.clone())
    }

    fn clear(&self) {
        let mut g = self.buffered.lock().expect("otel buffer mutex poisoned");
        g.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otel_stub_buffers_records() {
        let s = OtelStore::new("http://localhost:4318");
        assert_eq!(s.endpoint, "http://localhost:4318");
        s.record(&LlmObservation::new(
            "anthropic",
            "claude-opus-4-7",
            100,
            50,
            10,
        ));
        s.record(&LlmObservation::new("openai", "gpt-5", 200, 100, 20));
        assert_eq!(s.pending_count(), 2);
        let drained = s.flush();
        assert_eq!(drained.len(), 2);
        assert_eq!(s.pending_count(), 0);
    }

    #[test]
    fn otel_stub_bounds_memory_at_1024() {
        let s = OtelStore::new("http://localhost:4318");
        for i in 0..1100u64 {
            s.record(&LlmObservation::new(
                "anthropic",
                "claude-opus-4-7",
                i,
                0,
                1,
            ));
        }
        assert_eq!(s.pending_count(), 1024);
        // Oldest dropped first → buffer should start at i = 76.
        let buf = s.snapshot().unwrap();
        assert_eq!(buf[0].prompt_tokens, 76);
        assert_eq!(buf.last().unwrap().prompt_tokens, 1099);
    }
}
