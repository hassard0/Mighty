use crate::codes::DiagCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

#[derive(Debug, Clone)]
pub struct Label {
    pub start: usize,
    pub end: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub severity: Severity,
    pub primary: Label,
    pub secondary: Vec<Label>,
    pub notes: Vec<String>,
    pub helps: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: DiagCode, primary: Label) -> Self {
        Self {
            code,
            severity: Severity::Error,
            primary,
            secondary: vec![],
            notes: vec![],
            helps: vec![],
        }
    }
    pub fn with_note(mut self, n: impl Into<String>) -> Self {
        self.notes.push(n.into());
        self
    }
    pub fn with_help(mut self, h: impl Into<String>) -> Self {
        self.helps.push(h.into());
        self
    }
    pub fn with_secondary(mut self, l: Label) -> Self {
        self.secondary.push(l);
        self
    }
}
