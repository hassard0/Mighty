//! OTLP wire-format export for runtime telemetry (v0.3, closes A38).
//!
//! Slice 7 emitted JSON lines that were *shaped* like OpenTelemetry
//! attributes but were not OTLP wire frames. v0.3 adds a real
//! `opentelemetry_sdk::TracerProvider` configured with
//! `opentelemetry_otlp::SpanExporter`. When the env var
//! `MTY_OTLP_ENDPOINT` (or the legacy `STARDUST_OTLP_ENDPOINT`)
//! is set (e.g. `http://localhost:4317`), the runtime forwards every
//! telemetry event to the OTLP collector; otherwise the legacy
//! JSON-line emitter is used unchanged.
//!
//! ## Semantic conventions
//!
//! Mighty uses the `mty.agent.*` attribute namespace (v0.36 T4 —
//! renamed from `stardust.agent.*`; the v1.0 spec freezes this on
//! `mty.*`). Each span also carries a `mty.legacy_name` attribute
//! holding the pre-v0.36 `stardust.*` name so dashboards that filter
//! on the old prefix continue to match during the transition window.
//!
//! | Event            | Span name              | Attributes                                    |
//! |------------------|------------------------|-----------------------------------------------|
//! | TurnStart        | `mty.turn.start`       | `agent`, `msg`                                |
//! | TurnEnd          | `mty.turn.end`         | `agent`, `msg`, `duration_us`                 |
//! | Send             | `mty.send`             | `from`, `to`, `msg`                           |
//! | Ask              | `mty.ask`              | `from`, `to`, `msg`, `deadline_ms?`           |
//! | Reply            | `mty.reply`            | `from`, `msg`, `ok`                           |
//! | Spawn            | `mty.spawn`            | `name`, `agent_id`                            |
//! | Restart          | `mty.restart`          | `supervisor`, `child`, `attempt`              |
//! | BudgetBreach     | `mty.budget_breach`    | `agent`, `kind` (MT5xxx)                      |
//! | Shutdown         | `mty.shutdown`         | (none)                                        |
//!
//! All spans are emitted with `kind = INTERNAL` and a zero duration
//! (point-in-time events). A future v0.4 may pair TurnStart/TurnEnd
//! into a single span with real duration semantics.

use std::sync::Arc;

use opentelemetry::global;
use opentelemetry::trace::{Span, SpanKind, Status, Tracer, TracerProvider as _};
use opentelemetry::KeyValue;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::trace::{Sampler, TracerProvider as SdkTracerProvider};
use opentelemetry_sdk::Resource;

use crate::telemetry::TelemetryEvent;

const TRACER_NAME: &str = "mty-runtime";

/// Handle to an active OTLP exporter. Dropping it shuts down the
/// provider gracefully (flushing pending spans).
#[derive(Debug)]
pub struct OtlpHandle {
    provider: SdkTracerProvider,
}

impl OtlpHandle {
    /// Initialise OTLP export against `endpoint` (e.g.
    /// `http://localhost:4317`). Returns None if the endpoint can't
    /// be reached at startup or the exporter fails to build — we
    /// treat OTLP as best-effort and never fail runtime construction.
    pub fn try_init(endpoint: &str) -> Option<Arc<Self>> {
        // Build the OTLP gRPC exporter. The OTel SDK 0.27 builder
        // exposes `with_endpoint` for the HTTP/gRPC endpoint URL.
        let exporter = match SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
        {
            Ok(e) => e,
            Err(err) => {
                eprintln!("mighty: OTLP exporter init failed: {err}");
                return None;
            }
        };

        let resource = Resource::new(vec![
            KeyValue::new("service.name", "mty-runtime"),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ]);
        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
            .with_resource(resource)
            .with_sampler(Sampler::AlwaysOn)
            .build();
        // Install as global so any future tracer lookup gets ours.
        global::set_tracer_provider(provider.clone());
        Some(Arc::new(Self { provider }))
    }

