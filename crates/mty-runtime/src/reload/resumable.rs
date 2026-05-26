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

use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, OnceLock};

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
    /// be decoded into `Self`. The default check is bit-equal; v0.21
    /// callers register a migration via [`SchemaRegistry::register`]
    /// to widen the accepted hash set without changing this trait
    /// impl — see [`schema_check`] for the runtime-driven check that
    /// consults the registry first and falls back to this method.
    fn schema_compatible_with(other_hash: u64) -> bool {
        Self::SCHEMA_HASH == other_hash
    }
}

// ---------------------------------------------------------------------
// v0.21 schema migrations
// ---------------------------------------------------------------------

/// Migrate an old snapshot value into the new shape.
///
/// Implementors define the per-version transition logic, e.g.
/// "V2 = V1 with `created_at` defaulted to `epoch`". The runtime
/// composes these into chains via [`SchemaRegistry`] so a V1
/// snapshot can be lifted through V2 into V3 without the user
/// writing every pairwise hop.
///
/// `Old` and `Self` (the `New` type) must both implement [`Resumable`]
/// so the registry can chain migrations using their `SCHEMA_HASH`
/// constants.
pub trait MigrateFrom<Old: Resumable>: Resumable {
    /// Translate an `Old` value into `Self`. Errors propagate up
    /// through the swap pipeline as [`ResumableError::Decode`] — the
    /// caller sees a `MT5063` with the migration's explanation.
    fn migrate_from(old: Old) -> ResumableResult<Self>;
}

/// Re-encode a snapshot from `Old` shape into `New` shape via
/// [`MigrateFrom`]. The runtime invokes this exactly once per
/// registered migration hop; chained migrations apply the function
/// sequentially.
pub fn try_migrate<Old, New>(old_bytes: &[u8], old_hash: u64) -> ResumableResult<Vec<u8>>
where
    Old: Resumable,
    New: Resumable + MigrateFrom<Old>,
{
    if old_hash != Old::SCHEMA_HASH {
        return Err(ResumableError::Decode(format!(
            "migrate: source snapshot hash {old_hash:#018x} doesn't match Old::SCHEMA_HASH {:#018x}",
            Old::SCHEMA_HASH
        )));
    }
    let old: Old = SnapshotCodec::decode(old_bytes)?;
    let new = New::migrate_from(old)?;
    SnapshotCodec::encode(&new)
}

/// A registered migration step: re-encode bytes from `old_hash` shape
/// into `new_hash` shape.
pub type MigrationFn = Arc<dyn Fn(&[u8]) -> ResumableResult<Vec<u8>> + Send + Sync + 'static>;

/// Process-global registry of migration hops. Keyed by `(old, new)`
/// hash pair; the runtime composes them at lookup time into a chain
/// that lifts a snapshot from any registered source hash to a target
/// hash.
pub struct SchemaRegistry {
    edges: Mutex<HashMap<(u64, u64), MigrationFn>>,
}

impl std::fmt::Debug for SchemaRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let g = self.edges.lock();
        f.debug_struct("SchemaRegistry")
            .field("edges", &g.len())
            .finish()
    }
}

impl SchemaRegistry {
    /// Empty registry. Used by tests that need an isolated instance.
    pub fn new() -> Self {
        SchemaRegistry {
            edges: Mutex::new(HashMap::new()),
        }
    }

