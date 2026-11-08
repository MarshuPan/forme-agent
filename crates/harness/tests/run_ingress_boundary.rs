#[path = "support/core_brain.rs"]
mod support;

use forme_harness::{AgentHarness, HarnessIngress, IngressAuthority, IngressEvent};
use forme_protocol as p;
use std::sync::atomic::Ordering;
use support::{events, Fixture};

fn observation(authority: &IngressAuthority, scope: &p::WorkspaceRef) -> IngressEvent {
    authority.stamp(
        p::EventPayload::ObservationRecorded(p::ObservationRecordedPayload {
            learning_reply: None,
            source: p::Source::Communication,
            scope: p::Scope(scope.0.clone()),
            grant_ref: None,
        }),
        p::Provenance {
            source: p::Source::Communication,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::Untrusted,
            caused_by: None,
        },
    )
}

#[test]
fn ingress_authority_is_checked_before_writes_and_durable_command_replay() {
    let fixture = Fixture::new();
    *fixture.answer.lock().unwrap() = "ingress boundary witness".into();
    let request = p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::Communication,
        session: p::SessionRef("session:ingress-boundary".into()),
        agent_profile: fixture.config.agent_profile.clone(),
        input: p::RunInput("Produce the ingress boundary witness.".into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey("command:ingress-boundary".into())),
    };
    let (original_run, old_authority, original_events, model_calls) = {
        let runtime = fixture.build();
        let harness = runtime.harness();
        let authority = harness.ingress_authority();
        let before = events(&runtime);
        // One foreign member invalidates the whole submission, including the
        // preceding valid member; it must not consume this command's key.
        let mixed = vec![
            observation(&authority, &fixture.config.workspace),
            observation(&IngressAuthority::default(), &fixture.config.workspace),
        ];
        assert!(harness.submit_ingress(request.clone(), mixed).is_err());
        assert_eq!(events(&runtime), before);
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);

        let run = harness
            .submit_ingress(
                request.clone(),
                vec![observation(&authority, &fixture.config.workspace)],
            )
            .unwrap();
        assert_eq!(
            harness.wait(run.clone()).unwrap().status,
            p::RunStatus::Complete
        );
        let committed = events(&runtime);
        assert_eq!(
            committed
                .iter()
                .filter(
                    |event| event.run_id == run && event.kind == p::EventKind::ObservationRecorded
                )
                .count(),
            1
        );
        let calls = fixture.calls.load(Ordering::SeqCst);
        assert!(calls > 0);
        (run, authority, committed, calls)
    };

    let runtime = fixture.build();
    let harness = runtime.harness();
    assert_ne!(old_authority, harness.ingress_authority());
    assert!(harness
        .submit_ingress(
            request.clone(),
            vec![observation(&old_authority, &fixture.config.workspace)],
        )
        .is_err());
    assert_eq!(events(&runtime), original_events);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), model_calls);

    let replay = harness
        .submit_ingress(
            request.clone(),
            vec![observation(
                &harness.ingress_authority(),
                &fixture.config.workspace,
            )],
        )
        .unwrap();
    assert_eq!(replay, original_run);
    assert_eq!(harness.wait(replay).unwrap().status, p::RunStatus::Complete);
    assert_eq!(events(&runtime), original_events);

    let mut conflicting = request;
    conflicting.input = p::RunInput("Different semantics for the same command.".into());
    assert!(harness.submit_ingress(conflicting, vec![]).is_err());
    assert_eq!(events(&runtime), original_events);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), model_calls);
}
