//! v0.25 Track C: agent fields declared with fixed-size array types
//! (`[T; N]`) must typecheck end-to-end. v0.24 Track E's Notetris demo
//! needed `board: [U32; 200]` as an agent field — typeck was capturing
//! the element type but dropping the length on the HIR side, so the
//! resolved `TyData::Array { len: None }` was a slice rather than a
//! fixed array. This test pins that the length round-trips.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn errors(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "agent_field_array_typeck.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_lower_err {
        diags.extend(check_package(&pkg));
    }
    diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| format!("{}: {}", d.code.as_str(), d.primary.message))
        .collect()
}

#[test]
fn agent_array_field_typechecks() {
    let src = "
        protocol Tick { Pulse() -> I32 }
        agent Notetris: Tick {
          board: [U32; 200]
          score: U32 = 0
          on Pulse() -> score
        }
    ";
    let errs = errors(src);
    assert!(
        errs.is_empty(),
        "expected no typeck errors for agent array field, got {:?}",
        errs
    );
}

#[test]
fn agent_array_field_read_indexed() {
    let src = "
        protocol Tick { Pulse() -> U32 }
        agent Grid: Tick {
          cells: [U32; 16]
          on Pulse() -> cells[0]
        }
    ";
    let errs = errors(src);
    assert!(
        errs.is_empty(),
        "expected no typeck errors for indexed read of agent array, got {:?}",
        errs
    );
}

#[test]
fn agent_array_field_write_indexed() {
    let src = "
        protocol KB { KeyDown(k: I32) -> I32 }
        agent Game: KB {
          board: [I32; 16]
          on KeyDown(k) -> {
            board[0] = k;
            k
          }
        }
    ";
    let errs = errors(src);
    assert!(
        errs.is_empty(),
        "expected no typeck errors for indexed write of agent array, got {:?}",
        errs
    );
}

#[test]
fn agent_field_array_length_preserved_through_hir() {
    // Spot-check that the HIR lowerer captures `len = Some(_)` for an
    // agent's fixed-array field. Without this, the resolver would see
    // `TyData::Array { len: None }` (a slice) and storage-layout
    // computation downstream would fail.
    let parsed = parse_source(
        "agent X { board: [U32; 200] }".into(),
        "len_preserved.mty".into(),
    );
    let (pkg, _diags) = lower(&parsed);
    // Find the one agent and inspect its single state field.
    let agent_id = pkg
        .top_level
        .iter()
        .find_map(|iid| match &pkg.items[*iid] {
            mty_hir::Item::Agent(aid) => Some(*aid),
            _ => None,
        })
        .expect("agent decl lowered");
    let agent = &pkg.agents[agent_id];
    assert_eq!(agent.state.len(), 1);
    let field_ty = agent.state[0].ty.expect("field has a type");
    match &pkg.types[field_ty] {
        mty_hir::HirType::Array { len, .. } => {
            assert!(
                len.is_some(),
                "expected the fixed-size length expression to be lowered, got None"
            );
        }
        other => panic!("expected HirType::Array, got {:?}", other),
    }
}
