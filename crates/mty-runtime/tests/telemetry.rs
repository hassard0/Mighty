//! v0.16 OpenTelemetry agent-span layer — integration tests.
//!
//! The new telemetry layer lives in `mty_runtime::telemetry::{spans,
//! events}` and is exposed through the crate root as `span_spawn`,
//! `span_send`, `span_ask`, `span_handler`, `record_restart`,
//! `record_budget_exhausted`, `agent_event`, plus `init_from_env` /
//! `shutdown`. These tests exercise the public surface and verify the
//! disabled-path no-op contract.

use mty_runtime::{
    agent_event, init_telemetry_from_env, record_budget_exhausted, record_restart,
    shutdown_telemetry, span_ask, span_handler, span_send, span_spawn,
};

/// Test 1 — without `MTY_OTLP_ENDPOINT`, init is a no-op and no
/// runtime side effect should be observable. We just verify it
/// doesn't panic and that the helpers are callable.
#[test]
fn init_with_no_env_is_noop() {
    std::env::remove_var("MTY_OTLP_ENDPOINT");
    std::env::remove_var("MTY_OTLP_PROTOCOL");
    std::env::remove_var("MTY_OTLP_SAMPLE_RATE");
    init_telemetry_from_env();
    // The disabled-path span helpers should be cheap and safe.
    let _g = span_spawn("Echoer");
    span_send("Hit");
    let v = span_ask("Q", || 7);
    assert_eq!(v, 7);
    shutdown_telemetry();
}

/// Test 2 — with `MTY_OTLP_ENDPOINT` set, init attempts to install the
/// exporter. We do NOT spin up a real collector — the exporter is
/// allowed to fail (gRPC connection deferred to first export). The
/// contract is that init must not panic and a subsequent shutdown
/// must also not panic.
///
/// The batch exporter uses the tokio runtime for background flushes,
/// so this test runs under `tokio::test`.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn init_with_env_attempts_otlp() {
    // Use a port that's unlikely to be in use; we don't care if
    // the exporter ultimately can't talk to it.
    std::env::set_var("MTY_OTLP_ENDPOINT", "http://127.0.0.1:14317");
    init_telemetry_from_env();
    let _g = span_spawn("X");
    shutdown_telemetry();
    std::env::remove_var("MTY_OTLP_ENDPOINT");
}

/// Test 3 — RAII guard ends the span on drop. We can't peek into
/// OTel internals without a real exporter, so we verify the guard's
/// Drop runs without panic and a subsequent re-entry works.
#[test]
fn span_spawn_closes_on_drop() {
    {
        let _g = span_spawn("Echoer");
        // _g is alive here, span is open
    }
    // _g dropped, span ended. Reopen — must succeed.
    {
        let _g = span_spawn("Echoer");
    }
}

/// Test 4 — `agent_event` outside a handler context falls back to
/// stdout. We don't capture stdout here (awkward portably); we just
/// confirm the call returns without panic. The line-format is
/// covered by the inline unit test in `telemetry::events::tests`.
#[test]
fn agent_event_stdout_fallback() {
    // No active task-local handler-span — must take the stdout
    // fallback.
    agent_event(
        "test_event",
        &[("k1", "v1"), ("k2", "v2"), ("k3", "v with spaces")],
    );
    // Edge case: empty fields slice.
    agent_event("no_fields", &[]);
}

/// Test 5 — `record_restart` and `record_budget_exhausted` are safe
/// outside a handler-span context.
#[test]
fn supervisor_and_budget_helpers_outside_handler_are_safe() {
    record_restart("child_panicked");
    record_budget_exhausted("wall_budget");
    record_restart("escalated_to_root");
}

/// Test 6 — `span_handler` returns a guard whose context can be
/// queried; opening + dropping must not panic.
#[test]
fn span_handler_runs_for_dispatch() {
    let guard = span_handler("Echoer", "Ping");
    let _ctx = guard.context();
    // Drop ends the span.
    drop(guard);
}

/// Test 7 — Sample rate env var is parsed and accepted in the valid
/// range. We can't observe the sampler without a real exporter; this
/// just verifies init handles the env var without panicking.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn init_with_sample_rate_env() {
    std::env::set_var("MTY_OTLP_ENDPOINT", "http://127.0.0.1:14317");
    std::env::set_var("MTY_OTLP_SAMPLE_RATE", "0.25");
    init_telemetry_from_env();
    shutdown_telemetry();
    std::env::remove_var("MTY_OTLP_ENDPOINT");
    std::env::remove_var("MTY_OTLP_SAMPLE_RATE");
}

/// Test 8 — `init_from_env` is idempotent. Calling it twice in a row
/// (with the same env state) must not panic and must not double-install.
///
/// Note: we defensively clear `MTY_OTLP_ENDPOINT` first. Env vars are
/// process-wide, and tests 2/7 (which set the endpoint to spin up the
/// batch exporter) run under `tokio::test` because the exporter needs
/// a reactor. If we raced one of those tests after it set the env but
/// before its end-of-test `remove_var`, this plain `#[test]` would
/// try to spin up the OTLP exporter without a tokio runtime and panic
/// with "there is no reactor running". Defensive removes here keep the
/// test independent of cross-test ordering.
#[test]
fn init_is_idempotent() {
    std::env::remove_var("MTY_OTLP_ENDPOINT");
    std::env::remove_var("MTY_OTLP_PROTOCOL");
    std::env::remove_var("MTY_OTLP_SAMPLE_RATE");
    init_telemetry_from_env();
    init_telemetry_from_env();
    init_telemetry_from_env();
    shutdown_telemetry();
}

/// Test 9 — `shutdown_telemetry` is safe when nothing was ever
/// initialised. Defensively clear the OTLP env vars so we don't
/// accidentally trigger init on shutdown (same cross-test race
/// guarded against in test 8).
#[test]
fn shutdown_without_init_is_safe() {
    std::env::remove_var("MTY_OTLP_ENDPOINT");
    shutdown_telemetry();
    shutdown_telemetry();
}

/// Test 10 — span_send is fire-and-forget and does not block.
#[test]
fn span_send_returns_immediately() {
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        span_send("Hit");
    }
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "span_send should be fast"
    );
}
