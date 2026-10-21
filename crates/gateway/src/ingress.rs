//! Gateway ingress boundary; public facade remains in the crate root.
use super::*;

impl<H> LocalGateway<H>
where
    H: HarnessIngress + Send + Sync,
{
    pub fn new(
        harness: Arc<H>,
        owner: p::VerifiedPrincipal,
        agent_profile: p::AgentProfileRef,
    ) -> p::Result<Self> {
        if owner.0.trim().is_empty() || agent_profile.0.trim().is_empty() {
            return Err(p::Error("local gateway identity is incomplete".into()));
        }
        let ingress_authority = harness.ingress_authority();
        Ok(Self {
            harness,
            production_runtime: None,
            ingress_authority,
            owner,
            agent_profile,
            sequence: AtomicU64::new(1),
        })
    }

    pub fn production_runtime(&self) -> Option<Arc<ProductionRuntime>> {
        self.production_runtime.clone()
    }

    pub fn normalize_and_submit<A: ChannelAdapter>(
        &self,
        adapter: &A,
        raw: RawInput,
        auth: AuthContext,
    ) -> p::Result<p::RunId> {
        let event = adapter.normalize(raw)?;
        self.submit_event_with_auth(event, auth)
    }

    pub fn submit_event_with_auth(
        &self,
        event: CommunicationEvent,
        auth: AuthContext,
    ) -> p::Result<p::RunId> {
        event.validate()?;
        auth.validate(now_ms())?;
        validate_principal(&event.participant, &auth.principal, &self.owner)?;
        if matches!(event.participant, ParticipantProfile::ExternalAgent(_))
            && auth.method != AuthMethod::MutualChannel
        {
            return Err(p::Error(
                "external Agent input requires a mutually authenticated channel".into(),
            ));
        }
        let provenance = gateway_provenance(&event.participant, &auth.principal);
        let source = if event.participant == ParticipantProfile::Owner {
            p::Source::UserTurn
        } else {
            p::Source::Communication
        };
        let session = event
            .session_ref
            .as_ref()
            .map(|session| p::SessionRef(session.0.clone()))
            .unwrap_or_else(|| {
                p::SessionRef(format!(
                    "gateway:{}:{}",
                    event.participant.participant_id().0,
                    self.sequence.fetch_add(1, Ordering::SeqCst)
                ))
            });
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source,
            session,
            agent_profile: self.agent_profile.clone(),
            input: p::RunInput(event.content.clone()),
            budget: None,
            idempotency_key: Some(p::IdempotencyKey(format!(
                "gateway:{}:{}",
                event.channel_adapter.0, event.message_id
            ))),
        };
        self.harness.submit_ingress(
            request,
            vec![self
                .ingress_authority
                .stamp(event.event_payload(), provenance)],
        )
    }

    pub fn submit_run(&self, request: p::RunRequest, auth: AuthContext) -> p::Result<p::RunId> {
        self.validate_run_submission(&request, auth)?;
        self.harness.submit_ingress(request, Vec::new())
    }

    pub(super) fn validate_run_submission(
        &self,
        request: &p::RunRequest,
        auth: AuthContext,
    ) -> p::Result<()> {
        auth.validate(now_ms())?;
        match (&request.source, &auth.principal) {
            (p::Source::UserTurn, AuthPrincipal::Owner(owner)) if owner == &self.owner => {}
            (p::Source::Communication, AuthPrincipal::External(_))
            | (p::Source::Communication, AuthPrincipal::AgentRepresentingOwner(_)) => {}
            _ => {
                return Err(p::Error(
                    "authenticated principal cannot submit this run source".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn wait(&self, run: p::RunId) -> p::Result<p::RunResult> {
        self.harness.wait(run)
    }

    pub fn answer(&self, run: p::RunId) -> p::Result<Option<String>> {
        self.harness.result_text(run)
    }

    /// Owner-authenticated read of a run's final model text for remote
    /// clients. The text is process state, not an event fact, so a run from
    /// an earlier daemon process reports `None`.
    pub fn answer_for_owner(&self, run: p::RunId, auth: AuthContext) -> p::Result<Option<String>> {
        auth.validate(now_ms())?;
        match auth.principal {
            AuthPrincipal::Owner(owner) if owner == self.owner => self.harness.result_text(run),
            _ => Err(p::Error(
                "run answers require the authenticated owner".into(),
            )),
        }
    }

    pub fn harness(&self) -> &Arc<H> {
        &self.harness
    }

    pub fn open_external_session(
        &self,
        fabric: &InMemoryCommunicationFabric,
        grant: ExternalCommunicationGrant,
        auth: AuthContext,
    ) -> p::Result<SessionId> {
        auth.validate(now_ms())?;
        if !matches!(&auth.principal, AuthPrincipal::Owner(owner) if owner == &self.owner) {
            return Err(p::Error(
                "only the verified owner can create an external communication grant".into(),
            ));
        }
        let session = fabric.open_session(grant)?;
        self.persist_fabric_events(fabric, &session)?;
        Ok(session)
    }

    pub fn apply_external_disclosure(
        &self,
        fabric: &InMemoryCommunicationFabric,
        request: DisclosureRequest,
    ) -> p::Result<DisclosureOutcome> {
        let session = request.session.clone();
        let outcome = fabric.apply_disclosure(request);
        self.persist_fabric_events(fabric, &session)?;
        Ok(outcome)
    }

    pub fn terminate_external_session(
        &self,
        fabric: &InMemoryCommunicationFabric,
        session: SessionId,
        reason: p::TerminationReason,
    ) -> p::Result<()> {
        fabric.terminate(session.clone(), reason)?;
        self.persist_fabric_events(fabric, &session)
    }

    pub fn submit_device_observation(
        &self,
        observation: AuthorizedDeviceObservation,
        auth: AuthContext,
    ) -> p::Result<p::RunId> {
        self.submit_device_observation_at(observation, auth, now_ms())
    }

    pub(super) fn submit_device_observation_at(
        &self,
        observation: AuthorizedDeviceObservation,
        auth: AuthContext,
        now: p::Timestamp,
    ) -> p::Result<p::RunId> {
        auth.validate(now)?;
        if !matches!(&auth.principal, AuthPrincipal::Owner(owner) if owner == &self.owner) {
            return Err(p::Error(
                "device observation is not owner-authorized".into(),
            ));
        }
        let (event, observation) = observation.into_gateway_parts(now)?;
        event.validate()?;
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::Communication,
            session: event
                .session_ref
                .as_ref()
                .map(|session| p::SessionRef(session.0.clone()))
                .unwrap_or_else(|| {
                    p::SessionRef(format!("device-observation:{}", event.message_id))
                }),
            agent_profile: self.agent_profile.clone(),
            input: p::RunInput(event.content.clone()),
            budget: None,
            idempotency_key: Some(p::IdempotencyKey(format!(
                "device:{}:{}",
                event.channel_adapter.0, event.message_id
            ))),
        };
        let provenance = p::Provenance {
            source: p::Source::Communication,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::Untrusted,
            caused_by: None,
        };
        self.harness.submit_ingress(
            request,
            vec![
                self.ingress_authority
                    .stamp(event.event_payload(), provenance.clone()),
                self.ingress_authority.stamp(observation, provenance),
            ],
        )
    }

    pub fn revoke_device(
        &self,
        adapter: &LocalDeviceChannelAdapter,
        lineage: p::LineageRef,
        auth: AuthContext,
    ) -> p::Result<()> {
        auth.validate(now_ms())?;
        if !matches!(&auth.principal, AuthPrincipal::Owner(owner) if owner == &self.owner) {
            return Err(p::Error(
                "only the verified owner can revoke a device".into(),
            ));
        }
        let event = adapter.revoke_with_event(lineage)?;
        self.harness.append_ingress_events(
            p::RunId(format!("device-revocation:{}", adapter.id.0)),
            vec![self.ingress_authority.stamp(
                event,
                p::Provenance {
                    source: p::Source::Communication,
                    actor: p::Actor::Owner,
                    trust_tier: p::TrustTier::OwnerInput,
                    caused_by: None,
                },
            )],
        )?;
        Ok(())
    }

    pub(super) fn persist_fabric_events(
        &self,
        fabric: &InMemoryCommunicationFabric,
        session: &SessionId,
    ) -> p::Result<()> {
        let events = fabric
            .take_events()
            .into_iter()
            .map(|payload| {
                let provenance =
                    if matches!(payload, p::EventPayload::ExternalCommunicationGranted(_)) {
                        p::Provenance {
                            source: p::Source::Communication,
                            actor: p::Actor::Owner,
                            trust_tier: p::TrustTier::OwnerInput,
                            caused_by: None,
                        }
                    } else {
                        p::Provenance {
                            source: p::Source::Communication,
                            actor: p::Actor::System,
                            trust_tier: p::TrustTier::VerifiedProcess,
                            caused_by: None,
                        }
                    };
                self.ingress_authority.stamp(payload, provenance)
            })
            .collect();
        self.harness
            .append_ingress_events(p::RunId(format!("communication:{}", session.0)), events)?;
        Ok(())
    }
}

impl<H> LocalGateway<H>
where
    H: HarnessActionIngress + Send + Sync,
{
    pub fn deliver_external_message(
        &self,
        fabric: &InMemoryCommunicationFabric,
        adapter: &LoopbackChannelAdapter,
        message: OutboundMessage,
        auth: AuthContext,
    ) -> p::Result<p::RunId> {
        auth.validate(now_ms())?;
        if !matches!(&auth.principal, AuthPrincipal::Owner(owner) if owner == &self.owner) {
            return Err(p::Error(
                "only the verified owner can submit an external delivery proposal".into(),
            ));
        }
        let session = message.session.clone();
        let prepared = match adapter.prepare_delivery(fabric, message) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.persist_fabric_events(fabric, &session)?;
                return Err(error);
            }
        };
        let prelude = fabric
            .take_events()
            .into_iter()
            .map(|payload| {
                self.ingress_authority.stamp(
                    payload,
                    p::Provenance {
                        source: p::Source::Communication,
                        actor: p::Actor::System,
                        trust_tier: p::TrustTier::VerifiedProcess,
                        caused_by: None,
                    },
                )
            })
            .collect::<Vec<_>>();
        if !prelude
            .iter()
            .any(|event| matches!(event.payload(), p::EventPayload::DisclosurePolicyApplied(_)))
        {
            return Err(p::Error(
                "external delivery has no persisted disclosure decision".into(),
            ));
        }
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::Communication,
            session: p::SessionRef(prepared.session.0.clone()),
            agent_profile: self.agent_profile.clone(),
            input: p::RunInput(format!(
                "deliver approved external message {}",
                prepared.message_id
            )),
            budget: Some(prepared.envelope.budget.clone()),
            idempotency_key: Some(p::IdempotencyKey(format!(
                "external-delivery:{}:{}",
                adapter.id.0, prepared.message_id
            ))),
        };
        self.harness
            .submit_action(request, prepared.intent, prepared.envelope, prelude)
    }
}
