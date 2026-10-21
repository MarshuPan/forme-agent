//! Pure and bounded helpers used by the project-route runtime.

use forme_capabilities::Toolset;
use forme_coordination as c;
use forme_protocol as p;
use std::path::{Component, Path};

pub(super) fn node_record_attempts(record: &p::WorkRouteRecord, id: &str) -> u32 {
    record.nodes[id].attempts
}

pub(super) fn empty_refs() -> p::DecisionRefs {
    p::DecisionRefs {
        map: None,
        user: None,
        agent_self: None,
        trust: None,
        failure: vec![],
    }
}

pub(super) fn coordination_node(
    spec: &p::WorkNode,
    route: &p::RouteDeclaration,
    tools: Toolset,
) -> c::RouteNode {
    c::RouteNode {
        schema_version: p::SchemaVersion(1),
        subtask: c::Subtask {
            schema_version: p::SchemaVersion(1),
            id: spec.id.clone(),
            instruction: spec.instruction.clone(),
            intent_id: spec.intent.clone(),
        },
        role: c::SubagentProfile {
            schema_version: p::SchemaVersion(1),
            role: spec.role.clone(),
            toolset: tools.clone(),
            model: spec.model.clone(),
            permission: spec.scope.clone(),
            budget: p::Budget(format!("units:{}", spec.budget_units)),
        },
        resource_slice: tools,
        done: c::DoneContract::final_output(route.done.clone()),
        retry: c::RetryPolicy {
            schema_version: p::SchemaVersion(1),
            max_attempts: spec.max_attempts,
        },
    }
}

