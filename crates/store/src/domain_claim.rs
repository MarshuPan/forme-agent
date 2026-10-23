//! Durable domain command keys. All claims still use the event append transaction.
use forme_protocol as p;

pub(super) fn validate_resolution_claim(event: &p::Event, has_companion: bool) -> p::Result<()> {
    let claim = match &event.payload {
        p::EventPayload::RunResumed(payload)
            if payload.resume_ref.0.starts_with("control-resolution-v1|") =>
        {
            Some(
                p::ControlResolutionBinding::from_ref(&payload.resume_ref)?
                    .ok_or_else(|| p::Error("control claim is malformed".into()))?,
            )
        }
        _ => None,
    };
    if claim.is_some() {
        if !has_companion {
            return Err(p::Error(
                "control resolution claim requires an atomic protected companion".into(),
            ));
        }
        if event.provenance.source != p::Source::OwnerControl
            || event.provenance.actor != p::Actor::Owner
            || event.provenance.trust_tier != p::TrustTier::OwnerInput
        {
            return Err(p::Error("control claim requires owner provenance".into()));
        }
    }
    Ok(())
}

pub(super) fn domain_idempotency_key(event: &p::Event) -> Option<(&'static str, String)> {
    match &event.payload {
        p::EventPayload::RunResumed(_) => p::ControlResolutionBinding::from_event(event)
            .ok()
            .flatten()
            .map(|binding| ("control_resolution", binding.command_digest.0)),
        p::EventPayload::DecisionTraceRecorded(payload) if payload.orchestration.is_some() => {
            payload.orchestration.as_ref().map(|f| {
                (
                    "orchestration",
                    format!("{}:{}", event.run_id.0, f.command.0),
                )
            })
        }
        p::EventPayload::RunAccepted(payload) => payload
            .idempotency_key
            .as_ref()
            .map(|key| ("run_request", key.0.clone())),
        p::EventPayload::ActionPlanned(payload) => {
            Some(("action_intent", payload.intent_id.0.clone()))
        }
        p::EventPayload::MemoryMaintenanceApplied(payload)
            if payload.deltas_ref.0.starts_with("graph-deltas:v1:") =>
        {
            p::MemoryMaintenanceDeltaSet::from_ref(&payload.deltas_ref)
                .ok()
                .flatten()
                .map(|set| {
                    (
                        "graph_maintenance",
                        format!("{}:{}", event.run_id.0, set.command.0),
                    )
                })
        }
        p::EventPayload::MemoryMaintenanceApplied(payload) => payload
            .deltas_ref
            .0
            .strip_prefix("intention-lease|")
            .and_then(|value| {
                let mut fields = value.split('|');
                let generation = fields.next()?;
                let _lease_until = fields.next()?;
                let intention = fields.next()?;
                fields
                    .next()
                    .is_none()
                    .then(|| ("intention_lease", format!("{generation}|{intention}")))
            }),
        _ => None,
    }
}