    /// Process-global instance. The swap pipeline consults this when
    /// the snapshot's source hash doesn't match the new module's
    /// hash.
    pub fn global() -> &'static SchemaRegistry {
        static INSTANCE: OnceLock<SchemaRegistry> = OnceLock::new();
        INSTANCE.get_or_init(SchemaRegistry::new)
    }

    /// Register a migration from `Old` (`Old::SCHEMA_HASH`) to `New`
    /// (`New::SCHEMA_HASH`). Subsequent reloads that present an
    /// `Old`-shape snapshot to a `New`-shape module succeed by
    /// transparently re-encoding the bytes through `New::migrate_from`.
    pub fn register<Old, New>(&self)
    where
        Old: Resumable + 'static,
        New: Resumable + MigrateFrom<Old> + 'static,
    {
        let key = (Old::SCHEMA_HASH, New::SCHEMA_HASH);
        let f: MigrationFn =
            Arc::new(|bytes: &[u8]| try_migrate::<Old, New>(bytes, Old::SCHEMA_HASH));
        self.edges.lock().insert(key, f);
    }

    /// Register a raw migration function — used by integration tests
    /// that need a synthetic edge without owning both types statically.
    pub fn register_raw(
        &self,
        old_hash: u64,
        new_hash: u64,
        f: impl Fn(&[u8]) -> ResumableResult<Vec<u8>> + Send + Sync + 'static,
    ) {
        let f: MigrationFn = Arc::new(f);
        self.edges.lock().insert((old_hash, new_hash), f);
    }

    /// Compute a migration chain `old_hash → … → new_hash`. BFS so
    /// the shortest registered path wins.
    pub fn chain(&self, old_hash: u64, new_hash: u64) -> Option<Vec<MigrationFn>> {
        if old_hash == new_hash {
            return Some(Vec::new());
        }
        let edges = self.edges.lock();
        let mut frontier: VecDeque<(u64, Vec<MigrationFn>)> = VecDeque::new();
        let mut seen: HashSet<u64> = HashSet::new();
        frontier.push_back((old_hash, Vec::new()));
        seen.insert(old_hash);
        while let Some((cur, path)) = frontier.pop_front() {
            for (&(from, to), f) in edges.iter() {
                if from != cur || seen.contains(&to) {
                    continue;
                }
                let mut next_path = path.clone();
                next_path.push(f.clone());
                if to == new_hash {
                    return Some(next_path);
                }
                seen.insert(to);
                frontier.push_back((to, next_path));
            }
        }
        None
    }

    /// Apply a chain of migrations to `bytes` in order.
    pub fn apply_chain(chain: &[MigrationFn], bytes: &[u8]) -> ResumableResult<Vec<u8>> {
        let mut cur = bytes.to_vec();
        for f in chain {
            cur = f(&cur)?;
        }
        Ok(cur)
    }

    /// Number of registered edges. Used by tests.
    pub fn edge_count(&self) -> usize {
        self.edges.lock().len()
    }

    /// Drop every registered migration.
    pub fn clear(&self) {
        self.edges.lock().clear();
    }
}

impl Default for SchemaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Outcome of a schema-compatibility check, used by the swap pipeline
/// to decide whether to migrate, accept, or reject the snapshot.
pub enum SchemaCheck {
    /// Hashes match — pass the snapshot bytes through unchanged.
    Direct,
    /// Migration registered — apply the chain before deserialising.
    Migrate(Vec<MigrationFn>),
    /// No matching path — reload should fail with `MT5060`.
    Incompatible,
}

impl std::fmt::Debug for SchemaCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaCheck::Direct => write!(f, "SchemaCheck::Direct"),
            SchemaCheck::Migrate(chain) => write!(f, "SchemaCheck::Migrate(len={})", chain.len()),
            SchemaCheck::Incompatible => write!(f, "SchemaCheck::Incompatible"),
        }
    }
}

