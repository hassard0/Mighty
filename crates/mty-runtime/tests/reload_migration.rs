//! Hot-reload v0.21 — schema-migration tests.
//!
//! The v0.20 baseline rejected any mismatched `SCHEMA_HASH` outright
//! (`MT5060`). v0.21 widens compatibility through the
//! [`mty_runtime::reload::SchemaRegistry`] — implementors register a
//! `MigrateFrom<Old> for New` migration and the runtime
//! transparently re-encodes the snapshot through the chain before
//! deserialising into the new shape.

use mty_runtime::reload::{
    schema_check, try_migrate, MigrateFrom, ModuleSource, ReloadError, ReloadGate, ReloadOptions,
    ReloadRunner, Resumable, ResumableError, ResumableResult, SchemaCheck, SchemaRegistry,
    SnapshotCodec, SwapPlan,
};
use mty_runtime::AgentId;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

fn fixture_descriptor(name: &str, id: u64) -> Arc<mty_runtime::AgentDescriptor> {
    use mty_ir::interp::value::Value;
    use mty_ir::ir::AgentIrId;
    use mty_runtime::budget::{Budget, BudgetTracker};
    use mty_runtime::mailbox::{Mailbox, SendPolicy};

    Arc::new(mty_runtime::AgentDescriptor {
        id: AgentId(id),
        name: name.into(),
        sir_id: AgentIrId(0),
        state: Mutex::new(Value::Unit),
        mailbox: Arc::new(Mailbox::new(8, SendPolicy::Block)),
        budget: Arc::new(BudgetTracker::new(Budget::default())),
        supervisor: None,
        mailbox_depth: AtomicU64::new(0),
    })
}

