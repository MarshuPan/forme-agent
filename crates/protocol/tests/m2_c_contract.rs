use forme_protocol as p;

fn envelope() -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:alpha/resource:lint".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef("capability:lint".into())],
            permissions: vec![p::PermissionRef("permission:read".into())],
        },
        action_type: vec![p::ActionType::Analyze],
        risk_limit: p::Risk::Low,
        approval_rule: p::ApprovalRule::Ask,
        budget: p::Budget("units:2".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 10,
            expires_at: 20,
            max_turns: 2,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: true,
            boundary: Some(p::RollbackBoundary("discard prepared output".into())),
        },
    }
}

fn proposal() -> p::CapabilityUpdateProposal {
    p::CapabilityUpdateProposal {
        schema_version: p::SchemaVersion(1),
        reference: p::CapabilityUpdateProposalRef("capability-update:lint".into()),
        candidate_id: p::CandidateId("candidate:capability-update:lint".into()),
        gap: p::CapabilityGap {
            schema_version: p::SchemaVersion(1),
            reference: p::CapabilityGapRef("capability-gap:lint".into()),
            capability: p::CapabilityRef("capability:lint".into()),
            scope: p::Scope("workspace:alpha/resource:lint".into()),
            result_evidence: vec![p::CapabilityResultEvidence {
                schema_version: p::SchemaVersion(1),
                evidence_ref: p::EvidenceRef("verification:lint:1".into()),
                outcome: p::ResourceEvidenceOutcome::Pass,
                observed_at: 12,
                owner_feedback: Some(true),
            }],
            self_confidence: Some(p::Confidence(0.6)),
            ceiling: p::InterventionLevel::L3ActWithApproval,
        },
        requested_envelope: envelope(),
        evidence_refs: vec![p::EvidenceRef("verification:lint:1".into())],
    }
}

#[test]
fn m2_c_objects_are_versioned_round_trip_and_sync_is_additive() {
    let graph = p::ResourceGraphSnapshot {
        schema_version: p::SchemaVersion(1),
        reference: p::ResourceGraphSnapshotRef("resource-graph:7".into()),
        aggregate_versions: vec![p::AggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: p::RunId("run:resource".into()),
            value: 7,
        }],
        nodes: vec![p::ResourceNode {
            schema_version: p::SchemaVersion(1),
            resource: p::ResourceRef("capability:lint".into()),
            kind: p::ResourceKind::Tool,
            scope: p::Scope("workspace:alpha".into()),
            available: true,
            score: p::ResourceScore {
                schema_version: p::SchemaVersion(1),
                passed: 2,
                failed: 1,
                unverifiable: 0,
                latest_at: 12,
            },
            evidence_refs: vec![p::EventId("event:capability".into())],
        }],
        edges: Vec::new(),
        evidence_refs: vec![p::EventId("event:capability".into())],
        built_at: 13,
    };
    graph.validate().unwrap();
    proposal().validate().unwrap();

    let policy = p::ManagedPluginPolicy {
        schema_version: p::SchemaVersion(1),
        reference: p::ManagedPluginPolicyRef("managed-policy:1".into()),
        version: p::Version(1),
        allowed_plugins: vec![p::PluginRef("plugin:lint".into())],
        denied_plugins: Vec::new(),
        allowed_sources: vec![p::ManagedPluginSourceRef("source:local-admin".into())],
        revoked_manifests: Vec::new(),
        require_signature: true,
    };
    policy.validate().unwrap();

    for value in [
        serde_json::to_value(&graph).unwrap(),
        serde_json::to_value(proposal()).unwrap(),
        serde_json::to_value(policy).unwrap(),
    ] {
        assert_eq!(value["schema_version"], serde_json::json!(1));
    }
    assert_eq!(
        serde_json::from_value::<p::ConfigCheck>(serde_json::json!("Sync")).unwrap(),
        p::ConfigCheck::Sync
    );
    assert_eq!(p::EventKind::ALL.len(), 99);
    assert_eq!(p::EventKind::ALL[85], p::EventKind::ComplianceCheckResult);
    assert_eq!(
        p::EventKind::ALL[86],
        p::EventKind::EvolutionEvaluationRecorded
    );
}

#[test]
fn m2_c_payload_additions_decode_legacy_as_absent() {
    let candidate: p::CandidateCreatedPayload = serde_json::from_value(serde_json::json!({
        "candidate_id": "candidate:legacy",
        "target": "target:legacy",
        "evidence_refs": [],
        "confidence": 0.3,
        "provenance": {
            "source": "Internal",
            "actor": "System",
            "trust_tier": "VerifiedProcess",
            "caused_by": null
        },
        "target_tier": "Working"
    }))
    .unwrap();
    assert!(candidate.capability_update.is_none());

    let trace: p::DecisionTraceRecordedPayload = serde_json::from_value(serde_json::json!({
        "trace_ref": "trace:legacy",
        "refs": {"map": null, "user": null, "agent_self": null, "trust": null, "failure": []},
        "rationale": "legacy rationale",
        "workspace_snapshot": "workspace:legacy"
    }))
    .unwrap();
    assert!(trace.resource_graph_snapshot.is_none());

    let route: p::OrchestrationRouteCreatedPayload = serde_json::from_value(serde_json::json!({
        "pattern_ref": null,
        "route": "route:legacy"
    }))
    .unwrap();
    assert!(route.goal_frame.is_none());
    assert!(route.checkpoint.is_none());
}

#[test]
fn m2_c_sync_contract_rejects_zero_or_cross_aggregate_batches() {
    let peer = p::SyncPeer {
        schema_version: p::SchemaVersion(1),
        reference: p::SyncPeerRef("peer:laptop".into()),
        owner: p::VerifiedPrincipal("owner:local".into()),
        allowed_scopes: vec![p::Scope("workspace:alpha".into())],
    };
    let event = p::Event::new(
        p::EventId("event:sync".into()),
        p::RunId("run:other".into()),
        None,
        p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 1 }),
        p::SchemaVersion(1),
        1,
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        },
    );
    let invalid = p::SyncWriteBatch {
        schema_version: p::SchemaVersion(1),
        batch_id: p::SyncBatchId("batch:1".into()),
        peer,
        aggregate: p::RunId("run:target".into()),
        expected_version: p::AggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: p::RunId("run:target".into()),
            value: 0,
        },
        events: vec![event],
    };
    assert!(invalid.validate().is_err());
}
