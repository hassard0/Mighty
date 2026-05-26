//! Parse incoming wasm bytes to extract Mighty-embedded metadata.
//!
//! v0.21 Tier 1.5 — the runtime needs two pieces of information about
//! a replacement wasm module *before* it swaps the live agent:
//!
//! 1. **Agent type name.** The control-socket caller already names the
//!    agent type in the JSON envelope, but we cross-check it against
//!    the embedded value so a misnamed `mty reload` invocation fails
//!    fast (`MT5065`) rather than swapping the wrong agent's state.
//! 2. **Schema hash.** Embedded by the codegen as `__mty_schema_hash`
//!    (little-endian `u64`). The swap pipeline compares it to the
//!    snapshot's source hash before deserialising — same fail-fast
//!    behaviour the v0.20 trait-based check already provides for
//!    `SameProgram` reloads.
//!
//! ## Custom-section format
//!
//! The codegen emits two custom sections at module-tail position:
//!
//! - `__mty_agent_type` — UTF-8 bytes of the agent's struct name
//!   (e.g. `Echo`). No length prefix; the section length is the
//!   string length.
//! - `__mty_schema_hash` — exactly 8 bytes, little-endian `u64`.
//!
//! Older modules (pre v0.16) omit these sections. The loader reports
//! a clean error rather than silently swapping a stranger's wasm into
//! the agent slot.
//!
//! ## Why parse here and not in the codegen crate
//!
//! The codegen crate is off-limits to the v0.21 reload slice + we
//! want the runtime side of the contract to live next to the consumer
//! anyway (single source of truth for the section-name constants).
//! The codegen will adopt the same constants when it grows the
//! emit-side helpers in v0.22.

use wasmparser::{Parser, Payload};

/// Custom-section name that carries the UTF-8 agent type identifier.
pub const SECTION_AGENT_TYPE: &str = "__mty_agent_type";

/// Custom-section name that carries the 8-byte little-endian schema
/// hash (`u64`).
pub const SECTION_SCHEMA_HASH: &str = "__mty_schema_hash";

/// Wasm-magic prefix `\0asm` + version `1.0`.
const WASM_MAGIC_LEN: usize = 8;

/// Parsed payload extracted from a replacement wasm module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedAgentModule {
    /// The raw wasm bytes — owned so the loader's caller can keep
    /// holding the byte slice past the parser's lifetime.
    pub wasm: Vec<u8>,
    /// Identifier extracted from the `__mty_agent_type` custom section.
    pub agent_type: String,
    /// `u64` extracted from the `__mty_schema_hash` custom section.
    pub schema_hash: u64,
}

/// Failure modes for [`load_agent_module`]. Maps to `MT506x` codes
/// alongside the swap pipeline's errors — see [`WasmLoadError::diag_code`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum WasmLoadError {
    #[error("wasm bytes too short: {0} B (need at least 8 for the magic)")]
    TooShort(usize),

    #[error("wasm magic mismatch: expected \"\\0asm\\x01\\x00\\x00\\x00\", got {0:02x?}")]
    BadMagic([u8; 8]),

    #[error("wasm parse error at byte {offset}: {message}")]
    Parse { offset: usize, message: String },

    #[error(
        "missing required custom section `{0}` — module was not built by \
         a Mighty codegen that targets hot-reload (v0.16+)"
    )]
    MissingSection(&'static str),

    #[error("custom section `__mty_agent_type` is not valid UTF-8: {0}")]
    AgentTypeNotUtf8(String),

    #[error(
        "custom section `__mty_agent_type` is empty — expected the \
         agent struct name"
    )]
    AgentTypeEmpty,

    #[error(
        "custom section `__mty_schema_hash` has wrong length: expected 8 \
         bytes (LE u64), got {0}"
    )]
    SchemaHashBadLen(usize),
}