pub(super) fn final_node_output(events: &[p::Event]) -> String {
    let last_started = events.iter().rev().find_map(|e| match &e.payload {
        p::EventPayload::ModelCallStarted(p) => Some(&p.call_id),
        _ => None,
    });
    let final_call = events.iter().rev().find_map(|e| match &e.payload {
        p::EventPayload::ModelCallFinished(p) => Some(&p.call_id),
        _ => None,
    });
    // A tool proposal is not a final answer even when its delta happens to
    // contain text matching a completion predicate.
    let classified_final = events.iter().rev().find_map(|e| match &e.payload {
        p::EventPayload::OutputClassified(p) => Some(p.kind == p::OutputKind::Final),
        _ => None,
    }) == Some(true);
    if !classified_final || last_started.is_some_and(|s| Some(s) != final_call) {
        return String::new();
    }
    events
        .iter()
        .filter_map(|e| match &e.payload {
            p::EventPayload::ModelCallDelta(p) if Some(&p.call_id) == final_call => {
                Some(p.delta.as_str())
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn signature_from_protocol(s: &p::TaskSignature) -> c::ApplicabilitySignature {
    c::ApplicabilitySignature {
        schema_version: s.schema_version,
        decomposability: match s.partition {
            p::TaskPartition::Whole => c::Decomposability::Whole,
            p::TaskPartition::Sequential => c::Decomposability::Sequential,
            p::TaskPartition::Parallel => c::Decomposability::Parallel,
            p::TaskPartition::Iterative => c::Decomposability::Iterative,
        },
        dependency: match s.dependency {
            p::TaskDependency::Independent => c::DependencyShape::Independent,
            p::TaskDependency::Interdependent => c::DependencyShape::Interdependent,
        },
        verifiability: match s.verification {
            p::TaskVerification::SelfCheck => c::Verifiability::SelfCheck,
            p::TaskVerification::Environment => c::Verifiability::Environment,
            p::TaskVerification::IndependentReview => c::Verifiability::IndependentReview,
        },
        clarity: match s.clarity {
            p::TaskClarity::Clear => c::GoalClarity::Clear,
            p::TaskClarity::Clarify => c::GoalClarity::Clarify,
            p::TaskClarity::Explore => c::GoalClarity::Explore,
        },
        scale: match s.extent {
            p::TaskExtent::Single => c::TaskScale::Single,
            p::TaskExtent::MultiStage => c::TaskScale::MultiStage,
            p::TaskExtent::LongRunning => c::TaskScale::LongRunning,
        },
        reversibility: match s.reversibility {
            p::TaskReversibility::LowRiskReversible => c::Reversibility::LowRiskReversible,
            p::TaskReversibility::HighRiskIrreversible => c::Reversibility::HighRiskIrreversible,
        },
        case_anchors: s.anchors.clone(),
        fitness: p::Confidence(0.5),
    }
}

pub(super) fn verify_work_node(
    spec: &p::WorkNode,
    events: &[p::Event],
    output: &str,
    directory: &Path,
) -> p::Result<(p::VerificationOutcome, Vec<p::EvidenceRef>)> {
    let mut evidence = Vec::new();
    for requirement in &spec.requirements {
        let ok = match requirement {
            p::RouteRequirement::OutputContains(expected) => {
                !expected.is_empty() && output.contains(expected)
            }
            p::RouteRequirement::ActionSucceeded => events
                .iter()
                .any(|e| e.kind == p::EventKind::ActionCompleted),
            p::RouteRequirement::FileDigest {
                relative_path,
                digest,
            } => {
                let relative = Path::new(relative_path);
                if relative
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
                    || relative_path.is_empty()
                {
                    return Err(p::Error(
                        "file verification path is outside child workspace".into(),
                    ));
                }
                let mut file = directory.to_path_buf();
                for part in relative.components() {
                    file.push(part);
                    if std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
                        return Err(p::Error("file verification cannot follow a link".into()));
                    }
                    if file.exists() && !file.canonicalize().is_ok_and(|p| p == file) {
                        return Err(p::Error(
                            "file verification cannot follow a directory alias".into(),
                        ));
                    }
                }
                std::fs::read(&file)
                    .is_ok_and(|bytes| p::sha256_content_digest(&bytes).0 == *digest)
            }
        };
        if !ok {
            return Ok((
                p::VerificationOutcome::Fail,
                events
                    .iter()
                    .map(|e| p::EvidenceRef(e.event_id.0.clone()))
                    .take(100)
                    .collect(),
            ));
        }
    }
    evidence.extend(
        events
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    p::EventKind::RunComplete
                        | p::EventKind::ActionCompleted
                        | p::EventKind::ModelCallFinished
                )
            })
            .map(|e| p::EvidenceRef(e.event_id.0.clone()))
            .take(100),
    );
    if evidence.is_empty() {
        Ok((
            p::VerificationOutcome::Unverifiable(p::ReasonRef(
                "no independent runtime result".into(),
            )),
            evidence,
        ))
    } else {
        Ok((p::VerificationOutcome::Pass, evidence))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c16_final_verification_ignores_old_answers_and_unfinished_calls() {
        let payloads = vec![
            p::EventPayload::ModelCallDelta(p::ModelCallDeltaPayload {
                call_id: p::ModelCallId("old".into()),
                delta: "ground-truth".into(),
            }),
            p::EventPayload::ModelCallFinished(p::ModelCallFinishedPayload {
                call_id: p::ModelCallId("old".into()),
                model_profile: p::ModelProfileRef("model:test".into()),
                usage: p::ModelUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                finish_reason: p::FinishReason("stop".into()),
            }),
            p::EventPayload::OutputClassified(p::OutputClassifiedPayload {
                kind: p::OutputKind::Final,
            }),
            p::EventPayload::ModelCallDelta(p::ModelCallDeltaPayload {
                call_id: p::ModelCallId("new".into()),
                delta: "wrong".into(),
            }),
            p::EventPayload::ModelCallFinished(p::ModelCallFinishedPayload {
                call_id: p::ModelCallId("new".into()),
                model_profile: p::ModelProfileRef("model:test".into()),
                usage: p::ModelUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                finish_reason: p::FinishReason("stop".into()),
            }),
            p::EventPayload::OutputClassified(p::OutputClassifiedPayload {
                kind: p::OutputKind::Final,
            }),
        ];
        let mut events = payloads
            .into_iter()
            .enumerate()
            .map(|(i, payload)| {
                p::Event::new(
                    p::EventId(format!("e:{i}")),
                    p::RunId("run:test".into()),
                    None,
                    payload,
                    p::SchemaVersion(1),
                    i as i64,
                    crate::internal_provenance(p::Source::Internal),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(final_node_output(&events), "wrong");
        events.push(p::Event::new(
            p::EventId("e:unfinished".into()),
            p::RunId("run:test".into()),
            None,
            p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
                call_id: p::ModelCallId("unfinished".into()),
                model_profile: p::ModelProfileRef("model:test".into()),
            }),
            p::SchemaVersion(1),
            10,
            crate::internal_provenance(p::Source::Internal),
        ));
        assert_eq!(final_node_output(&events), "");
        events.pop();
        events.last_mut().unwrap().payload =
            p::EventPayload::OutputClassified(p::OutputClassifiedPayload {
                kind: p::OutputKind::Tool,
            });
        assert_eq!(final_node_output(&events), "");
    }
}
