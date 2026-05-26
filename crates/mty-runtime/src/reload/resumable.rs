//! `Resumable` trait + schema-hash helpers.
//!
//! v0.20 Tier 1.5 (hot reload). Agents that opt into hot reload
//! implement `Resumable` for their state shape. The runtime calls
//! `to_snapshot` before swapping the wasm module and `from_snapshot`
//! after the new module is loaded, transferring opaque bytes through
//! the swap pipeline.
//!
//! ## Schema compatibility
//!
//! Every implementation publishes a [`Resumable::SCHEMA_HASH`] — a
//! content-addressable digest of the state shape (FNV-1a over
//! field-name + type tag, in lexicographic order; see
//! [`compute_schema_hash`]). The swap pipeline refuses to deserialise
//! a snapshot whose source hash doesn't satisfy
//! [`Resumable::schema_compatible_with`].
//!
//! The default check is bit-equality. v0.21 will widen this so the
//! derived impl can opt into a *forward-compatible* range — fields
//! added at the tail of the struct stay compatible with old
//! snapshots, fields removed from the tail stay compatible with new
//! snapshots, and explicit `migrate_from(old: V1) -> V2` hooks bridge
//! anything that isn't tail-only.
//!
//! ## Wire format
//!
//! The trait is library-shaped: any encoder that round-trips bytes
//! is fine. The reference implementation in [`SnapshotCodec`] uses
//! `ciborium` (already a workspace dep — same wire as the cluster
//! transport). We keep the trait itself encoder-agnostic so user code
//! can derive `Serialize` / `Deserialize` from any serde-compatible
//! format (postcard, bincode, etc.).
//!
//! ## Why a separate trait
//!
//! `Serialize + DeserializeOwned` alone is not enough: the runtime
//! needs the schema hash *before* it deserialises (otherwise an
//! incompatible payload could trap inside the user agent's
//! deserialiser, leaving the runtime to interpret an opaque trap as
//! a generic `MT5005`). `Resumable` makes the hash a const and the
//! compatibility check a free function so the runtime can short-
//! circuit at the start of the swap.

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Result returned by `Resumable` codec helpers. The runtime maps
/// `Err` to the swap pipeline's [`crate::reload::ReloadError`]
/// (which in turn maps to the `MT5060` diagnostic code).
pub type ResumableResult<T> = Result<T, ResumableError>;

/// Encode/decode failures surfaced to the swap pipeline.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ResumableError {
    #[error("snapshot encode failed: {0}")]
    Encode(String),
    #[error("snapshot decode failed: {0}")]
    Decode(String),
    #[error("snapshot too large: {bytes} B exceeds limit {limit} B")]
    TooLarge { bytes: usize, limit: usize },
}

/// State-shape contract for hot reload.
///
/// Implementors expose a content-addressable [`SCHEMA_HASH`] and
/// round-trip the state through opaque bytes. The default codec is
/// `ciborium` (see [`SnapshotCodec::encode`] / [`SnapshotCodec::decode`])
/// but any impl that satisfies the trait works — the runtime treats
/// the snapshot bytes as opaque.
///
/// [`SCHEMA_HASH`]: Resumable::SCHEMA_HASH
pub trait Resumable: Sized + Serialize + DeserializeOwned {
    /// Content-addressable digest of the state shape. Equal whenever
    /// two implementations describe the same struct: same field names
    /// and types, in any source order (the hash is computed in
    /// lexicographic order — see [`compute_schema_hash`]).
    const SCHEMA_HASH: u64;

    /// Decode a snapshot payload. Default impl uses [`SnapshotCodec`].
    fn from_snapshot(bytes: &[u8]) -> ResumableResult<Self> {
        SnapshotCodec::decode(bytes)
    }

    /// Encode the current state to a snapshot payload. Default impl
    /// uses [`SnapshotCodec`].
    fn to_snapshot(&self) -> ResumableResult<Vec<u8>> {
        SnapshotCodec::encode(self)
    }

    /// True iff a snapshot produced by an impl with `other_hash` can
    /// be decoded into `Self`. Default: bit-equal. v0.21 widens this
    /// to a forward-compatible range — see module docs.
    fn schema_compatible_with(other_hash: u64) -> bool {
        Self::SCHEMA_HASH == other_hash
    }
}

/// Reference codec used by the default `Resumable` impl. Public so
/// user code can call it directly when overriding [`Resumable::to_snapshot`]
/// (e.g. to compose with a wrapping framing layer).
pub struct SnapshotCodec;

