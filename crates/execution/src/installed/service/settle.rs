//! Observe one accepted OS change; never resubmit it while status catches up.
#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use forme_protocol as p;

pub(super) fn wait(
    action: p::NativeServiceAction,
    deadline: Instant,
    mut inspect: impl FnMut() -> p::Result<p::NativeServiceState>,
    mut pause: impl FnMut(Duration),
) -> p::Result<p::NativeServiceState> {
    use p::NativeServiceState as State;
    let (source, pending, target) = match action {
        p::NativeServiceAction::Start => (State::Stopped, State::Starting, State::Running),
        p::NativeServiceAction::Stop => (State::Running, State::Stopping, State::Stopped),
    };
    let mut transition_seen = false;
    loop {
        let state = inspect()?;
        if state == target {
            return Ok(state);
        }
        // ControlService/StartService accepting a request does not mean its
        // asynchronous handler has published the pending state yet. The old
        // state may persist, but must not reappear after a pending observation.
        if state == pending {
            transition_seen = true;
        } else if state != source || transition_seen {
            return Err(unsettled());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(unsettled());
        }
        pause(remaining.min(Duration::from_millis(50)));
    }
}

fn unsettled() -> p::Error {
    p::Error("native service did not settle; probe the original operation before any retry".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_service_changes_wait_through_stale_status_without_resubmission() {
        use p::NativeServiceState as State;
        for (action, source, pending, target) in [
            (
                p::NativeServiceAction::Start,
                State::Stopped,
                State::Starting,
                State::Running,
            ),
            (
                p::NativeServiceAction::Stop,
                State::Running,
                State::Stopping,
                State::Stopped,
            ),
        ] {
            let mut states = [source, source, pending, pending, target].into_iter();
            let mut pauses = 0;
            let observed = wait(
                action,
                Instant::now() + Duration::from_secs(1),
                || Ok(states.next().expect("unexpected extra query")),
                |_| pauses += 1,
            )
            .unwrap();
            assert_eq!(observed, target);
            assert_eq!(pauses, 4);
            assert!(states.next().is_none());
        }
    }

    #[test]
    fn service_observation_rejects_regression_opposite_transition_and_unknown() {
        use p::NativeServiceState as State;
        for (action, sequence) in [
            (
                p::NativeServiceAction::Stop,
                vec![State::Stopping, State::Running],
            ),
            (
                p::NativeServiceAction::Start,
                vec![State::Starting, State::Stopped],
            ),
            (p::NativeServiceAction::Stop, vec![State::Starting]),
            (p::NativeServiceAction::Start, vec![State::Stopping]),
            (p::NativeServiceAction::Stop, vec![State::Unknown]),
            (p::NativeServiceAction::Start, vec![State::NotInstalled]),
            (p::NativeServiceAction::Start, vec![State::Failed]),
        ] {
            let mut states = sequence.into_iter();
            assert!(wait(
                action,
                Instant::now() + Duration::from_secs(1),
                || Ok(states.next().expect("unexpected extra query")),
                |_| {},
            )
            .is_err());
            assert!(states.next().is_none());
        }
    }

    #[test]
    fn stale_service_status_expires_without_sleeping_or_fabricating_success() {
        use p::NativeServiceState as State;
        for (action, state) in [
            (p::NativeServiceAction::Stop, State::Running),
            (p::NativeServiceAction::Stop, State::Stopping),
            (p::NativeServiceAction::Start, State::Stopped),
            (p::NativeServiceAction::Start, State::Starting),
        ] {
            assert!(wait(
                action,
                Instant::now(),
                || Ok(state),
                |_| panic!("expired wait slept")
            )
            .is_err());
        }
        let error = wait(
            p::NativeServiceAction::Stop,
            Instant::now() + Duration::from_secs(1),
            || Err(p::Error("status unavailable".into())),
            |_| panic!("unavailable status slept"),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "status unavailable");
    }
}
