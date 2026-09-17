//! Thin CLI client over the same RunRequest/EventStream protocol as the gateway (prd/14).
#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_gateway::{
    EvolutionActivationResult, EvolutionEvaluationResult, EvolutionRunGateway, RunGateway,
};
use forme_protocol as p;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliRequest {
    pub schema_version: p::SchemaVersion,
    pub session: p::SessionRef,
    pub question: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub schema_version: p::SchemaVersion,
    pub run: p::RunId,
    pub result: p::RunResult,
    pub answer: String,
    pub event_kinds: Vec<p::EventKind>,
}

pub struct CliClient<'a> {
    gateway: &'a dyn RunGateway,
    sequence: AtomicU64,
}

pub struct EvolutionCliClient<'a> {
    gateway: &'a dyn EvolutionRunGateway,
}

impl<'a> EvolutionCliClient<'a> {
    pub fn new(gateway: &'a dyn EvolutionRunGateway) -> Self {
        Self { gateway }
    }

    pub fn snapshot(&self, scope: p::Scope) -> p::Result<p::EvolutionSnapshot> {
        self.gateway.evolution_snapshot(scope)
    }

    pub fn record_candidate(
        &self,
        run: p::RunId,
        candidate: p::StrategyCandidate,
    ) -> p::Result<p::EventId> {
        self.gateway.record_strategy_candidate(run, candidate)
    }

    pub fn evaluate(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        comparison: p::EvolutionComparison,
    ) -> p::Result<EvolutionEvaluationResult> {
        self.gateway.evaluate_strategy(run, candidate, comparison)
    }

    pub fn promote(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
    ) -> p::Result<p::EventId> {
        self.gateway.promote_strategy(run, candidate, evaluation)
    }

    pub fn activate(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        promotion: p::EventId,
    ) -> p::Result<EvolutionActivationResult> {
        self.gateway
            .activate_strategy(run, aggregate, candidate, evaluation, promotion)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rollback(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        domain: p::StrategyDomain,
        scope: p::Scope,
        restored: p::StrategyVersionRef,
        triggers: Vec<p::EvidenceRef>,
        in_flight: p::InFlightDisposition,
    ) -> p::Result<EvolutionActivationResult> {
        self.gateway
            .rollback_strategy(run, aggregate, domain, scope, restored, triggers, in_flight)
    }

    pub fn set_auto_activation_paused(&self, paused: bool) -> p::Result<()> {
        self.gateway.set_auto_activation_paused(paused)
    }

    pub fn auto_activation_paused(&self) -> p::Result<bool> {
        self.gateway.auto_activation_paused()
    }
}

impl<'a> CliClient<'a> {
    pub fn new(gateway: &'a dyn RunGateway) -> Self {
        Self {
            gateway,
            sequence: AtomicU64::new(1),
        }
    }

    pub fn ask(&self, request: CliRequest) -> p::Result<CliOutput> {
        if request.schema_version.0 == 0
            || request.session.0.trim().is_empty()
            || request.question.trim().is_empty()
        {
            return Err(p::Error("CLI request is incomplete".into()));
        }
        let run_request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::UserTurn,
            session: request.session,
            agent_profile: p::AgentProfileRef("agent:forme-local".into()),
            input: p::RunInput(request.question),
            budget: None,
            idempotency_key: Some(p::IdempotencyKey(format!(
                "cli:{}:{}",
                now_nanos(),
                self.sequence.fetch_add(1, Ordering::SeqCst)
            ))),
        };
        let run = self.gateway.submit_run(run_request)?;
        let result = self.gateway.wait_run(run.clone())?;
        let event_kinds = self
            .gateway
            .stream_run(run.clone())
            .map(|event| event.kind)
            .collect();
        let answer = self
            .gateway
            .answer(run.clone())?
            .ok_or_else(|| p::Error("completed run has no model output".into()))?;
        Ok(CliOutput {
            schema_version: p::SchemaVersion(1),
            run,
            result,
            answer,
            event_kinds,
        })
    }
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    use forme_gateway::{EventStream, EvolutionRunGateway, RunGateway};

    use super::*;

    struct FakeGateway {
        request: Mutex<Option<p::RunRequest>>,
    }

    struct FakeEvolutionGateway {
        paused: AtomicBool,
    }

    impl RunGateway for FakeGateway {
        fn submit_run(&self, request: p::RunRequest) -> p::Result<p::RunId> {
            *self
                .request
                .lock()
                .map_err(|_| p::Error("fake request lock failed".into()))? = Some(request);
            Ok(p::RunId("cli-run".into()))
        }

