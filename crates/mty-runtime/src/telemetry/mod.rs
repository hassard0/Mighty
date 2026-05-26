//! Telemetry surface for the Mighty runtime.
//!
//! This module is split into two cooperating layers:
//!
//! * **[`sink`]** — the slice-7 / v0.3 JSON-line + OTLP-event emitter.
//!   Every internal runtime call site already routes through
//!   [`TelemetrySink::emit`] (see [`crate::runtime::Runtime`]). That
//!   path stays unchanged for backwards-compat: tests and downstream
//!   consumers that import `mty_runtime::telemetry::{TelemetryEvent,
//!   TelemetrySink}` keep working.
//!
//! * **[`spans`] + [`events`] + this module** — the v0.16 OpenTelemetry
//!   span layer. Per the agent-features roadmap (Tier 1.2 / 1.3) we
//!   add RAII span guards for `spawn` / `send` / `ask`, plus a
//!   task-local handler-span context so user code can call
//!   [`events::agent_event`] from inside a handler and the event
//!   attaches to the active span. When `MTY_OTLP_ENDPOINT` is unset
//!   this layer is a zero-cost no-op — no tracer provider is built
//!   and the span guards collapse to inert structs.
//!
//! ## Activation
//!
//! ```text
//! MTY_OTLP_ENDPOINT=http://localhost:4317   # gRPC OTLP collector
//! MTY_OTLP_PROTOCOL=grpc|http               # default grpc
//! MTY_OTLP_SAMPLE_RATE=0.0..=1.0            # default AlwaysOn (1.0)
//! ```
//!
//! Call [`init_from_env`] once at program startup (idempotent — the
//! second call is a no-op). Call [`shutdown`] before exiting to flush
//! pending spans. The runtime's `RuntimeBuilder` does *not* call these
//! automatically because the lifetime of the tracer provider belongs
//! to `main`, not to a particular runtime instance.
//!
//! ## Privacy
//!
//! Spans emit message **names** (the protocol message variant) but
//! never message bodies. This matches the privacy stance documented
//! in `docs/internals/agent-features-roadmap.md` ("Privacy" open
//! question). Body capture would be a separate, opt-in capability.

pub mod events;
pub mod sink;
pub mod spans;

pub use events::agent_event;
pub use sink::{TelemetryEvent, TelemetrySink};
// v0.22 — work-stealing steal counter (recorded by the scheduler's
// work-stealing loop, observed by tests + introspect surfaces).
pub use sink::{record_worker_steal, steal_counter_snapshot, steal_counter_total};
pub use spans::{
    current_handler_context, record_budget_exhausted, record_restart, span_ask, span_handler,
    span_send, span_spawn, HandlerGuard, SpanContext, SpawnGuard,
};

use std::sync::atomic::{AtomicBool, Ordering};

/// Has [`init_from_env`] already succeeded? Used to make the public
/// init/shutdown idempotent and to keep span helpers cheap when
/// telemetry is disabled.
static TELEMETRY_ENABLED: AtomicBool = AtomicBool::new(false);

/// Read OTLP configuration from environment variables and (when
/// `MTY_OTLP_ENDPOINT` is set) install an OpenTelemetry tracer
/// provider with an OTLP exporter.
///
/// Idempotent: subsequent calls return without re-initialising.
/// Failure is non-fatal — if the exporter cannot be built the runtime
/// silently falls back to the no-op path and prints a single
/// diagnostic line to stderr.
pub fn init_from_env() {
    if TELEMETRY_ENABLED.load(Ordering::Acquire) {
        return;
    }
    #[cfg(feature = "otlp")]
    {
        if let Ok(endpoint) = std::env::var("MTY_OTLP_ENDPOINT") {
            let protocol = std::env::var("MTY_OTLP_PROTOCOL").unwrap_or_else(|_| "grpc".into());
            let sample = std::env::var("MTY_OTLP_SAMPLE_RATE")
                .ok()
                .and_then(|s| s.parse::<f64>().ok());
            if otlp_provider::install(&endpoint, &protocol, sample) {
                TELEMETRY_ENABLED.store(true, Ordering::Release);
                return;
            }
        }
    }
    // No OTLP requested (or feature disabled). Mark as initialised
    // so the no-op path is taken and we don't retry.
    TELEMETRY_ENABLED.store(true, Ordering::Release);
}

