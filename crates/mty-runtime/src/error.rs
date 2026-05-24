//! Runtime error taxonomy. Maps to SD5xxx diagnostics.

use std::time::Duration;

pub type RuntimeResult<T> = Result<T, RuntimeError>;

#[derive(Debug, Clone, thiserror::Error)]
pub enum RuntimeError {
    #[error("agent panicked: {msg}")]
    AgentPanic { msg: String },

    #[error("deadline exceeded after {0:?}")]
    DeadlineExceeded(Duration),

    #[error("mailbox full (agent {agent})")]
    MailboxFull { agent: String },

    #[error("supervisor escalated: {child}")]
    SupervisorEscalated { child: String },

    #[error("restart limit exceeded for {child}")]
    RestartLimitExceeded { child: String },

    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),

    #[error("capability outside sandbox: {0}")]
    CapabilityOutsideSandbox(String),

    #[error("extern fn unimplemented: {0}")]
    ExternUnimplemented(String),

    #[error("agent not found: {0}")]
    AgentNotFound(String),

    #[error("handler not found: {agent}.{msg}")]
    HandlerNotFound { agent: String, msg: String },

    #[error("trap: {code} {message}")]
    Trap { code: &'static str, message: String },
}

impl RuntimeError {
    /// Map to the SD5xxx diagnostic id used in user-facing messages
    /// and exit-code mapping.
    pub fn diag_code(&self) -> &'static str {
        match self {
            RuntimeError::AgentPanic { .. } => "MT5001",
            RuntimeError::DeadlineExceeded(_) => "MT5011",
            RuntimeError::MailboxFull { .. } => "MT5012",
            RuntimeError::SupervisorEscalated { .. } => "MT5013",
            RuntimeError::RestartLimitExceeded { .. } => "MT5014",
            RuntimeError::BudgetExceeded(_) => "MT5009",
            RuntimeError::CapabilityOutsideSandbox(_) => "MT5015",
            RuntimeError::ExternUnimplemented(_) => "MT5050",
            RuntimeError::AgentNotFound(_) => "MT5021",
            RuntimeError::HandlerNotFound { .. } => "MT5020",
            RuntimeError::Trap { code, .. } => code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_cover_all_variants() {
        let cases = [
            (RuntimeError::AgentPanic { msg: "x".into() }, "MT5001"),
            (
                RuntimeError::DeadlineExceeded(Duration::from_millis(10)),
                "MT5011",
            ),
            (RuntimeError::MailboxFull { agent: "A".into() }, "MT5012"),
            (
                RuntimeError::SupervisorEscalated { child: "c".into() },
                "MT5013",
            ),
            (
                RuntimeError::RestartLimitExceeded { child: "c".into() },
                "MT5014",
            ),
            (RuntimeError::BudgetExceeded("cpu".into()), "MT5009"),
            (
                RuntimeError::CapabilityOutsideSandbox("/etc".into()),
                "MT5015",
            ),
            (RuntimeError::ExternUnimplemented("foo".into()), "MT5050"),
            (RuntimeError::AgentNotFound("A".into()), "MT5021"),
            (
                RuntimeError::HandlerNotFound {
                    agent: "A".into(),
                    msg: "M".into(),
                },
                "MT5020",
            ),
            (
                RuntimeError::Trap {
                    code: "MT5005",
                    message: "u".into(),
                },
                "MT5005",
            ),
        ];
        for (err, code) in cases {
            assert_eq!(err.diag_code(), code, "wrong code for {err:?}");
        }
    }
}
