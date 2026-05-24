//! Telemetry JSON line emitter (OTLP-flavoured, see A38).

use parking_lot::Mutex;
use std::io::Write;
use std::sync::Arc;
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
}

impl TelemetrySink {
    pub fn buffer() -> (Self, Arc<Mutex<Vec<String>>>) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        (TelemetrySink::Buffer(buf.clone()), buf)
    }

    pub fn from_env() -> Self {
        match std::env::var("STARDUST_TRACE").as_deref() {
            Ok("stderr") => TelemetrySink::Stderr,
            Ok(v) if v.starts_with("file:") => {
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
        }
    }
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
}
