//! Stable diagnostic codes. Once assigned, NEVER renumber.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiagCode(pub u16);

impl DiagCode {
    pub const fn new(n: u16) -> Self { DiagCode(n) }
    pub fn as_str(&self) -> String { format!("SD{:04}", self.0) }
}

// Lex/parse: SD0001..SD0999
pub const UNEXPECTED_TOKEN: DiagCode      = DiagCode::new(1);
pub const UNTERMINATED_STRING: DiagCode   = DiagCode::new(2);
pub const INVALID_ESCAPE: DiagCode        = DiagCode::new(3);
pub const UNKNOWN_DURATION_UNIT: DiagCode = DiagCode::new(4);
pub const EXPECTED_ITEM: DiagCode         = DiagCode::new(10);
pub const EXPECTED_EXPR: DiagCode         = DiagCode::new(11);
pub const MISMATCHED_DELIMITER: DiagCode  = DiagCode::new(12);
pub const DUPLICATE_ON_HANDLER: DiagCode  = DiagCode::new(20);
pub const PUB_NEEDS_RETURN_TYPE: DiagCode = DiagCode::new(21);
pub const DEPTH_LIMIT_EXCEEDED: DiagCode  = DiagCode::new(30);

// HIR: SD1001..SD1999
pub const UNRESOLVED_NAME: DiagCode       = DiagCode::new(1001);
pub const USE_RESOLVES_TO_NOTHING: DiagCode = DiagCode::new(1002);