impl WasmLoadError {
    /// Map to the `MT506x` diagnostic family. Distinct codes from the
    /// swap pipeline so CLI output can distinguish a malformed module
    /// (caller bug) from a deserialise failure (state-shape drift).
    pub fn diag_code(&self) -> &'static str {
        match self {
            WasmLoadError::TooShort(_) | WasmLoadError::BadMagic(_) => "MT5066",
            WasmLoadError::Parse { .. } => "MT5067",
            WasmLoadError::MissingSection(_) => "MT5068",
            WasmLoadError::AgentTypeNotUtf8(_)
            | WasmLoadError::AgentTypeEmpty
            | WasmLoadError::SchemaHashBadLen(_) => "MT506A",
        }
    }
}

/// Parse `bytes` and extract the Mighty-embedded agent type + schema
/// hash. Returns an owned [`LoadedAgentModule`] so the caller can
/// drop the source slice immediately.
///
/// The parser is one-pass — it stops scanning custom sections as soon
/// as both metadata fields are populated, so a large code section past
/// the metadata isn't fully walked. Total bound is O(n) on module size
/// regardless.
///
/// # Errors
///
/// Returns [`WasmLoadError`] for malformed or non-Mighty wasm. The
/// caller (the swap pipeline) translates the error into the runtime's
/// `MT506x` family.
pub fn load_agent_module(bytes: &[u8]) -> Result<LoadedAgentModule, WasmLoadError> {
    if bytes.len() < WASM_MAGIC_LEN {
        return Err(WasmLoadError::TooShort(bytes.len()));
    }
    // `\0asm\x01\x00\x00\x00`
    if &bytes[..4] != b"\0asm" || bytes[4..8] != [0x01u8, 0x00, 0x00, 0x00] {
        let mut magic = [0u8; 8];
        magic.copy_from_slice(&bytes[..8]);
        return Err(WasmLoadError::BadMagic(magic));
    }

    let mut agent_type: Option<String> = None;
    let mut schema_hash: Option<u64> = None;

    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(|e| WasmLoadError::Parse {
            offset: e.offset(),
            message: e.message().to_string(),
        })?;

        if let Payload::CustomSection(reader) = payload {
            match reader.name() {
                n if n == SECTION_AGENT_TYPE => {
                    let raw = reader.data();
                    if raw.is_empty() {
                        return Err(WasmLoadError::AgentTypeEmpty);
                    }
                    let s = std::str::from_utf8(raw)
                        .map_err(|e| WasmLoadError::AgentTypeNotUtf8(e.to_string()))?
                        .to_string();
                    if s.is_empty() {
                        return Err(WasmLoadError::AgentTypeEmpty);
                    }
                    agent_type = Some(s);
                }
                n if n == SECTION_SCHEMA_HASH => {
                    let raw = reader.data();
                    if raw.len() != 8 {
                        return Err(WasmLoadError::SchemaHashBadLen(raw.len()));
                    }
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(raw);
                    schema_hash = Some(u64::from_le_bytes(buf));
                }
                _ => {}
            }
            if agent_type.is_some() && schema_hash.is_some() {
                break;
            }
        }
    }

    let agent_type = agent_type.ok_or(WasmLoadError::MissingSection(SECTION_AGENT_TYPE))?;
    let schema_hash = schema_hash.ok_or(WasmLoadError::MissingSection(SECTION_SCHEMA_HASH))?;

    Ok(LoadedAgentModule {
        wasm: bytes.to_vec(),
        agent_type,
        schema_hash,
    })
}

