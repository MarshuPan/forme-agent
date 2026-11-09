//! Production release observer seam; no SCM control or installed data is used.
use std::cell::Cell;
use std::collections::VecDeque;

use super::*;

#[test]
fn release_observer_waits_for_requested_direction_after_stale_status() {
    for (action, source, pending, target) in [
        (
            p::NativeServiceAction::Start,
            ServiceState::Stopped,
            ServiceState::StartPending,
            ServiceState::Running,
        ),
        (
            p::NativeServiceAction::Stop,
            ServiceState::Running,
            ServiceState::StopPending,
            ServiceState::Stopped,
        ),
    ] {
        let mut states = VecDeque::from([source, source, pending, pending, target]);
        let rechecks = Cell::new(0);
        wait_for_change(
            action,
            Instant::now() + Duration::from_secs(2),
            || states.pop_front().ok_or_else(invalid),
            || {
                rechecks.set(rechecks.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert!(states.is_empty());
        assert_eq!(rechecks.get(), 5);
    }
}

#[test]
fn release_observer_accepts_a_verified_terminal_without_waiting() {
    for (action, target) in [
        (p::NativeServiceAction::Start, ServiceState::Running),
        (p::NativeServiceAction::Stop, ServiceState::Stopped),
    ] {
        let queries = Cell::new(0);
        let rechecks = Cell::new(0);
        wait_for_change(
            action,
            Instant::now() + Duration::from_secs(2),
            || {
                queries.set(queries.get() + 1);
                Ok(target)
            },
            || {
                rechecks.set(rechecks.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(queries.get(), 1);
        assert_eq!(rechecks.get(), 1);
    }
}

#[test]
fn release_observer_rejects_opposite_pending_paused_and_regression() {
    for (action, states) in [
        (
            p::NativeServiceAction::Start,
            vec![ServiceState::StopPending],
        ),
        (
            p::NativeServiceAction::Stop,
            vec![ServiceState::StartPending],
        ),
        (p::NativeServiceAction::Start, vec![ServiceState::Paused]),
        (
            p::NativeServiceAction::Stop,
            vec![ServiceState::PausePending],
        ),
        (
            p::NativeServiceAction::Start,
            vec![ServiceState::ContinuePending],
        ),
        (
            p::NativeServiceAction::Start,
            vec![ServiceState::StartPending, ServiceState::Stopped],
        ),
        (
            p::NativeServiceAction::Stop,
            vec![ServiceState::StopPending, ServiceState::Running],
        ),
    ] {
        let expected = states.len();
        let mut states = VecDeque::from(states);
        let queries = Cell::new(0);
        assert!(wait_for_change(
            action,
            Instant::now() + Duration::from_secs(2),
            || {
                queries.set(queries.get() + 1);
                states.pop_front().ok_or_else(invalid)
            },
            || Ok(()),
        )
        .is_err());
        assert!(states.is_empty());
        assert_eq!(
            queries.get(),
            expected,
            "unexpected extra query: {action:?}"
        );
    }
}

#[test]
fn release_observer_stops_before_query_when_registration_changes() {
    for accepted_queries in [0, 1] {
        let queries = Cell::new(0);
        let rechecks = Cell::new(0);
        let error = wait_for_change(
            p::NativeServiceAction::Stop,
            Instant::now() + Duration::from_secs(2),
            || {
                queries.set(queries.get() + 1);
                Ok(ServiceState::StopPending)
            },
            || {
                rechecks.set(rechecks.get() + 1);
                if queries.get() == accepted_queries {
                    Err(p::Error("fixture registration changed".into()))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert_eq!(error.0, "fixture registration changed");
        assert_eq!(queries.get(), accepted_queries);
        assert_eq!(rechecks.get(), accepted_queries + 1);
    }
}

#[test]
fn release_observer_does_not_retry_an_unavailable_status() {
    let queries = Cell::new(0);
    let error = wait_for_change(
        p::NativeServiceAction::Stop,
        Instant::now() + Duration::from_secs(2),
        || {
            queries.set(queries.get() + 1);
            Err(p::Error("fixture status unavailable".into()))
        },
        || Ok(()),
    )
    .unwrap_err();
    assert_eq!(error.0, "fixture status unavailable");
    assert_eq!(queries.get(), 1);
}

#[test]
fn release_observer_refuses_expired_deadlines_even_with_terminal_status() {
    for action in [p::NativeServiceAction::Start, p::NativeServiceAction::Stop] {
        assert!(wait_for_change(
            action,
            Instant::now(),
            || panic!("expired operation queried SCM"),
            || panic!("expired operation rechecked SCM"),
        )
        .is_err());
    }
}

#[test]
fn release_observer_never_renews_deadline_for_stale_or_pending_status() {
    for (action, state) in [
        (p::NativeServiceAction::Start, ServiceState::Stopped),
        (p::NativeServiceAction::Start, ServiceState::StartPending),
        (p::NativeServiceAction::Stop, ServiceState::Running),
        (p::NativeServiceAction::Stop, ServiceState::StopPending),
    ] {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(30);
        assert!(wait_for_change(action, deadline, || Ok(state), || Ok(())).is_err());
        assert!(
            Instant::now() >= deadline,
            "old status was not observed to deadline"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "observation exceeded original bound"
        );
    }
}

#[test]
fn release_observer_cannot_accept_terminal_status_after_the_original_deadline() {
    for (action, target) in [
        (p::NativeServiceAction::Start, ServiceState::Running),
        (p::NativeServiceAction::Stop, ServiceState::Stopped),
    ] {
        for slow_recheck in [false, true] {
            let queries = Cell::new(0);
            let deadline = Instant::now() + Duration::from_millis(30);
            assert!(wait_for_change(
                action,
                deadline,
                || {
                    queries.set(queries.get() + 1);
                    if !slow_recheck {
                        std::thread::sleep(Duration::from_millis(40));
                    }
                    Ok(target)
                },
                || {
                    if slow_recheck {
                        std::thread::sleep(Duration::from_millis(40));
                    }
                    Ok(())
                },
            )
            .is_err());
            assert_eq!(queries.get(), usize::from(!slow_recheck));
        }
    }
}
