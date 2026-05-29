//! Structured events from user-land agent code.
//!
//! Roadmap Tier 1.3: `agent.event(name, &[(key, value)])` emits a
//! typed event into the OTel pipeline (or stdout when no OTLP is
//! configured). The intent is to mirror the
//! `wasi:logging@0.2.x` shape so the same call can later be lowered
//! to a Component Model import without changing user code.
//!
//! Behaviour:
//!
//! * Inside a handler dispatch (when [`super::spans::HANDLER_SPAN`]
//!   is set), the event is added to the active handler span.
//! * Outside a handler (e.g. called from `main` or from a unit
//!   test), the event is written as a single line to stdout. We use
//!   the same JSON shape as the existing JSON-line sink so log
//!   tooling already wired up for `MTY_TRACE=stderr` (or the legacy
//!   `STARDUST_TRACE=stderr`) keeps working without changes.

use super::spans::{current_handler_context, SpanContext};

/// Emit a structured event. See module docs for routing rules.
pub fn agent_event(name: &str, fields: &[(&str, &str)]) {
    let ctx = current_handler_context();
    if ctx.is_some() {
        ctx.add_event(name, fields);
        return;
    }
    // Stdout fallback: keep a stable, parseable shape so log
    // aggregators can pick the events up even without OTLP wired in.
    println!("{}", format_event_line(name, fields));
}

/// Returns the active span context (re-exported via the parent
/// module so user code that wants to add multiple events without
/// repeated task-local lookups can keep the handle).
pub fn current_handler_context_handle() -> SpanContext {
    current_handler_context()
}

/// Format an event into the stdout-fallback line. Public for tests.
pub(crate) fn format_event_line(name: &str, fields: &[(&str, &str)]) -> String {
    let mut s = String::with_capacity(64 + name.len() + fields.len() * 16);
    s.push_str(r#"{"kind":"agent_event","name":""#);
    s.push_str(&esc(name));
    s.push('"');
    if !fields.is_empty() {
        s.push_str(r#","fields":{"#);
        let mut first = true;
        for (k, v) in fields {
            if !first {
                s.push(',');
            }
            first = false;
            s.push('"');
            s.push_str(&esc(k));
            s.push_str(r#"":""#);
            s.push_str(&esc(v));
            s.push('"');
        }
        s.push('}');
    }
    s.push('}');
    s
}

fn esc(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_event_line_no_fields() {
        let s = format_event_line("started", &[]);
        assert_eq!(s, r#"{"kind":"agent_event","name":"started"}"#);
    }

    #[test]
    fn format_event_line_with_fields() {
        let s = format_event_line("hit", &[("path", "/a"), ("status", "200")]);
        assert!(s.contains(r#""name":"hit""#));
        assert!(s.contains(r#""path":"/a""#));
        assert!(s.contains(r#""status":"200""#));
    }

    #[test]
    fn format_event_line_escapes_quotes_and_backslashes() {
        let s = format_event_line(r#"a"b\c"#, &[]);
        assert!(s.contains(r#"a\"b\\c"#));
    }

    #[test]
    fn agent_event_outside_handler_does_not_panic() {
        // We're outside a handler-task scope here. The stdout
        // fallback is exercised; we just need to confirm it doesn't
        // panic. Capturing stdout in a unit test is awkward — the
        // dedicated integration test verifies the line shape.
        agent_event("noop", &[("k", "v")]);
    }
}