    /// Emit a single telemetry event as an OTLP span.
    pub fn emit(&self, ev: &TelemetryEvent) {
        let tracer = self.provider.tracer(TRACER_NAME);
        let (name, mut attrs) = event_to_span(ev);
        // Carry the legacy `stardust.*` name as an attribute so
        // dashboards that filtered on the old prefix continue to
        // match the new spans during the v0.36 → v1.0 transition.
        attrs.push(KeyValue::new("mty.legacy_name", legacy_span_name(name)));
        let mut span = tracer
            .span_builder(name)
            .with_kind(SpanKind::Internal)
            .with_attributes(attrs)
            .start(&tracer);
        // Most events are point-in-time. We immediately mark Ok and
        // end the span; TurnEnd already carries duration_us as an attr.
        span.set_status(Status::Ok);
        span.end();
    }

    /// Force-flush pending spans (used by tests + shutdown).
    pub fn flush(&self) {
        for res in self.provider.force_flush() {
            let _ = res;
        }
    }
}

impl Drop for OtlpHandle {
    fn drop(&mut self) {
        // shutdown returns Result in 0.27; ignore the error so drop
        // never panics.
        let _ = self.provider.shutdown();
    }
}

/// Map the new `mty.*` span name to the pre-v0.36 `stardust.*`
/// equivalent so we can include it as a `mty.legacy_name` attribute.
pub(crate) fn legacy_span_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("mty.") {
        format!("stardust.{}", rest)
    } else {
        name.to_string()
    }
}

/// Translate a `TelemetryEvent` to the (span_name, attributes) pair
/// per the semantic convention table above.
fn event_to_span(ev: &TelemetryEvent) -> (&'static str, Vec<KeyValue>) {
    match ev {
        TelemetryEvent::TurnStart { agent, msg } => (
            "mty.turn.start",
            vec![
                KeyValue::new("agent", agent.clone()),
                KeyValue::new("msg", msg.clone()),
            ],
        ),
        TelemetryEvent::TurnEnd {
            agent,
            msg,
            duration_us,
        } => (
            "mty.turn.end",
            vec![
                KeyValue::new("agent", agent.clone()),
                KeyValue::new("msg", msg.clone()),
                KeyValue::new("duration_us", *duration_us as i64),
            ],
        ),
        TelemetryEvent::Send { from, to, msg } => (
            "mty.send",
            vec![
                KeyValue::new("from", from.clone()),
                KeyValue::new("to", to.clone()),
                KeyValue::new("msg", msg.clone()),
            ],
        ),
        TelemetryEvent::Ask {
            from,
            to,
            msg,
            deadline_ms,
        } => {
            let mut a = vec![
                KeyValue::new("from", from.clone()),
                KeyValue::new("to", to.clone()),
                KeyValue::new("msg", msg.clone()),
            ];
            if let Some(d) = deadline_ms {
                a.push(KeyValue::new("deadline_ms", *d as i64));
            }
            ("mty.ask", a)
        }
        TelemetryEvent::Reply { from, msg, ok } => (
            "mty.reply",
            vec![
                KeyValue::new("from", from.clone()),
                KeyValue::new("msg", msg.clone()),
                KeyValue::new("ok", *ok),
            ],
        ),
        TelemetryEvent::Spawn { name, agent_id } => (
            "mty.spawn",
            vec![
                KeyValue::new("name", name.clone()),
                KeyValue::new("agent_id", *agent_id as i64),
            ],
        ),
        TelemetryEvent::Restart {
            supervisor,
            child,
            attempt,
        } => (
            "mty.restart",
            vec![
                KeyValue::new("supervisor", supervisor.clone()),
                KeyValue::new("child", child.clone()),
                KeyValue::new("attempt", *attempt as i64),
            ],
        ),
        TelemetryEvent::BudgetBreach { agent, kind } => (
            "mty.budget_breach",
            vec![
                KeyValue::new("agent", agent.clone()),
                KeyValue::new("kind", kind.clone()),
            ],
        ),
        TelemetryEvent::Shutdown => ("mty.shutdown", vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_span_name_maps_known_prefixes() {
        assert_eq!(legacy_span_name("mty.turn.start"), "stardust.turn.start");
        assert_eq!(legacy_span_name("mty.send"), "stardust.send");
        assert_eq!(legacy_span_name("mty.shutdown"), "stardust.shutdown");
    }

    #[test]
    fn legacy_span_name_passes_through_unknown() {
        assert_eq!(legacy_span_name("other.event"), "other.event");
    }
}