impl SnapshotCodec {
    /// Encode `value` to ciborium bytes.
    pub fn encode<T: Serialize>(value: &T) -> ResumableResult<Vec<u8>> {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(value, &mut buf)
            .map_err(|e| ResumableError::Encode(e.to_string()))?;
        Ok(buf)
    }

    /// Decode `bytes` as ciborium-encoded `T`.
    pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> ResumableResult<T> {
        ciborium::de::from_reader(bytes).map_err(|e| ResumableError::Decode(e.to_string()))
    }
}

/// Compute a content-addressable schema hash from a list of `(field_name, type_tag)`
/// pairs.
///
/// The hash is FNV-1a 64-bit over the canonical-sorted form,
/// so two struct definitions with identical fields in any source
/// order produce the same hash.
///
/// `type_tag` is a short stable string — e.g. `"u64"`, `"String"`,
/// `"Vec<u8>"`. The exact spelling is opaque to the hash function;
/// the only requirement is that semantically-equivalent shapes use
/// the same tags. The future derive macro (v0.21) will normalise
/// tags via a type-id table; for v0.20 the caller is responsible.
///
/// # Examples
///
/// ```
/// use mty_runtime::reload::resumable::compute_schema_hash;
/// let a = compute_schema_hash(&[("count", "u64"), ("name", "String")]);
/// // Order-insensitive: same fields in reversed order → same hash.
/// let b = compute_schema_hash(&[("name", "String"), ("count", "u64")]);
/// assert_eq!(a, b);
/// // Different shape → different hash.
/// let c = compute_schema_hash(&[("count", "i32"), ("name", "String")]);
/// assert_ne!(a, c);
/// ```
#[must_use]
pub fn compute_schema_hash(fields: &[(&str, &str)]) -> u64 {
    // Canonicalise: sort by field name (lexicographic). Two structs
    // with the same field set produce the same hash regardless of
    // source order.
    let mut sorted: Vec<(&str, &str)> = fields.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(b.0));

    // FNV-1a 64-bit. Cheap, has no deps, deterministic across hosts.
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut h: u64 = FNV_OFFSET;
    let mut update = |bytes: &[u8]| {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(FNV_PRIME);
        }
    };
    for (name, ty) in sorted {
        update(name.as_bytes());
        // Separator so `("ab", "c")` and `("a", "bc")` don't collide.
        update(b"\x1f");
        update(ty.as_bytes());
        update(b"\x1e");
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Counter {
        count: u64,
        label: String,
    }

    impl Resumable for Counter {
        // Schema hash is computed once at trait-impl time. In v0.21
        // a derive macro will emit this constant from the struct
        // shape automatically.
        const SCHEMA_HASH: u64 = 0xa1b2_c3d4_e5f6_0001;
    }

    #[test]
    fn schema_hash_is_order_insensitive() {
        let a = compute_schema_hash(&[("count", "u64"), ("name", "String")]);
        let b = compute_schema_hash(&[("name", "String"), ("count", "u64")]);
        assert_eq!(a, b);
    }

    #[test]
    fn schema_hash_changes_with_type() {
        let a = compute_schema_hash(&[("count", "u64")]);
        let b = compute_schema_hash(&[("count", "i64")]);
        assert_ne!(a, b);
    }

    #[test]
    fn schema_hash_changes_with_field_name() {
        let a = compute_schema_hash(&[("count", "u64")]);
        let b = compute_schema_hash(&[("counter", "u64")]);
        assert_ne!(a, b);
    }

    #[test]
    fn schema_hash_separator_prevents_collision() {
        // Without a separator FNV would treat ("ab","c") and ("a","bc")
        // as the same byte stream. Verify they differ.
        let a = compute_schema_hash(&[("ab", "c")]);
        let b = compute_schema_hash(&[("a", "bc")]);
        assert_ne!(a, b);
    }

    #[test]
    fn round_trip_through_default_codec() {
        let c = Counter {
            count: 42,
            label: "answer".into(),
        };
        let bytes = c.to_snapshot().expect("encode");
        assert!(!bytes.is_empty());
        let back: Counter = Counter::from_snapshot(&bytes).expect("decode");
        assert_eq!(c, back);
    }

    #[test]
    fn default_compatibility_is_exact_match() {
        assert!(Counter::schema_compatible_with(Counter::SCHEMA_HASH));
        assert!(!Counter::schema_compatible_with(Counter::SCHEMA_HASH ^ 1));
    }

    #[test]
    fn codec_surfaces_decode_errors() {
        // Random non-ciborium bytes should fail cleanly with Decode.
        let err = Counter::from_snapshot(&[0xFFu8; 4]).unwrap_err();
        assert!(matches!(err, ResumableError::Decode(_)));
    }
}
