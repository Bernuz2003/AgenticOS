use super::*;

#[test]
fn register_sets_defaults() {
    let mut sched = ProcessScheduler::new();
    sched.register(1, WorkloadClass::Fast, ProcessPriority::Normal);

    assert_eq!(sched.priority(1), ProcessPriority::Normal);
    let q = sched.quota(1).expect("quota exists");
    assert_eq!(q.max_tokens, 512);
    assert_eq!(q.max_syscalls, 2);
    assert_eq!(sched.tokens_generated(1), 0);
}

#[test]
fn unregister_cleans_up() {
    let mut sched = ProcessScheduler::new();
    sched.register(1, WorkloadClass::General, ProcessPriority::High);
    sched.unregister(1);
    assert!(sched.quota(1).is_none());
    assert!(sched.snapshot(1).is_none());
}

#[test]
fn record_token_enforces_quota() {
    let mut sched = ProcessScheduler::new();
    sched.register(1, WorkloadClass::Fast, ProcessPriority::Normal);
    // Fast workload max_tokens = 512
    for _ in 0..511 {
        assert!(!sched.record_token(1));
    }
    // 512th token -> quota exceeded
    assert!(sched.record_token(1));
}

#[test]
fn record_syscall_enforces_quota() {
    let mut sched = ProcessScheduler::new();
    sched.register(1, WorkloadClass::Fast, ProcessPriority::Normal);
    // Fast workload max_syscalls = 2
    assert!(!sched.record_syscall(1));
    assert!(sched.record_syscall(1));
}

#[test]
fn scheduling_order_respects_priority() {
    let mut sched = ProcessScheduler::new();
    sched.register(1, WorkloadClass::General, ProcessPriority::Low);
    sched.register(2, WorkloadClass::General, ProcessPriority::Critical);
    sched.register(3, WorkloadClass::General, ProcessPriority::Normal);
    sched.register(4, WorkloadClass::General, ProcessPriority::High);

    let order = sched.scheduling_order(&[1, 2, 3, 4]);
    assert_eq!(order, vec![2, 4, 3, 1]);
}

#[test]
fn scheduling_order_tie_breaks_by_pid() {
    let mut sched = ProcessScheduler::new();
    sched.register(5, WorkloadClass::General, ProcessPriority::Normal);
    sched.register(3, WorkloadClass::General, ProcessPriority::Normal);
    sched.register(7, WorkloadClass::General, ProcessPriority::Normal);

    let order = sched.scheduling_order(&[5, 3, 7]);
    assert_eq!(order, vec![3, 5, 7]);
}

#[test]
fn set_priority_changes_order() {
    let mut sched = ProcessScheduler::new();
    sched.register(1, WorkloadClass::General, ProcessPriority::Normal);
    sched.register(2, WorkloadClass::General, ProcessPriority::Normal);

    assert!(sched.set_priority(2, ProcessPriority::High));
    let order = sched.scheduling_order(&[1, 2]);
    assert_eq!(order, vec![2, 1]);
}

#[test]
fn set_quota_override() {
    let mut sched = ProcessScheduler::new();
    sched.register(1, WorkloadClass::Fast, ProcessPriority::Normal);

    let custom = ProcessQuota {
        max_tokens: 100,
        max_syscalls: 3,
    };
    assert!(sched.set_quota(1, custom));

    // max_syscalls = 3: first two calls ok, third exceeds
    assert!(!sched.record_syscall(1));
    assert!(!sched.record_syscall(1));
    assert!(sched.record_syscall(1));
}

#[test]
fn workload_defaults_vary() {
    let fast = ProcessQuota::defaults_for(WorkloadClass::Fast);
    let reasoning = ProcessQuota::defaults_for(WorkloadClass::Reasoning);
    assert!(reasoning.max_tokens > fast.max_tokens);
}

#[test]
fn priority_from_str_loose() {
    assert_eq!(
        ProcessPriority::from_str_loose("HIGH"),
        Some(ProcessPriority::High)
    );
    assert_eq!(
        ProcessPriority::from_str_loose("low"),
        Some(ProcessPriority::Low)
    );
    assert_eq!(ProcessPriority::from_str_loose("unknown"), None);
}

#[test]
fn snapshot_returns_correct_data() {
    let mut sched = ProcessScheduler::new();
    sched.register(42, WorkloadClass::Code, ProcessPriority::High);
    sched.record_token(42);
    sched.record_token(42);
    sched.record_syscall(42);

    let snap = sched.snapshot(42).expect("snapshot should exist");
    assert_eq!(snap.tokens_generated, 2);
    assert_eq!(snap.syscalls_used, 1);
    assert_eq!(snap.priority, ProcessPriority::High);
    assert!(matches!(snap.workload, WorkloadClass::Code));
}

#[test]
fn summary_counts_priorities() {
    let mut sched = ProcessScheduler::new();
    sched.register(1, WorkloadClass::General, ProcessPriority::Normal);
    sched.register(2, WorkloadClass::General, ProcessPriority::High);
    sched.register(3, WorkloadClass::General, ProcessPriority::High);

    let s = sched.summary();
    assert!(s.contains("scheduler_tracked=3"));
    assert!(s.contains("priority_high=2"));
    assert!(s.contains("priority_normal=1"));
}

#[test]
fn registered_pids_returns_sorted() {
    let mut sched = ProcessScheduler::new();
    sched.register(5, WorkloadClass::General, ProcessPriority::Normal);
    sched.register(1, WorkloadClass::Code, ProcessPriority::High);
    sched.register(3, WorkloadClass::Fast, ProcessPriority::Low);

    let pids = sched.registered_pids();
    assert_eq!(pids, vec![1, 3, 5]);

    sched.unregister(3);
    let pids = sched.registered_pids();
    assert_eq!(pids, vec![1, 5]);
}