// ---------------------------------------------------------------------
// Three schema versions, two migrations.
// ---------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct V1 {
    count: u64,
}
impl Resumable for V1 {
    const SCHEMA_HASH: u64 = 0xA000_0000_0000_0001;
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct V2 {
    count: u64,
    label: String,
}
impl Resumable for V2 {
    const SCHEMA_HASH: u64 = 0xA000_0000_0000_0002;
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct V3 {
    count: u64,
    label: String,
    created_at: u64,
}
impl Resumable for V3 {
    const SCHEMA_HASH: u64 = 0xA000_0000_0000_0003;
}

impl MigrateFrom<V1> for V2 {
    fn migrate_from(old: V1) -> ResumableResult<Self> {
        Ok(V2 {
            count: old.count,
            label: String::from("(migrated-from-v1)"),
        })
    }
}

impl MigrateFrom<V2> for V3 {
    fn migrate_from(old: V2) -> ResumableResult<Self> {
        Ok(V3 {
            count: old.count,
            label: old.label,
            // Default added field.
            created_at: 0,
        })
    }
}

// ---------------------------------------------------------------------
// 1. V1 → V2 migration translates state cleanly
// ---------------------------------------------------------------------

#[test]
fn migrate_v1_to_v2_state() {
    let desc = fixture_descriptor("Connection", 200);
    // Agent's state cell is *typed as V2* — the migration
    // converts the on-disk V1 snapshot into V2 before decoding.
    let state = Arc::new(Mutex::new(V2 {
        count: 0,
        label: "placeholder".into(),
    }));
    // Pre-encode a V1 snapshot to simulate the "on-disk" state that
    // the running agent will be migrated from. In a real reload the
    // snapshot is taken inside the pipeline; here we shape the test
    // by seeding the typed cell with a V2 wrapping the V1 fields.
    // The dry-run codec round-trip below verifies the migration
    // works end-to-end.

    // Build a migration registry and register V1→V2.
    let registry = Arc::new(SchemaRegistry::new());
    registry.register::<V1, V2>();

    // Directly exercise the chain: encode a V1 value, migrate, then
    // decode as V2.
    let v1 = V1 { count: 7 };
    let v1_bytes = SnapshotCodec::encode(&v1).unwrap();
    match schema_check(&registry, V1::SCHEMA_HASH, V2::SCHEMA_HASH) {
        SchemaCheck::Migrate(chain) => {
            let v2_bytes = SchemaRegistry::apply_chain(&chain, &v1_bytes).unwrap();
            let v2: V2 = SnapshotCodec::decode(&v2_bytes).unwrap();
            assert_eq!(v2.count, 7);
            assert!(v2.label.contains("migrated-from-v1"));
        }
        other => panic!("expected Migrate, got {other:?}"),
    }

    // The runner-driven path: stash a V2-shaped state, run a swap
    // with V1's hash as the "old" so the migration kicks in.
    *state.lock() = V2 {
        count: 0,
        label: "before".into(),
    };
    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: V1::SCHEMA_HASH,
        new_schema_hash: V2::SCHEMA_HASH,
        module: ModuleSource::SameProgram,
        options: ReloadOptions::default(),
    };
    let gate = Arc::new(ReloadGate::new());
    // For this test the snapshot the runner emits will be a V2
    // (matching the typed cell), so the migration is best exercised
    // via the chain test above. We still run the pipeline to confirm
    // a registry-driven non-Direct schema check completes without
    // erroring.
    let runner = ReloadRunner {
        plan,
        desc,
        state: state.clone(),
        gate,
        drain_signal: None,
        schema_registry: Some(registry.clone()),
        program: None,
    };
    runner.run().expect("runner with migration registry runs");
}

// ---------------------------------------------------------------------
// 2. V2 migration adds a defaulted field
// ---------------------------------------------------------------------

#[test]
fn migrate_with_extra_field_defaults() {
    let v2 = V2 {
        count: 1,
        label: "x".into(),
    };
    let v2_bytes = SnapshotCodec::encode(&v2).unwrap();
    let v3_bytes = try_migrate::<V2, V3>(&v2_bytes, V2::SCHEMA_HASH).unwrap();
    let v3: V3 = SnapshotCodec::decode(&v3_bytes).unwrap();
    assert_eq!(v3.count, 1);
    assert_eq!(v3.label, "x");
    assert_eq!(v3.created_at, 0);
}

// ---------------------------------------------------------------------
// 3. V1 → V2 → V3 chain composes via the registry BFS
// ---------------------------------------------------------------------

#[test]
fn migrate_chain_v1_to_v3() {
    let registry = SchemaRegistry::new();
    registry.register::<V1, V2>();
    registry.register::<V2, V3>();
    let chain = registry
        .chain(V1::SCHEMA_HASH, V3::SCHEMA_HASH)
        .expect("chain present");
    assert_eq!(chain.len(), 2);

    let v1 = V1 { count: 99 };
    let v1_bytes = SnapshotCodec::encode(&v1).unwrap();
    let v3_bytes = SchemaRegistry::apply_chain(&chain, &v1_bytes).unwrap();
    let v3: V3 = SnapshotCodec::decode(&v3_bytes).unwrap();
    assert_eq!(v3.count, 99);
    assert!(v3.label.contains("migrated-from-v1"));
    assert_eq!(v3.created_at, 0);
}

// ---------------------------------------------------------------------
// 4. A migration function that fails returns a clean diagnostic
// ---------------------------------------------------------------------

#[test]
fn migrate_fail_returns_clean_error() {
    let registry = SchemaRegistry::new();
    registry.register_raw(0x1111, 0x2222, |_bytes: &[u8]| {
        Err(ResumableError::Decode(
            "synthetic migration failure for test".into(),
        ))
    });

    let chain = registry.chain(0x1111, 0x2222).expect("chain");
    let bogus = vec![0u8; 8];
    let err = SchemaRegistry::apply_chain(&chain, &bogus).unwrap_err();
    match err {
        ResumableError::Decode(s) => assert!(s.contains("synthetic migration failure")),
        other => panic!("expected Decode, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// 5. Incompatible hashes with no migration → MT5060
// ---------------------------------------------------------------------

#[test]
fn migrate_no_chain_yields_incompatible_schema() {
    let registry = Arc::new(SchemaRegistry::new());
    // No edge registered.
    let desc = fixture_descriptor("Connection", 201);
    let state = Arc::new(Mutex::new(V2 {
        count: 0,
        label: "x".into(),
    }));
    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: V1::SCHEMA_HASH,
        new_schema_hash: V2::SCHEMA_HASH,
        module: ModuleSource::SameProgram,
        options: ReloadOptions::default(),
    };
    let gate = Arc::new(ReloadGate::new());
    let runner = ReloadRunner {
        plan,
        desc,
        state,
        gate,
        drain_signal: None,
        schema_registry: Some(registry),
        program: None,
    };
    let err = runner.run().unwrap_err();
    match &err {
        ReloadError::IncompatibleSchema { old, new } => {
            assert_eq!(*old, V1::SCHEMA_HASH);
            assert_eq!(*new, V2::SCHEMA_HASH);
        }
        other => panic!("expected IncompatibleSchema, got {other:?}"),
    }
    assert_eq!(err.diag_code(), "MT5060");
}

// ---------------------------------------------------------------------
// 6. Direct (identity) chain when hashes match — no migration applied
// ---------------------------------------------------------------------

#[test]
fn migrate_direct_path_when_hashes_match() {
    let registry = SchemaRegistry::new();
    registry.register::<V1, V2>();
    match schema_check(&registry, V2::SCHEMA_HASH, V2::SCHEMA_HASH) {
        SchemaCheck::Direct => {}
        other => panic!("expected Direct, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// 7. Multiple registrations don't shadow each other — V1→V2 and
//    V2→V3 coexist; V1→V3 also finds the chain.
// ---------------------------------------------------------------------

#[test]
fn migrate_registry_holds_multiple_edges() {
    let registry = SchemaRegistry::new();
    registry.register::<V1, V2>();
    registry.register::<V2, V3>();
    assert_eq!(registry.edge_count(), 2);
    assert!(registry.chain(V1::SCHEMA_HASH, V2::SCHEMA_HASH).is_some());
    assert!(registry.chain(V2::SCHEMA_HASH, V3::SCHEMA_HASH).is_some());
    assert!(registry.chain(V1::SCHEMA_HASH, V3::SCHEMA_HASH).is_some());
    // Reverse direction not registered.
    assert!(registry.chain(V3::SCHEMA_HASH, V1::SCHEMA_HASH).is_none());
}

// ---------------------------------------------------------------------
// 8. Migration through the swap pipeline with a chain registered
// ---------------------------------------------------------------------

#[test]
fn migrate_pipeline_applies_chain() {
    // Two synthetic hashes to keep this test independent of the V1
    // shape. The agent's typed state is V2; the snapshot we want to
    // restore is shaped like V2 already (since we don't actually
    // change the type), but we drive the pipeline as if it crossed
    // a migration edge by registering an identity chain.
    let h_old: u64 = 0xBEEF_0001;
    let h_new: u64 = 0xBEEF_0002;
    let registry = Arc::new(SchemaRegistry::new());
    registry.register_raw(h_old, h_new, |bytes: &[u8]| {
        // Identity migration — the V2 snapshot bytes are valid V2
        // already, so we just round-trip them.
        Ok(bytes.to_vec())
    });

    let desc = fixture_descriptor("Connection", 202);
    let state = Arc::new(Mutex::new(V2 {
        count: 42,
        label: "alpha".into(),
    }));
    let gate = Arc::new(ReloadGate::new());

    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: h_old,
        new_schema_hash: h_new,
        module: ModuleSource::SameProgram,
        options: ReloadOptions::default(),
    };
    let runner = ReloadRunner {
        plan,
        desc,
        state: state.clone(),
        gate,
        drain_signal: None,
        schema_registry: Some(registry),
        program: None,
    };
    let report = runner.run().expect("migration runner");
    assert_eq!(report.old_schema_hash, h_old);
    assert_eq!(report.new_schema_hash, h_new);
    // State preserved.
    let s = state.lock();
    assert_eq!(s.count, 42);
    assert_eq!(s.label, "alpha");
}
