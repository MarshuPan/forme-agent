use std::collections::{BTreeMap, BTreeSet};

use forme_protocol as p;

#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceGraphProjector;

impl ResourceGraphProjector {
    pub fn rebuild(
        &self,
        events: &[p::Event],
        reference: p::ResourceGraphSnapshotRef,
        built_at: p::Timestamp,
    ) -> p::Result<p::ResourceGraphSnapshot> {
        if reference.0.trim().is_empty() || built_at <= 0 || events.is_empty() {
            return Err(p::Error(
                "resource graph rebuild boundary is incomplete".into(),
            ));
        }
        let mut ordered = events.to_vec();
        ordered.sort_by(|left, right| {
            left.run_id
                .cmp(&right.run_id)
                .then_with(|| left.stream_seq.cmp(&right.stream_seq))
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        if ordered.iter().any(|event| event.stream_seq == 0) {
            return Err(p::Error(
                "resource graph only accepts store-sequenced events".into(),
            ));
        }

        let mut aggregate_versions = BTreeMap::<p::RunId, u64>::new();
        let mut nodes = BTreeMap::<(p::ResourceKind, p::ResourceRef), p::ResourceNode>::new();
        let mut edges = BTreeMap::<
            (
                p::ResourceRef,
                p::ResourceRef,
                p::ResourceRelation,
                p::Scope,
            ),
            p::ResourceEdge,
        >::new();
        let mut last_resource = BTreeMap::<p::RunId, (p::ResourceKind, p::ResourceRef)>::new();
        let mut evidence_refs = BTreeSet::new();

        for event in &ordered {
            aggregate_versions
                .entry(event.run_id.clone())
                .and_modify(|version| *version = (*version).max(event.stream_seq))
                .or_insert(event.stream_seq);

            match &event.payload {
                p::EventPayload::ToolCallProposed(payload) => {
                    let key = ensure_node(
                        &mut nodes,
                        p::ResourceKind::Tool,
                        p::ResourceRef(payload.tool.0.clone()),
                        run_scope(event),
                        event,
                    );
                    last_resource.insert(event.run_id.clone(), key);
                    evidence_refs.insert(event.event_id.clone());
                }
                p::EventPayload::SkillMetadataExposed(payload) => {
                    for skill in &payload.skills {
                        let key = ensure_node(
                            &mut nodes,
                            p::ResourceKind::Skill,
                            p::ResourceRef(skill.0.clone()),
                            payload.scope.clone(),
                            event,
                        );
                        last_resource.insert(event.run_id.clone(), key);
                    }
                    evidence_refs.insert(event.event_id.clone());
                }
                p::EventPayload::SkillBodyLoaded(payload) => {
                    let key = ensure_node(
                        &mut nodes,
                        p::ResourceKind::Skill,
                        p::ResourceRef(payload.skill.0.clone()),
                        payload.scope.clone(),
                        event,
                    );
                    last_resource.insert(event.run_id.clone(), key);
                    evidence_refs.insert(event.event_id.clone());
                }
                p::EventPayload::ModelCallStarted(payload) => {
                    let key = ensure_node(
                        &mut nodes,
                        p::ResourceKind::Model,
                        p::ResourceRef(payload.model_profile.0.clone()),
                        run_scope(event),
                        event,
                    );
                    last_resource.insert(event.run_id.clone(), key);
                    evidence_refs.insert(event.event_id.clone());
                }
                p::EventPayload::MemoryNodeAppended(payload) => {
                    let key = ensure_node(
                        &mut nodes,
                        p::ResourceKind::Memory,
                        p::ResourceRef(payload.node_id.0.clone()),
                        payload.scope.clone(),
                        event,
                    );
                    last_resource.insert(event.run_id.clone(), key);
                    evidence_refs.insert(event.event_id.clone());
                }
                p::EventPayload::ImportedHistoricalEvidenceRecorded(payload) => {
                    let key = ensure_node(
                        &mut nodes,
                        p::ResourceKind::Source,
                        p::ResourceRef(payload.source.0.clone()),
                        run_scope(event),
                        event,
                    );
                    last_resource.insert(event.run_id.clone(), key);
                    evidence_refs.insert(event.event_id.clone());
                }
                p::EventPayload::CapabilityIndexed(payload) => {
                    let capability = p::ResourceRef(payload.capability.0.clone());
                    let kind = existing_kind(&nodes, &capability).unwrap_or(p::ResourceKind::Tool);
                    let capability_key = ensure_node(
                        &mut nodes,
                        kind,
                        capability.clone(),
                        run_scope(event),
                        event,
                    );
                    for source in &payload.sources {
                        let source_ref = p::ResourceRef(source.0.clone());
                        ensure_node(
                            &mut nodes,
                            p::ResourceKind::Source,
                            source_ref.clone(),
                            run_scope(event),
                            event,
                        );
                        add_edge(
                            &mut edges,
                            source_ref,
                            capability.clone(),
                            p::ResourceRelation::Provides,
                            run_scope(event),
                            event,
                        );
                    }
                    last_resource.insert(event.run_id.clone(), capability_key);
                    evidence_refs.insert(event.event_id.clone());
                }
                p::EventPayload::ActionPlanned(payload) => {
                    let backend = p::ResourceRef(format!("backend:{:?}", payload.backend));
                    let backend_key = ensure_node(
                        &mut nodes,
                        p::ResourceKind::Backend,
                        backend.clone(),
                        payload.scope.clone(),
                        event,
                    );
                    if let Some((_, prior)) = last_resource.get(&event.run_id) {
                        add_edge(
                            &mut edges,
                            prior.clone(),
                            backend,
                            p::ResourceRelation::UsedBy,
                            payload.scope.clone(),
                            event,
                        );
                    }
                    last_resource.insert(event.run_id.clone(), backend_key);
                    evidence_refs.insert(event.event_id.clone());
                }
                p::EventPayload::CapabilityEvidenceRecorded(payload) if process_evidence(event) => {
                    let resource = p::ResourceRef(payload.capability.0.clone());
                    let kind = existing_kind(&nodes, &resource).unwrap_or(p::ResourceKind::Tool);
                    let key = ensure_node(&mut nodes, kind, resource, run_scope(event), event);
                    apply_outcome(
                        nodes
                            .get_mut(&key)
                            .expect("resource node exists after ensure"),
                        capability_outcome(&payload.outcome),
                        event,
                    );
                    last_resource.insert(event.run_id.clone(), key);
                    evidence_refs.insert(event.event_id.clone());
                }
                p::EventPayload::VerificationFinished(payload) if process_evidence(event) => {
                    if let Some(key) = last_resource.get(&event.run_id) {
                        let outcome = match payload.outcome {
                            p::VerificationOutcome::Pass => p::ResourceEvidenceOutcome::Pass,
                            p::VerificationOutcome::Fail => p::ResourceEvidenceOutcome::Fail,
                            p::VerificationOutcome::Unverifiable(_) => {
                                p::ResourceEvidenceOutcome::Unverifiable
                            }
                        };
                        apply_outcome(
                            nodes
                                .get_mut(key)
                                .expect("last resource points at an existing node"),
                            outcome,
                            event,
                        );
                        evidence_refs.insert(event.event_id.clone());
                    }
                }
                p::EventPayload::FailureEvidenceRecorded(payload) if process_evidence(event) => {
                    let mut matched = false;
                    for related in &payload.related_refs {
                        let keys = nodes
                            .keys()
                            .filter(|(_, resource)| resource.0 == related.0)
                            .cloned()
                            .collect::<Vec<_>>();
                        for key in keys {
                            apply_outcome(
                                nodes.get_mut(&key).expect("matched resource node exists"),
                                p::ResourceEvidenceOutcome::Fail,
                                event,
                            );
                            matched = true;
                        }
                    }
                    if !matched {
                        if let Some(key) = last_resource.get(&event.run_id) {
                            apply_outcome(
                                nodes
                                    .get_mut(key)
                                    .expect("last resource points at an existing node"),
                                p::ResourceEvidenceOutcome::Fail,
                                event,
                            );
                        }
                    }
                    evidence_refs.insert(event.event_id.clone());
                }
                p::EventPayload::RevocationEvent(payload) if process_evidence(event) => {
                    for node in nodes
                        .values_mut()
                        .filter(|node| node.resource.0 == payload.target_object.0)
                    {
                        node.available = false;
                        apply_outcome(node, p::ResourceEvidenceOutcome::Revoked, event);
                    }
                    evidence_refs.insert(event.event_id.clone());
                }
                _ => {}
            }
        }

        let snapshot = p::ResourceGraphSnapshot {
            schema_version: p::SchemaVersion(1),
            reference,
            aggregate_versions: aggregate_versions
                .into_iter()
                .map(|(aggregate, value)| p::AggregateVersion {
                    schema_version: p::SchemaVersion(1),
                    aggregate,
                    value,
                })
                .collect(),
            nodes: nodes.into_values().collect(),
            edges: edges.into_values().collect(),
            evidence_refs: evidence_refs.into_iter().collect(),
            built_at,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

fn run_scope(event: &p::Event) -> p::Scope {
    p::Scope(format!("run:{}", event.run_id.0))
}

fn existing_kind(
    nodes: &BTreeMap<(p::ResourceKind, p::ResourceRef), p::ResourceNode>,
    reference: &p::ResourceRef,
) -> Option<p::ResourceKind> {
    nodes
        .keys()
        .find_map(|(kind, resource)| (resource == reference).then_some(*kind))
}

fn ensure_node(
    nodes: &mut BTreeMap<(p::ResourceKind, p::ResourceRef), p::ResourceNode>,
    kind: p::ResourceKind,
    resource: p::ResourceRef,
    scope: p::Scope,
    event: &p::Event,
) -> (p::ResourceKind, p::ResourceRef) {
    let key = (kind, resource.clone());
    let node = nodes.entry(key.clone()).or_insert_with(|| p::ResourceNode {
        schema_version: p::SchemaVersion(1),
        resource,
        kind,
        scope,
        available: true,
        score: p::ResourceScore {
            schema_version: p::SchemaVersion(1),
            passed: 0,
            failed: 0,
            unverifiable: 0,
            latest_at: event.ts_unix_ms,
        },
        evidence_refs: Vec::new(),
    });
    push_evidence(node, event);
    key
}

fn add_edge(
    edges: &mut BTreeMap<
        (
            p::ResourceRef,
            p::ResourceRef,
            p::ResourceRelation,
            p::Scope,
        ),
        p::ResourceEdge,
    >,
    from: p::ResourceRef,
    to: p::ResourceRef,
    relation: p::ResourceRelation,
    scope: p::Scope,
    event: &p::Event,
) {
    let key = (from.clone(), to.clone(), relation, scope.clone());
    let edge = edges.entry(key).or_insert_with(|| p::ResourceEdge {
        schema_version: p::SchemaVersion(1),
        from,
        to,
        relation,
        scope,
        fresh_at: event.ts_unix_ms,
        evidence_refs: Vec::new(),
    });
    edge.fresh_at = edge.fresh_at.max(event.ts_unix_ms);
    if !edge.evidence_refs.contains(&event.event_id) {
        edge.evidence_refs.push(event.event_id.clone());
    }
}

fn process_evidence(event: &p::Event) -> bool {
    matches!(
        event.provenance.actor,
        p::Actor::Owner | p::Actor::Agent | p::Actor::System
    )
}

fn capability_outcome(outcome: &p::CapabilityOutcome) -> p::ResourceEvidenceOutcome {
    match outcome.0.to_ascii_lowercase().as_str() {
        "success" | "passed" | "pass" | "completed" => p::ResourceEvidenceOutcome::Pass,
        "failure" | "failed" | "cancelled" | "denied" => p::ResourceEvidenceOutcome::Fail,
        _ => p::ResourceEvidenceOutcome::Unverifiable,
    }
}

fn apply_outcome(
    node: &mut p::ResourceNode,
    outcome: p::ResourceEvidenceOutcome,
    event: &p::Event,
) {
    match outcome {
        p::ResourceEvidenceOutcome::Pass => node.score.passed += 1,
        p::ResourceEvidenceOutcome::Fail => node.score.failed += 1,
        p::ResourceEvidenceOutcome::Unverifiable => node.score.unverifiable += 1,
        p::ResourceEvidenceOutcome::Revoked => node.available = false,
    }
    node.score.latest_at = node.score.latest_at.max(event.ts_unix_ms);
    push_evidence(node, event);
}

fn push_evidence(node: &mut p::ResourceNode, event: &p::Event) {
    if !node.evidence_refs.contains(&event.event_id) {
        node.evidence_refs.push(event.event_id.clone());
    }
}
