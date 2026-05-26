//! RAII span wrappers for the v0.16 OpenTelemetry agent
//! instrumentation (roadmap Tier 1.2).
//!
//! Every helper is a no-op when telemetry is disabled (no
//! `MTY_OTLP_ENDPOINT`, or built without the `otlp` feature). The
//! helpers must stay cheap on the hot path — that means:
//!
//! * No allocation on the disabled path.
//! * No global mutex acquisitions on the disabled path.
//! * The returned guards are tiny structs whose `Drop` is a single
//!   atomic-load check.
//!
//! The active **handler** span is stashed in a `tokio::task_local!`
//! so that:
//!
//! 1. User code running inside `agent_event(...)` can find its
//!    parent span context.
//! 2. Nested calls inside a handler (e.g. `ask`) automatically
//!    become children of the handler span.

use std::sync::Arc;

/// Lightweight handle to an in-flight span. The variants intentionally
/// have no payload when telemetry is disabled so the disabled path
/// never touches OTel types.
#[derive(Clone, Default)]
pub struct SpanContext {
    inner: Option<Arc<SpanInner>>,
}

impl SpanContext {
    /// Empty context — used as the task-local default and returned
    /// by helpers on the disabled path.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_some(&self) -> bool {
        self.inner.is_some()
    }

    /// Add a typed event to this span (called by
    /// [`crate::telemetry::events::agent_event`]). No-op when the
    /// context is empty.
    pub fn add_event(&self, _name: &str, _fields: &[(&str, &str)]) {
        #[cfg(feature = "otlp")]
        {
            if let Some(inner) = &self.inner {
                inner.add_event(_name, _fields);
            }
        }
    }
}

impl std::fmt::Debug for SpanContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpanContext")
            .field("active", &self.inner.is_some())
            .finish()
    }
}

#[cfg(feature = "otlp")]
struct SpanInner {
    // Held behind a Mutex<Option<_>> because OTel's BoxedSpan needs
    // &mut for `add_event` / `end`. Option lets the guard drop set it
    // to None before the span itself is dropped, so we never call
    // `add_event` on an ended span.
    span: parking_lot::Mutex<Option<opentelemetry::global::BoxedSpan>>,
}

#[cfg(feature = "otlp")]
impl SpanInner {
    fn add_event(&self, name: &str, fields: &[(&str, &str)]) {
        use opentelemetry::trace::Span;
        use opentelemetry::KeyValue;
        let mut guard = self.span.lock();
        if let Some(span) = guard.as_mut() {
            let attrs: Vec<KeyValue> = fields
                .iter()
                .map(|(k, v)| KeyValue::new((*k).to_string(), (*v).to_string()))
                .collect();
            span.add_event(name.to_string(), attrs);
        }
    }
    fn end(&self) {
        use opentelemetry::trace::Span;
        if let Some(mut span) = self.span.lock().take() {
            span.end();
        }
    }
}

#[cfg(not(feature = "otlp"))]
struct SpanInner;
#[cfg(not(feature = "otlp"))]
impl SpanInner {
    fn end(&self) {}
}

tokio::task_local! {
    /// Currently-active handler-span for the running tokio task. Set
    /// by [`span_handler`] and read by [`current_handler_context`]
    /// (re-exported via the parent module). Outside a handler task
    /// the task-local is uninitialised; callers MUST use
    /// `try_with` / the `current_handler_context()` helper, never
    /// the bare `get`.
    pub(crate) static HANDLER_SPAN: SpanContext;
}

/// Returns the current handler-span context if one is active,
/// otherwise an empty context.
pub fn current_handler_context() -> SpanContext {
    HANDLER_SPAN.try_with(|ctx| ctx.clone()).unwrap_or_default()
}

/// Open a span for `Runtime::spawn`. The returned guard ends the span
/// when dropped.
///
/// Span name: `agent.spawn`. Attributes: `agent.type` (the agent type
/// name).
pub fn span_spawn(agent_type: &str) -> SpawnGuard {
    SpawnGuard {
        inner: build_span("agent.spawn", &[("agent.type", agent_type)]),
    }
}

/// Open a short-lived span describing a `send` (fire-and-forget). We
/// model it as a point-in-time span: the message has left the caller
/// but the receiver's handler will get its own [`span_handler`].
///
/// Span name: `agent.send`. Attributes: `protocol.msg`.
pub fn span_send(msg_name: &str) {
    let _g = build_span("agent.send", &[("protocol.msg", msg_name)]);
    // _g dropped immediately — span ends. Receiver-side handler span
    // is opened in [`span_handler`].
}

/// Wrap an `ask` operation in a span that lives for the entire
/// request/reply round-trip. The closure executes inside the span's
/// context.
///
/// Span name: `agent.ask`. Attributes: `protocol.msg`.
pub fn span_ask<F, T>(msg_name: &str, f: F) -> T
where
    F: FnOnce() -> T,
{
    let _g = build_span("agent.ask", &[("protocol.msg", msg_name)]);
    f()
}