/// Synthesize a minimal wasm module containing only the Mighty
/// custom sections. Used by unit + integration tests to exercise the
/// loader without dragging the full codegen crate (off-limits to the
/// reload slice).
///
/// The emitted module has no functions or exports — the v0.21 reload
/// pipeline never executes the wasm (the interpreter still owns
/// dispatch); it only inspects the metadata and stashes the bytes in
/// the per-agent program slot.
#[cfg(test)]
fn synthesize_test_module(agent_type: &str, schema_hash: u64) -> Vec<u8> {
    let mut module = wasm_encoder::Module::new();
    module.section(&wasm_encoder::CustomSection {
        name: std::borrow::Cow::Borrowed(SECTION_AGENT_TYPE),
        data: std::borrow::Cow::Borrowed(agent_type.as_bytes()),
    });
    let hash_bytes = schema_hash.to_le_bytes();
    module.section(&wasm_encoder::CustomSection {
        name: std::borrow::Cow::Borrowed(SECTION_SCHEMA_HASH),
        data: std::borrow::Cow::Borrowed(&hash_bytes),
    });
    module.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_synth_module() {
        let bytes = synthesize_test_module("Echo", 0xDEAD_BEEF_CAFE_F00D);
        let loaded = load_agent_module(&bytes).expect("load ok");
        assert_eq!(loaded.agent_type, "Echo");
        assert_eq!(loaded.schema_hash, 0xDEAD_BEEF_CAFE_F00D);
        assert_eq!(loaded.wasm, bytes);
    }

    #[test]
    fn rejects_too_short_bytes() {
        let err = load_agent_module(b"\0asm").unwrap_err();
        assert!(matches!(err, WasmLoadError::TooShort(4)));
        assert_eq!(err.diag_code(), "MT5066");
    }

    #[test]
    fn rejects_bad_magic() {
        let err = load_agent_module(b"XXXXXXXX").unwrap_err();
        assert!(matches!(err, WasmLoadError::BadMagic(_)));
        assert_eq!(err.diag_code(), "MT5066");
    }

    #[test]
    fn rejects_module_missing_agent_type() {
        // Hand-rolled: valid wasm magic + only the schema-hash section.
        let mut module = wasm_encoder::Module::new();
        let hash = 0x1234_u64.to_le_bytes();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(SECTION_SCHEMA_HASH),
            data: std::borrow::Cow::Borrowed(&hash),
        });
        let bytes = module.finish();
        let err = load_agent_module(&bytes).unwrap_err();
        match err {
            WasmLoadError::MissingSection(s) => assert_eq!(s, SECTION_AGENT_TYPE),
            other => panic!("expected MissingSection, got {other:?}"),
        }
    }

    #[test]
    fn rejects_module_missing_schema_hash() {
        let mut module = wasm_encoder::Module::new();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(SECTION_AGENT_TYPE),
            data: std::borrow::Cow::Borrowed(b"Echo"),
        });
        let bytes = module.finish();
        let err = load_agent_module(&bytes).unwrap_err();
        match err {
            WasmLoadError::MissingSection(s) => assert_eq!(s, SECTION_SCHEMA_HASH),
            other => panic!("expected MissingSection, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_agent_type() {
        let mut module = wasm_encoder::Module::new();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(SECTION_AGENT_TYPE),
            data: std::borrow::Cow::Borrowed(b""),
        });
        let hash = 0u64.to_le_bytes();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(SECTION_SCHEMA_HASH),
            data: std::borrow::Cow::Borrowed(&hash),
        });
        let bytes = module.finish();
        let err = load_agent_module(&bytes).unwrap_err();
        assert!(matches!(err, WasmLoadError::AgentTypeEmpty));
    }

    #[test]
    fn rejects_wrong_length_schema_hash() {
        let mut module = wasm_encoder::Module::new();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(SECTION_AGENT_TYPE),
            data: std::borrow::Cow::Borrowed(b"Echo"),
        });
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(SECTION_SCHEMA_HASH),
            data: std::borrow::Cow::Borrowed(&[0u8; 4]),
        });
        let bytes = module.finish();
        let err = load_agent_module(&bytes).unwrap_err();
        match err {
            WasmLoadError::SchemaHashBadLen(n) => assert_eq!(n, 4),
            other => panic!("expected SchemaHashBadLen, got {other:?}"),
        }
    }
}
