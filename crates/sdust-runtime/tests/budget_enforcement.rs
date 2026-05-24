use sdust_runtime::budget::{Budget, BudgetBreach, BudgetTracker};
use std::time::Duration;

#[test]
fn cpu_budget_breach() {
    let b = Budget {
        cpu: Some(Duration::from_millis(10)),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    t.record_cpu(Duration::from_millis(5));
    assert!(t.check().is_ok());
    t.record_cpu(Duration::from_millis(8));
    assert!(matches!(t.check().unwrap_err(), BudgetBreach::Cpu(_)));
}

#[test]
fn mailbox_budget_breach() {
    let b = Budget {
        mailbox: Some(2),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    assert!(t.check_mailbox_depth(1).is_ok());
    assert!(t.check_mailbox_depth(2).is_ok());
    assert!(matches!(
        t.check_mailbox_depth(3).unwrap_err(),
        BudgetBreach::Mailbox(_)
    ));
}

#[test]
fn spawned_tasks_breach() {
    let b = Budget {
        spawned: Some(3),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    for _ in 0..3 {
        assert!(t.record_spawn().is_ok());
    }
    assert!(matches!(
        t.record_spawn().unwrap_err(),
        BudgetBreach::Spawned(_)
    ));
}

#[test]
fn host_allowlist_blocks_other_host() {
    let b = Budget {
        hosts: Some(vec!["api.example.com:443".into()]),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    assert!(t.check_host("api.example.com:443").is_ok());
    assert!(matches!(
        t.check_host("evil.example.com:443").unwrap_err(),
        BudgetBreach::Host(_)
    ));
}

#[test]
fn path_allowlist_prefix_matches() {
    let b = Budget {
        read_paths: Some(vec!["/models".into(), "/tmp/input.json".into()]),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    assert!(t.check_read_path("/models/foo").is_ok());
    assert!(t.check_read_path("/tmp/input.json").is_ok());
    assert!(matches!(
        t.check_read_path("/etc/passwd").unwrap_err(),
        BudgetBreach::Path(_)
    ));
}

#[test]
fn breach_to_runtime_error_maps_correctly() {
    use sdust_runtime::error::RuntimeError;
    let b = BudgetBreach::Cpu(Duration::from_millis(10));
    let err = b.into_runtime_error();
    assert!(matches!(err, RuntimeError::BudgetExceeded(_)));
    assert_eq!(err.diag_code(), "SD5009");

    let b = BudgetBreach::Host("evil".into());
    let err = b.into_runtime_error();
    assert_eq!(err.diag_code(), "SD5015");
}
