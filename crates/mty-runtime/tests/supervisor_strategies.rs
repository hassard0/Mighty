use mty_runtime::supervisor::{RestartPolicy, RestartTracker, Strategy};
use std::time::Duration;

#[test]
fn one_for_one_is_default_first() {
    assert_eq!(Strategy::OneForOne as i32, 0);
    assert_eq!(Strategy::OneForAll as i32, 1);
    assert_eq!(Strategy::RestForOne as i32, 2);
    assert_eq!(Strategy::Escalate as i32, 3);
}

#[test]
fn restart_tracker_allows_under_limit() {
    let mut t = RestartTracker::new(RestartPolicy {
        max_attempts: 3,
        window: Duration::from_secs(30),
        backoff_min: Duration::from_millis(0),
        backoff_max: Duration::from_millis(0),
    });
    assert!(t.may_restart().is_some());
    assert!(t.may_restart().is_some());
    assert!(t.may_restart().is_some());
    assert!(t.may_restart().is_none());
}

#[test]
fn backoff_within_range() {
    let mut t = RestartTracker::new(RestartPolicy {
        max_attempts: 10,
        window: Duration::from_secs(30),
        backoff_min: Duration::from_millis(10),
        backoff_max: Duration::from_millis(20),
    });
    for _ in 0..5 {
        let d = t.may_restart().unwrap();
        assert!(
            d >= Duration::from_millis(10) && d < Duration::from_millis(20),
            "backoff {:?} out of [10ms, 20ms)",
            d
        );
    }
}

#[test]
fn child_failure_from_runtime_error() {
    use mty_runtime::error::RuntimeError;
    use mty_runtime::supervisor::ChildFailure;

    let f: ChildFailure = RuntimeError::BudgetExceeded("cpu".into()).into();
    assert!(matches!(f, ChildFailure::Budget(_)));

    let f: ChildFailure = RuntimeError::DeadlineExceeded(Duration::from_millis(1)).into();
    assert!(matches!(f, ChildFailure::Deadline));

    let f: ChildFailure = RuntimeError::AgentPanic { msg: "boom".into() }.into();
    assert!(matches!(f, ChildFailure::Panic(_)));
}