        fn stream_run(&self, run: p::RunId) -> EventStream {
            let provenance = p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            };
            EventStream::new(vec![
                p::Event::new(
                    p::EventId("accepted".into()),
                    run.clone(),
                    None,
                    p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                        source: p::Source::UserTurn,
                        session_ref: p::SessionId("session:cli".into()),
                        input_ref: p::InputRef("question".into()),
                        idempotency_key: None,
                    }),
                    p::SchemaVersion(1),
                    1,
                    provenance.clone(),
                ),
                p::Event::new(
                    p::EventId("complete".into()),
                    run,
                    None,
                    p::EventPayload::RunComplete(p::RunCompletePayload {
                        stop_reason: p::StopReason("final_output".into()),
                        result_ref: None,
                    }),
                    p::SchemaVersion(1),
                    2,
                    provenance,
                ),
            ])
        }

        fn wait_run(&self, _run: p::RunId) -> p::Result<p::RunResult> {
            Ok(p::RunResult {
                schema_version: p::SchemaVersion(1),
                status: p::RunStatus::Complete,
                stop_reason: p::StopReason("final_output".into()),
                outputs: vec![p::OutputRef("answer".into())],
                evidence_refs: Vec::new(),
            })
        }

        fn answer(&self, _run: p::RunId) -> p::Result<Option<String>> {
            Ok(Some("answer text".into()))
        }
    }

    impl EvolutionRunGateway for FakeEvolutionGateway {
        fn evolution_snapshot(&self, scope: p::Scope) -> p::Result<p::EvolutionSnapshot> {
            Ok(p::EvolutionSnapshot {
                schema_version: p::SchemaVersion(1),
                snapshot: p::EvolutionSnapshotRef(format!("snapshot:{}", scope.0)),
                aggregates: vec![p::EvolutionAggregateVersion {
                    schema_version: p::SchemaVersion(1),
                    aggregate: p::EvolutionAggregateRef("evolution:owner:workspace".into()),
                    value: 1,
                }],
                strategies: vec![p::ActiveStrategyRef {
                    schema_version: p::SchemaVersion(1),
                    id: p::ActiveStrategyId("active:loop:v1".into()),
                    aggregate: p::EvolutionAggregateRef("evolution:owner:workspace".into()),
                    domain: p::StrategyDomain::Loop,
                    scope,
                    version: p::StrategyVersionRef("loop:v1".into()),
                    spec_ref: p::ContentRef("content:loop:v1".into()),
                    spec_digest: p::SchemaDigest("digest:loop:v1".into()),
                    activation_event: p::EventId("event:activation:v1".into()),
                }],
                digest: p::SchemaDigest("digest:snapshot:v1".into()),
            })
        }

        fn record_strategy_candidate(
            &self,
            _run: p::RunId,
            _candidate: p::StrategyCandidate,
        ) -> p::Result<p::EventId> {
            Err(p::Error("not used by this CLI fixture".into()))
        }

        fn evaluate_strategy(
            &self,
            _run: p::RunId,
            _candidate: &p::StrategyCandidate,
            _comparison: p::EvolutionComparison,
        ) -> p::Result<EvolutionEvaluationResult> {
            Err(p::Error("not used by this CLI fixture".into()))
        }

        fn promote_strategy(
            &self,
            _run: p::RunId,
            _candidate: &p::StrategyCandidate,
            _evaluation: &p::EvolutionEvaluation,
        ) -> p::Result<p::EventId> {
            Err(p::Error("not used by this CLI fixture".into()))
        }

        fn activate_strategy(
            &self,
            _run: p::RunId,
            _aggregate: p::EvolutionAggregateRef,
            _candidate: &p::StrategyCandidate,
            _evaluation: &p::EvolutionEvaluation,
            _promotion: p::EventId,
        ) -> p::Result<EvolutionActivationResult> {
            Err(p::Error("not used by this CLI fixture".into()))
        }

        fn rollback_strategy(
            &self,
            _run: p::RunId,
            _aggregate: p::EvolutionAggregateRef,
            _domain: p::StrategyDomain,
            _scope: p::Scope,
            _restored: p::StrategyVersionRef,
            _triggers: Vec<p::EvidenceRef>,
            _in_flight: p::InFlightDisposition,
        ) -> p::Result<EvolutionActivationResult> {
            Err(p::Error("not used by this CLI fixture".into()))
        }

        fn set_auto_activation_paused(&self, paused: bool) -> p::Result<()> {
            self.paused.store(paused, Ordering::SeqCst);
            Ok(())
        }

        fn auto_activation_paused(&self) -> p::Result<bool> {
            Ok(self.paused.load(Ordering::SeqCst))
        }
    }

    #[test]
    fn cli_submits_the_frozen_run_request_and_consumes_gateway_events() {
        let gateway = FakeGateway {
            request: Mutex::new(None),
        };
        let output = CliClient::new(&gateway)
            .ask(CliRequest {
                schema_version: p::SchemaVersion(1),
                session: p::SessionRef("session:cli".into()),
                question: "question".into(),
            })
            .unwrap();
        assert_eq!(output.answer, "answer text");
        assert_eq!(
            output.event_kinds,
            vec![p::EventKind::RunAccepted, p::EventKind::RunComplete]
        );
        let request = gateway.request.lock().unwrap().clone().unwrap();
        assert_eq!(request.source, p::Source::UserTurn);
        assert_eq!(request.input, p::RunInput("question".into()));
        assert!(request.idempotency_key.is_some());
    }

    #[test]
    fn m3_evolution_cli_uses_the_owner_gated_gateway_surface() {
        let gateway = FakeEvolutionGateway {
            paused: AtomicBool::new(false),
        };
        let client = EvolutionCliClient::new(&gateway);
        let snapshot = client.snapshot(p::Scope("workspace".into())).unwrap();
        assert_eq!(snapshot.strategies[0].version.0, "loop:v1");
        assert!(!client.auto_activation_paused().unwrap());
        client.set_auto_activation_paused(true).unwrap();
        assert!(client.auto_activation_paused().unwrap());
    }
}
