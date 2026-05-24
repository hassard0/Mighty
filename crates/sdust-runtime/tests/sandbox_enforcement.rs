use sdust_runtime::budget::{Budget, BudgetTracker};

#[test]
fn host_allowlist_blocks_external() {
    let b = Budget {
        hosts: Some(vec!["api.example.com:443".into()]),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    assert!(t.check_host("api.example.com:443").is_ok());
    assert!(t.check_host("evil.example.com:443").is_err());
}

#[test]
fn read_path_allowlist_admits_prefix_dirs() {
    let b = Budget {
        read_paths: Some(vec!["/models".into(), "/tmp/input.json".into()]),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    assert!(t.check_read_path("/models/foo").is_ok());
    assert!(t.check_read_path("/models").is_ok());
    assert!(t.check_read_path("/tmp/input.json").is_ok());
    assert!(t.check_read_path("/etc/passwd").is_err());
}

#[test]
fn write_path_allowlist_independent_of_read() {
    let b = Budget {
        read_paths: Some(vec!["/models".into()]),
        write_paths: Some(vec!["/tmp/out".into()]),
        ..Default::default()
    };
    let t = BudgetTracker::new(b);
    assert!(t.check_read_path("/models/x").is_ok());
    assert!(t.check_write_path("/models/x").is_err());
    assert!(t.check_write_path("/tmp/out/y").is_ok());
}

#[test]
fn no_allowlist_means_permissive() {
    let t = BudgetTracker::new(Budget::default());
    assert!(t.check_host("anything").is_ok());
    assert!(t.check_read_path("/anywhere").is_ok());
}