/// Returns `true` when [`init_from_env`] saw a usable
/// `MTY_OTLP_ENDPOINT` and successfully installed a tracer provider.
/// Span guards key off this to stay zero-cost when disabled.
pub fn is_enabled() -> bool {
    #[cfg(feature = "otlp")]
    {
        TELEMETRY_ENABLED.load(Ordering::Acquire) && otlp_provider::is_installed()
    }
    #[cfg(not(feature = "otlp"))]
    {
        false
    }
}

/// Flush pending spans and shut down the tracer provider. Safe to
/// call even if [`init_from_env`] was never called or the provider
/// was never installed.
pub fn shutdown() {
    #[cfg(feature = "otlp")]
    {
        otlp_provider::shutdown();
    }
    TELEMETRY_ENABLED.store(false, Ordering::Release);
}

#[cfg(feature = "otlp")]
mod otlp_provider {
    //! Lazy-initialised tracer provider keyed off `MTY_OTLP_ENDPOINT`.
    //!
    //! Kept private to the telemetry module so OTel imports never leak
    //! into user crates. The pre-existing `crate::otlp` module is the
    //! v0.3 event-emitter bridge and stays untouched; this module owns
    //! the v0.16 span-instrumentation provider.

    use opentelemetry::global;
    use opentelemetry::KeyValue;
    use opentelemetry_otlp::{SpanExporter, WithExportConfig};
    use opentelemetry_sdk::trace::{Sampler, TracerProvider as SdkTracerProvider};
    use opentelemetry_sdk::Resource;
    use parking_lot::Mutex;
    use std::sync::OnceLock;

    static PROVIDER: OnceLock<Mutex<Option<SdkTracerProvider>>> = OnceLock::new();

    fn slot() -> &'static Mutex<Option<SdkTracerProvider>> {
        PROVIDER.get_or_init(|| Mutex::new(None))
    }

    /// Try to build + install a tracer provider. Returns true on
    /// success. Errors are swallowed (printed to stderr) so that a
    /// missing collector never aborts startup.
    pub fn install(endpoint: &str, protocol: &str, sample_rate: Option<f64>) -> bool {
        let exporter = match protocol {
            "http" | "http/protobuf" => SpanExporter::builder()
                .with_http()
                .with_endpoint(endpoint)
                .build(),
            _ => SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .build(),
        };
        let exporter = match exporter {
            Ok(e) => e,
            Err(err) => {
                eprintln!("mighty: OTLP span exporter init failed: {err}");
                return false;
            }
        };

        let resource = Resource::new(vec![
            KeyValue::new("service.name", "mighty-runtime"),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ]);
        let sampler = match sample_rate {
            Some(r) if (0.0..=1.0).contains(&r) => Sampler::TraceIdRatioBased(r),
            _ => Sampler::AlwaysOn,
        };
        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
            .with_resource(resource)
            .with_sampler(sampler)
            .build();
        global::set_tracer_provider(provider.clone());
        *slot().lock() = Some(provider);
        true
    }

    pub fn is_installed() -> bool {
        slot().lock().is_some()
    }

    pub fn shutdown() {
        let taken = slot().lock().take();
        if let Some(p) = taken {
            // Best-effort flush, then shut down.
            for res in p.force_flush() {
                let _ = res;
            }
            let _ = p.shutdown();
        }
    }

    /// Test/debug accessor — returns a tracer named for the runtime.
    pub fn tracer(name: &'static str) -> Option<opentelemetry::global::BoxedTracer> {
        if !is_installed() {
            return None;
        }
        Some(global::tracer(name))
    }
}

#[cfg(feature = "otlp")]
pub(crate) use otlp_provider::tracer as global_tracer;

#[cfg(not(feature = "otlp"))]
pub(crate) fn global_tracer(_name: &'static str) -> Option<()> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_when_uninit_is_noop() {
        // Should not panic even if nothing was initialised.
        shutdown();
    }

    #[test]
    fn is_enabled_false_by_default() {
        // No env, no init call → not enabled.
        assert!(!is_enabled());
    }
}
