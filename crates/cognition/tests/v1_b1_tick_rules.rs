//! B1: the deterministic rules of the owner-brain tick (prd/23 B1). A window's
//! signals, reflections, and report digest derive only from its range, so a
//! retried window derives exactly what its first attempt derived.

use std::collections::BTreeMap;

use forme_cognition::{
    CognitiveTickPhase, CognitiveTickPhaseReport, CognitiveTickPhaseStatus, CognitiveTickReport,
    MapUpdateKind, TickSignals, MAX_TICK_FAILURES,
};
use forme_protocol as p;
use forme_store::{
    CognitiveTickClaim, TickEventCursor, TickEventRange, COGNITIVE_TICK_SCHEMA_VERSION,
};

fn event(id: &str, payload: p::EventPayload) -> p::Event {
    p::Event::new(
        p::EventId(id.into()),
        p::RunId("run:b1-rules".into()),
        None,
        payload,
        p::SchemaVersion(1),
        1,
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        },
    )
}

fn recorded(id: &str, failure: &str, scope: &str, impact: p::Impact) -> p::Event {
    event(
        id,
        p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
            failure_ref: p::FailureEvidenceRef(failure.into()),
            class: p::FailureClass::ExecutionFailure,
            impact,
            scope: p::Scope(scope.into()),
            related_refs: Vec::new(),
            suggested_fix: None,
        }),
    )
}

fn action_failed(id: &str, failure: &str) -> p::Event {
    event(
        id,
        p::EventPayload::ActionFailed(p::ActionFailedPayload {
            intent_id: p::ActionId(format!("action:{id}")),
            failure_ref: p::FailureEvidenceRef(failure.into()),
            remote_lease: None,
        }),
    )
}

fn run_failed(id: &str) -> p::Event {
    event(
        id,
        p::EventPayload::RunFailed(p::RunFailedPayload {
            stop_reason: p::StopReason("the step failed".into()),
            result_ref: None,
        }),
    )
}

fn verification(id: &str, outcome: p::VerificationOutcome) -> p::Event {
    event(
        id,
        p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
            verifier_kind: p::VerifierKind("deterministic".into()),
            outcome,
            against: p::DoneContractRef("contract:b1".into()),
        }),
    )
}

