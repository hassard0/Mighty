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
    Trap {
        code: &'static str,
        message: String,
    },
}

impl RuntimeError {
    /// Map to the SD5xxx diagnostic id used in user-facing messages
    /// and exit-code mapping.
    pub fn diag_code(&self) -> &'static str {
        match self {
            RuntimeError::AgentPanic { .. } => "SD5001",
            RuntimeError::DeadlineExceeded(_) => "SD5011",
            RuntimeError::MailboxFull { .. } => "SD5012",
            RuntimeError::SupervisorEscalated { .. } => "SD5013",
            RuntimeError::RestartLimitExceeded { .. } => "SD5014",
            RuntimeError::BudgetExceeded(_) => "SD5009",
            RuntimeError::CapabilityOutsideSandbox(_) => "SD5015",
            RuntimeError::ExternUnimplemented(_) => "SD5050",
            RuntimeError::AgentNotFound(_) => "SD5021",
            RuntimeError::HandlerNotFound { .. } => "SD5020",
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
            (RuntimeError::AgentPanic { msg: "x".into() }, "SD5001"),
            (
                RuntimeError::DeadlineExceeded(Duration::from_millis(10)),
                "SD5011",
            ),
            (
                RuntimeError::MailboxFull {
                    agent: "A".into(),
                },
                "SD5012",
            ),
            (
                RuntimeError::SupervisorEscalated {
                    child: "c".into(),
                },
                "SD5013",
            ),
            (
                RuntimeError::RestartLimitExceeded {
                    child: "c".into(),
                },
                "SD5014",
            ),
            (RuntimeError::BudgetExceeded("cpu".into()), "SD5009"),
            (
                RuntimeError::CapabilityOutsideSandbox("/etc".into()),
                "SD5015",
            ),
            (RuntimeError::ExternUnimplemented("foo".into()), "SD5050"),
            (RuntimeError::AgentNotFound("A".into()), "SD5021"),
            (
                RuntimeError::HandlerNotFound {
                    agent: "A".into(),
                    msg: "M".into(),
                },
                "SD5020",
            ),
            (
                RuntimeError::Trap {
                    code: "SD5005",
                    message: "u".into(),
                },
                "SD5005",
            ),
        ];
        for (err, code) in cases {
            assert_eq!(err.diag_code(), code, "wrong code for {err:?}");
        }
    }
}
