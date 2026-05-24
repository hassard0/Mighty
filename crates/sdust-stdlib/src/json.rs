//! `std.json` — JSON value, parser, and emitter.
//!
//! Wraps [`serde_json`] but exposes a Stardust-shaped `Json` enum so
//! prelude bindings (`Json::Null`, `Json::Bool(b)`, ...) line up with
//! what user code constructs and destructures.

use serde_json::Value as SVal;
use std::collections::BTreeMap;

/// Stardust's `Json` value. Mirrors `serde_json::Value` but uses a
/// deterministically-ordered map (BTreeMap by key) so encoded output is
/// stable across runs — which matters for `std.test` snapshots and
/// content-addressed caches.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// All numerics flow through f64 to match Stardust's surface
    /// `Num` shape; lossless round-trip of u64/i64 large values is
    /// flagged as a known gap (`STDLIB_V0_2_NOTES.md`).
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

#[derive(Debug, thiserror::Error)]
pub enum JsonErr {
    #[error("json parse: {0}")]
    Parse(String),
    #[error("json encode: {0}")]
    Encode(String),
}

impl Json {
    /// Recursively convert a `serde_json::Value` into a `Json`.
    pub fn from_serde(v: SVal) -> Self {
        match v {
            SVal::Null => Json::Null,
            SVal::Bool(b) => Json::Bool(b),
            SVal::Number(n) => Json::Num(n.as_f64().unwrap_or(0.0)),
            SVal::String(s) => Json::Str(s),
            SVal::Array(xs) => Json::Arr(xs.into_iter().map(Json::from_serde).collect()),
            SVal::Object(m) => {
                let mut out = BTreeMap::new();
                for (k, v) in m {
                    out.insert(k, Json::from_serde(v));
                }
                Json::Obj(out)
            }
        }
    }

    /// Inverse of [`Json::from_serde`]. Lossy for numeric range edges
    /// outside f64's safe-integer band; documented in `STDLIB_V0_2_NOTES.md`.
    pub fn to_serde(&self) -> SVal {
        match self {
            Json::Null => SVal::Null,
            Json::Bool(b) => SVal::Bool(*b),
            Json::Num(n) => serde_json::Number::from_f64(*n)
                .map(SVal::Number)
                .unwrap_or(SVal::Null),
            Json::Str(s) => SVal::String(s.clone()),
            Json::Arr(xs) => SVal::Array(xs.iter().map(Json::to_serde).collect()),
            Json::Obj(m) => {
                let mut out = serde_json::Map::new();
                for (k, v) in m {
                    out.insert(k.clone(), v.to_serde());
                }
                SVal::Object(out)
            }
        }
    }
}

/// Parse a JSON document. Returns `Json` on success or `JsonErr` on
/// malformed input.
pub fn parse(s: &str) -> Result<Json, JsonErr> {
    let v: SVal = serde_json::from_str(s).map_err(|e| JsonErr::Parse(e.to_string()))?;
    Ok(Json::from_serde(v))
}

/// Encode a `Json` to a compact (no whitespace) string.
pub fn encode(v: &Json) -> Result<String, JsonErr> {
    serde_json::to_string(&v.to_serde()).map_err(|e| JsonErr::Encode(e.to_string()))
}

/// Encode a `Json` to a pretty-printed (2-space indented) string.
pub fn encode_pretty(v: &Json) -> Result<String, JsonErr> {
    serde_json::to_string_pretty(&v.to_serde()).map_err(|e| JsonErr::Encode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_primitives() {
        assert_eq!(parse("null").unwrap(), Json::Null);
        assert_eq!(parse("true").unwrap(), Json::Bool(true));
        assert_eq!(parse("42").unwrap(), Json::Num(42.0));
        assert_eq!(parse("\"hi\"").unwrap(), Json::Str("hi".into()));
    }

    #[test]
    fn encode_compact_is_stable() {
        let mut m = BTreeMap::new();
        m.insert("b".into(), Json::Num(2.0));
        m.insert("a".into(), Json::Num(1.0));
        let s = encode(&Json::Obj(m)).unwrap();
        // BTreeMap iterates sorted → "a" precedes "b" regardless of
        // insertion order. serde_json emits a float that round-trips
        // (either "1" or "1.0" depending on Number repr); we only
        // assert key ordering.
        let a_idx = s.find("\"a\"").expect("a key");
        let b_idx = s.find("\"b\"").expect("b key");
        assert!(a_idx < b_idx, "a should sort before b: {s}");
    }

    #[test]
    fn parse_err_on_garbage() {
        assert!(parse("not json").is_err());
    }
}
