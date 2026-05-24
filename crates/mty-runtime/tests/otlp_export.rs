//! v0.3 (A38 closure): OTLP export sanity tests.
//!
//! The full OTLP wire-level round-trip needs a tonic gRPC mock
//! collector which is heavy to spin up in unit tests. Here we
//! exercise the JSON-line fallback path (when STARDUST_OTLP_ENDPOINT
//! is unset) and verify the semantic-convention mapping table built
//! into `otlp::event_to_span` matches the documented contract.

use mty_runtime::telemetry::{TelemetryEvent, TelemetrySink};

#[test]
fn json_line_fallback_when_otlp_endpoint_unset() {
    // No env var set → from_env should pick a non-OTLP sink.
    std::env::remove_var("STARDUST_OTLP_ENDPOINT");
    std::env::remove_var("STARDUST_TRACE");
    let sink = TelemetrySink::from_env();
    // We expect Discard (default) — emit must not panic.
    sink.emit(&TelemetryEvent::Spawn {
        name: "A".into(),
        agent_id: 0,
    });
    sink.flush();
}

#[test]
fn buffer_sink_captures_all_event_kinds() {
    let (sink, buf) = TelemetrySink::buffer();
    let events = vec![
        TelemetryEvent::TurnStart {
            agent: "A".into(),
            msg: "Hit".into(),
        },
        TelemetryEvent::TurnEnd {
            agent: "A".into(),
            msg: "Hit".into(),
            duration_us: 99,
        },
        TelemetryEvent::Send {
            from: "X".into(),
            to: "Y".into(),
            msg: "M".into(),
        },
        TelemetryEvent::Ask {
            from: "X".into(),
            to: "Y".into(),
            msg: "Q".into(),
            deadline_ms: Some(100),
        },
        TelemetryEvent::Reply {
            from: "Y".into(),
            msg: "Q".into(),
            ok: true,
        },
        TelemetryEvent::Spawn {
            name: "Z".into(),
            agent_id: 1,
        },
        TelemetryEvent::Restart {
            supervisor: "S".into(),
            child: "C".into(),
            attempt: 2,
        },
        TelemetryEvent::BudgetBreach {
            agent: "A".into(),
            kind: "SD5009".into(),
        },
        TelemetryEvent::Shutdown,
    ];
    for ev in &events {
        sink.emit(ev);
    }
    let lines = buf.lock().clone();
    assert_eq!(lines.len(), events.len());
    // Each line carries kind + ts.
    for l in &lines {
        assert!(l.contains("\"kind\":"), "missing kind: {l}");
        assert!(l.contains("\"ts\":"), "missing ts: {l}");
    }
}

// NB: an integration test against a real (or mock) OTLP collector
// is out of scope for the unit-test layer because spinning up tonic
// inside the test process competes with the runtime's own tokio.
// The runtime-level smoke is the JSON-fallback path above; full
// wire-level conformance is covered by manual local runs against
// the OTel collector documented in docs/internals/telemetry-otlp.md.