/// Decide how the swap pipeline should handle the given (old, new)
/// hash pair.
pub fn schema_check(registry: &SchemaRegistry, old_hash: u64, new_hash: u64) -> SchemaCheck {
    if old_hash == new_hash {
        return SchemaCheck::Direct;
    }
    match registry.chain(old_hash, new_hash) {
        Some(chain) if !chain.is_empty() => SchemaCheck::Migrate(chain),
        _ => SchemaCheck::Incompatible,
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

    // -----------------------------------------------------------------
    // v0.21 schema migration tests
    // -----------------------------------------------------------------

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct StateV1 {
        count: u64,
    }
    impl Resumable for StateV1 {
        const SCHEMA_HASH: u64 = 0x0001_0000_0000_0001;
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct StateV2 {
        count: u64,
        label: String,
    }
    impl Resumable for StateV2 {
        const SCHEMA_HASH: u64 = 0x0002_0000_0000_0002;
    }

    impl MigrateFrom<StateV1> for StateV2 {
        fn migrate_from(old: StateV1) -> ResumableResult<Self> {
            Ok(StateV2 {
                count: old.count,
                label: String::new(),
            })
        }
    }

    #[test]
    fn try_migrate_v1_to_v2() {
        let v1 = StateV1 { count: 42 };
        let bytes = SnapshotCodec::encode(&v1).unwrap();
        let v2_bytes = try_migrate::<StateV1, StateV2>(&bytes, StateV1::SCHEMA_HASH).unwrap();
        let v2: StateV2 = SnapshotCodec::decode(&v2_bytes).unwrap();
        assert_eq!(v2.count, 42);
        assert_eq!(v2.label, "");
    }

    #[test]
    fn try_migrate_rejects_wrong_hash() {
        let v1 = StateV1 { count: 0 };
        let bytes = SnapshotCodec::encode(&v1).unwrap();
        let err = try_migrate::<StateV1, StateV2>(&bytes, StateV1::SCHEMA_HASH ^ 1).unwrap_err();
        match err {
            ResumableError::Decode(s) => assert!(s.contains("doesn't match")),
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    #[test]
    fn registry_register_and_chain_direct() {
        let r = SchemaRegistry::new();
        r.register::<StateV1, StateV2>();
        assert_eq!(r.edge_count(), 1);
        let chain = r
            .chain(StateV1::SCHEMA_HASH, StateV2::SCHEMA_HASH)
            .expect("chain");
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn registry_no_chain_for_unrelated_hashes() {
        let r = SchemaRegistry::new();
        r.register::<StateV1, StateV2>();
        assert!(r.chain(0xDEAD_BEEF, 0xFEED_BEEF).is_none());
    }

    #[test]
    fn schema_check_direct_when_hashes_equal() {
        let r = SchemaRegistry::new();
        match schema_check(&r, 42, 42) {
            SchemaCheck::Direct => {}
            other => panic!("expected Direct, got {other:?}"),
        }
    }

    #[test]
    fn schema_check_migrate_when_edge_present() {
        let r = SchemaRegistry::new();
        r.register::<StateV1, StateV2>();
        match schema_check(&r, StateV1::SCHEMA_HASH, StateV2::SCHEMA_HASH) {
            SchemaCheck::Migrate(chain) => assert_eq!(chain.len(), 1),
            other => panic!("expected Migrate, got {other:?}"),
        }
    }

    #[test]
    fn schema_check_incompatible_when_no_edge() {
        let r = SchemaRegistry::new();
        match schema_check(&r, 1, 2) {
            SchemaCheck::Incompatible => {}
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn apply_chain_round_trip() {
        let r = SchemaRegistry::new();
        r.register::<StateV1, StateV2>();
        let chain = r
            .chain(StateV1::SCHEMA_HASH, StateV2::SCHEMA_HASH)
            .expect("chain");
        let v1_bytes = SnapshotCodec::encode(&StateV1 { count: 7 }).unwrap();
        let v2_bytes = SchemaRegistry::apply_chain(&chain, &v1_bytes).unwrap();
        let v2: StateV2 = SnapshotCodec::decode(&v2_bytes).unwrap();
        assert_eq!(v2.count, 7);
    }

    #[test]
    fn registry_clear_removes_edges() {
        let r = SchemaRegistry::new();
        r.register::<StateV1, StateV2>();
        assert_eq!(r.edge_count(), 1);
        r.clear();
        assert_eq!(r.edge_count(), 0);
    }
}