#[test]
fn signals_represent_each_failure_once_and_put_unknown_outcomes_first() {
    let events = vec![
        recorded("e1", "failure:deploy", "workspace:b1", p::Impact::High),
        action_failed("e2", "failure:deploy"),
        action_failed("e3", "failure:lint"),
        run_failed("e4"),
        verification(
            "e5",
            p::VerificationOutcome::Unverifiable(p::ReasonRef("no done contract".into())),
        ),
        verification("e6", p::VerificationOutcome::Fail),
        event(
            "e7",
            p::EventPayload::ActionOutcomeUnknown(p::ActionOutcomeUnknownPayload {
                intent_id: p::ActionId("action:e7".into()),
                probe_hint: p::ProbeHintRef("probe:e7".into()),
                remote_lease: None,
            }),
        ),
        event(
            "e8",
            p::EventPayload::RunComplete(p::RunCompletePayload {
                stop_reason: p::StopReason("done".into()),
                result_ref: None,
            }),
        ),
    ];
    let signals = TickSignals::collect(&events);
    assert_eq!(signals.events, 8);
    assert_eq!(signals.outcomes, 2, "one failed and one completed run");
    let classes = signals
        .failures
        .iter()
        .map(|failure| (failure.source.0.as_str(), failure.class.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        classes,
        vec![
            ("e7", "outcome-unknown"),
            ("e1", "failure:execution"),
            ("e3", "action-failed"),
            ("e4", "run-failed"),
            ("e6", "verification-failed"),
        ],
        "the recorded failure stands for its action failure; an unverifiable \
         outcome is no failure; an unknown outcome is most pressing"
    );
    assert_eq!(signals.unobserved_failures, 0);
    assert_eq!(signals.groups.len(), 5);
    assert!(signals.groups[0].outcome_unknown);
    let recorded_group = signals
        .groups
        .iter()
        .find(|group| group.class == "failure:execution")
        .unwrap();
    assert_eq!(recorded_group.scope, p::Scope("workspace:b1".into()));
    assert_eq!(recorded_group.impact, p::Impact::High);
    assert_eq!(
        recorded_group.evidence,
        vec![p::EvidenceRef("failure:deploy".into())]
    );
}

#[test]
fn failures_beyond_the_observation_bound_still_reach_reflection() {
    let events = (0..MAX_TICK_FAILURES + 4)
        .map(|index| run_failed(&format!("f{index:02}")))
        .collect::<Vec<_>>();
    let signals = TickSignals::collect(&events);
    assert_eq!(signals.failures.len(), MAX_TICK_FAILURES);
    assert_eq!(signals.unobserved_failures, 4);
    assert_eq!(signals.groups.len(), 1);
    assert_eq!(signals.groups[0].count, MAX_TICK_FAILURES + 4);
    assert_eq!(signals.groups[0].evidence.len(), MAX_TICK_FAILURES + 4);
}

#[test]
fn reflections_are_deterministic_low_confidence_blind_spots() {
    let first = TickSignals::collect(&[recorded(
        "r1",
        "failure:a",
        "workspace:b1",
        p::Impact::Medium,
    )]);
    let again = TickSignals::collect(&[recorded(
        "r1",
        "failure:a",
        "workspace:b1",
        p::Impact::Medium,
    )]);
    let other = TickSignals::collect(&[recorded(
        "r2",
        "failure:b",
        "workspace:b1",
        p::Impact::Medium,
    )]);
    let reflection = first.groups[0].reflection();
    assert_eq!(reflection, again.groups[0].reflection());
    assert_eq!(reflection.kind, MapUpdateKind::BlindSpot);
    assert!(reflection.confidence.0 <= 0.5);
    assert_eq!(
        reflection.evidence,
        vec![p::EvidenceRef("failure:a".into())]
    );
    assert!(reflection.statement.contains("failure:execution failures"));
    assert_ne!(
        first.groups[0].candidate_id(),
        other.groups[0].candidate_id(),
        "different failures are different candidates"
    );
    assert_eq!(
        first.failures[0].observation(),
        again.failures[0].observation()
    );
}

fn claim() -> CognitiveTickClaim {
    let cursor = |position| {
        TickEventCursor(BTreeMap::from([(
            p::RunId("run:b1-rules".into()),
            position,
        )]))
    };
    CognitiveTickClaim {
        schema_version: COGNITIVE_TICK_SCHEMA_VERSION,
        owner_digest: p::sha256_content_digest(b"owner:b1-rules"),
        window: 1_000,
        lease_nonce: p::Nonce("tick:rules".into()),
        attempt: 1,
        expires_at: 2_000,
        range: TickEventRange {
            from: cursor(2),
            to: cursor(9),
        },
    }
}

fn phases(reflect: CognitiveTickPhaseReport) -> Vec<CognitiveTickPhaseReport> {
    vec![
        CognitiveTickPhaseReport::completed(
            CognitiveTickPhase::ObserveAndPropose,
            true,
            vec![p::EventId("brain-tick:1000:attempt:1:event:1".into())],
        ),
        reflect,
        CognitiveTickPhaseReport::completed(CognitiveTickPhase::MaintainMemory, false, Vec::new()),
    ]
}

#[test]
fn a_report_lists_its_phases_in_order_and_consumes_by_deferral() {
    let run = p::RunId("brain-tick:1000".into());
    let ran = CognitiveTickReport::seal(
        claim(),
        run.clone(),
        phases(CognitiveTickPhaseReport::completed(
            CognitiveTickPhase::ReflectAndCandidate,
            true,
            Vec::new(),
        )),
    )
    .unwrap();
    assert_eq!(ran.consumed, claim().range.to);
    assert_eq!(ran.event_range, claim().range);
    assert!(!ran.deferred() && !ran.failed());
    assert_eq!(ran.model_calls(), 0);

    let deferred = CognitiveTickReport::seal(
        claim(),
        run.clone(),
        phases(CognitiveTickPhaseReport::deferred(
            CognitiveTickPhase::ReflectAndCandidate,
            "a foreground session is active",
        )),
    )
    .unwrap();
    assert_eq!(
        deferred.consumed,
        claim().range.from,
        "deferred inputs wait for a later window"
    );
    assert_ne!(deferred.digest, ran.digest);

    let resealed = CognitiveTickReport::seal(
        claim(),
        run.clone(),
        phases(CognitiveTickPhaseReport::completed(
            CognitiveTickPhase::ReflectAndCandidate,
            true,
            Vec::new(),
        )),
    )
    .unwrap();
    assert_eq!(resealed.digest, ran.digest, "the digest is deterministic");

    let mut reordered = phases(CognitiveTickPhaseReport::completed(
        CognitiveTickPhase::ReflectAndCandidate,
        true,
        Vec::new(),
    ));
    reordered.swap(0, 1);
    assert!(CognitiveTickReport::seal(claim(), run.clone(), reordered).is_err());
    let mut missing = phases(CognitiveTickPhaseReport::completed(
        CognitiveTickPhase::ReflectAndCandidate,
        true,
        Vec::new(),
    ));
    missing.pop();
    assert!(CognitiveTickReport::seal(claim(), run, missing).is_err());
    assert_eq!(
        CognitiveTickPhaseStatus::Deferred.as_str(),
        "deferred",
        "statuses have stable names"
    );
}