/// Open a span for the duration of a handler dispatch and bind it as
/// the current handler-span for downstream calls. The returned guard
/// ends the span on drop.
///
/// This is intended to be used with `tokio::task_local!::scope`:
///
/// ```ignore
/// let guard = span_handler("Echoer", "Ping");
/// HANDLER_SPAN.scope(guard.context(), async move { ... }).await;
/// // guard dropped after the scope returns
/// ```
///
/// In the runtime's current synchronous-handler shape we use the
/// `[scope_with_handler]` helper below which packages the
/// task-local push + span guard into one call.
pub fn span_handler(agent_type: &str, handler_name: &str) -> HandlerGuard {
    HandlerGuard {
        inner: build_span(
            "agent.handler",
            &[("agent.type", agent_type), ("agent.handler", handler_name)],
        ),
    }
}

/// Record a `supervise.restart` event on the currently-active
/// supervisor handler span (when one is active). Otherwise it is a
/// no-op.
pub fn record_restart(reason: &str) {
    let ctx = current_handler_context();
    if ctx.is_some() {
        ctx.add_event("supervise.restart", &[("reason", reason)]);
    } else {
        // Fall back to opening a tiny standalone span so the event
        // still reaches the collector. This matches the disabled
        // path's no-op semantics (build_span returns empty there).
        let _g = build_span("supervise.restart", &[("reason", reason)]);
    }
}

/// Record a `budget.exhausted` event with the given reason. Behaves
/// like [`record_restart`] — attaches to the current handler span if
/// any, else opens a standalone span.
pub fn record_budget_exhausted(reason: &str) {
    let ctx = current_handler_context();
    if ctx.is_some() {
        ctx.add_event("budget.exhausted", &[("reason", reason)]);
    } else {
        let _g = build_span("budget.exhausted", &[("reason", reason)]);
    }
}

/// Guard returned by [`span_spawn`]. Drops the span on drop.
pub struct SpawnGuard {
    inner: SpanContext,
}

impl SpawnGuard {
    pub fn context(&self) -> SpanContext {
        self.inner.clone()
    }
}

impl Drop for SpawnGuard {
    fn drop(&mut self) {
        end_span(&self.inner);
    }
}

/// Guard returned by [`span_handler`]. Drops the span on drop.
pub struct HandlerGuard {
    inner: SpanContext,
}

impl HandlerGuard {
    pub fn context(&self) -> SpanContext {
        self.inner.clone()
    }
}

impl Drop for HandlerGuard {
    fn drop(&mut self) {
        end_span(&self.inner);
    }
}

// ---------------------------------------------------------------------
// internals
// ---------------------------------------------------------------------

fn build_span(_name: &'static str, _attrs: &[(&str, &str)]) -> SpanContext {
    #[cfg(feature = "otlp")]
    {
        use opentelemetry::trace::{Span, SpanKind, Tracer};
        use opentelemetry::KeyValue;
        let Some(tracer) = super::global_tracer("mty-runtime") else {
            return SpanContext::empty();
        };
        let kv: Vec<KeyValue> = _attrs
            .iter()
            .map(|(k, v)| KeyValue::new((*k).to_string(), (*v).to_string()))
            .collect();
        let span = tracer
            .span_builder(_name)
            .with_kind(SpanKind::Internal)
            .with_attributes(kv)
            .start(&tracer);
        // We have to box the span to erase the tracer-specific span
        // type. opentelemetry::global already returns BoxedTracer, and
        // its `start` returns BoxedSpan. Perfect.
        let _ = (Span::span_context(&span),); // touch to satisfy unused-import lints
        SpanContext {
            inner: Some(Arc::new(SpanInner {
                span: parking_lot::Mutex::new(Some(span)),
            })),
        }
    }
    #[cfg(not(feature = "otlp"))]
    {
        SpanContext::empty()
    }
}

fn end_span(ctx: &SpanContext) {
    if let Some(inner) = &ctx.inner {
        inner.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_spawn_returns_guard_no_otlp() {
        // With no OTLP endpoint, the guard is an empty context. Drop
        // must not panic.
        let g = span_spawn("Echoer");
        assert!(!g.context().is_some());
    }

    #[test]
    fn span_ask_runs_closure() {
        let v = span_ask("Ping", || 42);
        assert_eq!(v, 42);
    }

    #[test]
    fn span_send_is_fire_and_forget() {
        // Just verify it doesn't panic.
        span_send("Hit");
    }

    #[test]
    fn record_restart_outside_handler_is_safe() {
        // No active handler-span; helper opens a one-shot span (or
        // is a no-op on the disabled path). Either way: no panic.
        record_restart("child_panicked");
    }

    #[test]
    fn current_handler_context_outside_handler_is_empty() {
        // We're not inside a handler scope here.
        assert!(!current_handler_context().is_some());
    }
}
