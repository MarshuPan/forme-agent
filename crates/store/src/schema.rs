use core::{fmt, str::FromStr};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

macro_rules! define_payload_types {
    ($($kind:ident),+ $(,)?) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            Serialize,
            Deserialize,
        )]
        pub enum PayloadType {
            $($kind),+
        }

        impl PayloadType {
            pub const ALL: [Self; p::EventKind::ALL.len()] = [$(Self::$kind),+];

            pub const fn from_event_kind(kind: p::EventKind) -> Self {
                match kind {
                    $(p::EventKind::$kind => Self::$kind),+
                }
            }

            pub const fn event_kind(self) -> p::EventKind {
                match self {
                    $(Self::$kind => p::EventKind::$kind),+
                }
            }

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$kind => stringify!($kind)),+
                }
            }
        }

        impl FromStr for PayloadType {
            type Err = p::Error;

            fn from_str(value: &str) -> p::Result<Self> {
                match value {
                    $(stringify!($kind) => Ok(Self::$kind)),+,
                    _ => Err(p::Error(format!("unknown payload type: {value}"))),
                }
            }
        }
    };
}

define_payload_types! {
    RunAccepted,
    SessionBound,
    RunComplete,
    RunAborted,
    RunFailed,
    RunLimited,
    RunWaiting,
    RunResumed,
    TurnStarted,
    TurnComplete,
    ContextBuildStarted,
    ContextBuildFinished,
    CompactionStarted,
    CompactionFinished,
    ModelCallStarted,
    ModelCallDelta,
    ModelCallFinished,
    OutputClassified,
    ToolCallProposed,
    ToolPolicyEvaluated,
    ApprovalRequested,
    ApprovalResolved,
    HandoffRequested,
    HandoffResolved,
    ActionPlanned,
    ActionStarted,
    ActionOutputDelta,
    ActionCompleted,
    ActionFailed,
    ActionDenied,
    ActionCancelled,
    ActionOutcomeUnknown,
    VerificationStarted,
    VerificationFinished,
    FailureEvidenceRecorded,
    FailureDigestUpdated,
    CandidateCreated,
    CandidateConflictDetected,
    CandidatePromoted,
    CandidateRejected,
    CandidateDowngraded,
    CandidateDecayed,
    RetractionEvent,
    RevocationEvent,
    ReevaluationTaskCreated,
    ObservationRecorded,
    OpportunityDetected,
    ValueGateEvaluated,
    CompetenceGateEvaluated,
    ImpulseRaised,
    ReflectionProduced,
    ProactiveProposalEmitted,
    ProactiveProposalResolved,
    ProspectiveIntentionCreated,
    ProspectiveIntentionResolved,
    GoalFramed,
    ResourcePlanned,
    DoneContractSet,
    AutonomyEnvelopeSet,
    DecisionTraceRecorded,
    OrchestrationRouteCreated,
    SubagentSpawned,
    SubagentResultReturned,
    CapabilityIndexed,
    ToolsetResolved,
    McpDiscovered,
    McpCallEvent,
    SkillMetadataExposed,
    SkillBodyLoaded,
    PluginContributionRegistered,
    PluginToggled,
    CapabilityEvidenceRecorded,
    CommunicationEventReceived,
    CommunicationSessionOpened,
    CommunicationSessionTerminated,
    ExternalCommunicationGranted,
    DisclosurePolicyApplied,
    CommunicationProposalEmitted,
    MemoryNodeAppended,
    MemoryEdgeAppended,
    MemoryMaintenanceApplied,
    UserAttributeCandidateCreated,
    ImportedHistoricalEvidenceRecorded,
    CognitiveMapUpdateProposed,
    ConfigDoctorReport,
    ComplianceCheckResult,
    EvolutionEvaluationRecorded,
    StrategyActivated,
    StrategyRolledBack,
    FederatedPeerRegistered,
    FederatedPeerRevoked,
    RemoteExecutionLeaseChanged,
    ReplicationCheckpointAdvanced,
    CapabilityPublisherChanged,
    CapabilityPackageAdmitted,
    CapabilityPackageStateChanged,
    CapabilityPackageDistributionRecorded,
    WorkspaceCharterChanged,
    DataLifecycleApplied,
}

impl From<p::EventKind> for PayloadType {
    fn from(kind: p::EventKind) -> Self {
        Self::from_event_kind(kind)
    }
}

impl From<PayloadType> for p::EventKind {
    fn from(payload_type: PayloadType) -> Self {
        payload_type.event_kind()
    }
}

impl fmt::Display for PayloadType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaSnapshot {
    pub schema: BTreeMap<PayloadType, p::SchemaVersion>,
    pub policy_version: p::Version,
    pub loop_version: p::Version,
    pub model_profile: p::ModelProfileRef,
    pub tool_schema: p::Version,
}

type Transform = dyn Fn(Vec<u8>) -> p::Result<Vec<u8>> + Send + Sync + 'static;

#[derive(Clone)]
pub struct Upcaster {
    migration_note: String,
    implementation_identity: String,
    transform: Arc<Transform>,
}

impl Upcaster {
    pub fn new(
        migration_note: impl Into<String>,
        implementation_identity: impl Into<String>,
        transform: impl Fn(Vec<u8>) -> p::Result<Vec<u8>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            migration_note: migration_note.into(),
            implementation_identity: implementation_identity.into(),
            transform: Arc::new(transform),
        }
    }

    pub fn migration_note(&self) -> &str {
        &self.migration_note
    }

    pub fn implementation_identity(&self) -> &str {
        &self.implementation_identity
    }
}

impl fmt::Debug for Upcaster {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Upcaster")
            .field("migration_note", &self.migration_note)
            .field("implementation_identity", &self.implementation_identity)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct UpcasterKey {
    payload_type: PayloadType,
    from: p::SchemaVersion,
}

#[derive(Clone)]
struct UpcasterEdge {
    to: p::SchemaVersion,
    migration_note: String,
    implementation_identity: String,
    transform: Arc<Transform>,
}

#[derive(Clone, Default)]
pub(crate) struct UpcasterGraph {
    edges: BTreeMap<UpcasterKey, UpcasterEdge>,
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpcasterGraphToken {
    pub(crate) generation: u64,
    identity: Vec<UpcasterEdgeIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpcasterEdgeIdentity {
    payload_type: PayloadType,
    from: p::SchemaVersion,
    to: p::SchemaVersion,
    migration_note: String,
    implementation_identity: String,
}

impl UpcasterGraph {
    pub(crate) fn token(&self) -> UpcasterGraphToken {
        UpcasterGraphToken {
            generation: self.generation,
            identity: self
                .edges
                .iter()
                .map(|(key, edge)| UpcasterEdgeIdentity {
                    payload_type: key.payload_type,
                    from: key.from,
                    to: edge.to,
                    migration_note: edge.migration_note.clone(),
                    implementation_identity: edge.implementation_identity.clone(),
                })
                .collect(),
        }
    }

    pub(crate) fn identity_bytes(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        push_identity_field(&mut encoded, b"forme-upcaster-graph-v1");
        for (key, edge) in &self.edges {
            push_identity_field(&mut encoded, key.payload_type.as_str().as_bytes());
            push_identity_field(&mut encoded, &key.from.0.to_le_bytes());
            push_identity_field(&mut encoded, &edge.to.0.to_le_bytes());
            push_identity_field(&mut encoded, edge.migration_note.as_bytes());
            push_identity_field(&mut encoded, edge.implementation_identity.as_bytes());
        }
        encoded
    }

    pub(crate) fn validate_registration(
        &self,
        payload_type: PayloadType,
        from: p::SchemaVersion,
        to: p::SchemaVersion,
        upcaster: &Upcaster,
    ) -> p::Result<()> {
        if upcaster.migration_note.trim().is_empty() {
            return Err(p::Error("migration note cannot be empty".into()));
        }
        if upcaster.implementation_identity.trim().is_empty() {
            return Err(p::Error(
                "migration implementation identity cannot be empty".into(),
            ));
        }
        if from == to {
            return Err(p::Error(format!(
                "upcaster cycle for {payload_type}: schema {} points to itself",
                from.0
            )));
        }
        if to < from {
            return Err(p::Error(format!(
                "upcaster downgrade for {payload_type} is forbidden: schema {} to {}",
                from.0, to.0
            )));
        }

        let key = UpcasterKey { payload_type, from };
        if let Some(existing) = self.edges.get(&key) {
            if existing.to == to
                && existing.migration_note == upcaster.migration_note
                && existing.implementation_identity == upcaster.implementation_identity
            {
                return Ok(());
            }
            return Err(p::Error(format!(
                "conflicting upcaster implementation edge for {payload_type} schema {}: already targets schema {} with implementation {}",
                from.0, existing.to.0, existing.implementation_identity
            )));
        }
        if self.path_reaches(payload_type, to, from)? {
            return Err(p::Error(format!(
                "upcaster cycle for {payload_type}: schema {} reaches schema {}",
                to.0, from.0
            )));
        }
        Ok(())
    }

    pub(crate) fn insert(
        &mut self,
        payload_type: PayloadType,
        from: p::SchemaVersion,
        to: p::SchemaVersion,
        upcaster: Upcaster,
    ) {
        let key = UpcasterKey { payload_type, from };
        if self.edges.contains_key(&key) {
            return;
        }
        self.edges.insert(
            key,
            UpcasterEdge {
                to,
                migration_note: upcaster.migration_note,
                implementation_identity: upcaster.implementation_identity,
                transform: upcaster.transform,
            },
        );
        self.generation = self.generation.saturating_add(1);
    }

    pub(crate) fn plan(
        &self,
        payload_type: PayloadType,
        from: p::SchemaVersion,
        target: p::SchemaVersion,
    ) -> p::Result<Vec<UpcastStep>> {
        if from > target {
            return Err(p::Error(format!(
                "schema downgrade for {payload_type} is unsupported: stored schema {} to target schema {}",
                from.0, target.0
            )));
        }

        let mut current = from;
        let mut visited = BTreeSet::new();
        let mut steps = Vec::new();
        while current < target {
            if !visited.insert(current) {
                return Err(p::Error(format!(
                    "upcaster cycle detected for {payload_type} at schema {}",
                    current.0
                )));
            }
            let edge = self
                .edges
                .get(&UpcasterKey {
                    payload_type,
                    from: current,
                })
                .ok_or_else(|| {
                    p::Error(format!(
                        "missing upcaster for {payload_type} schema {} while targeting schema {}",
                        current.0, target.0
                    ))
                })?;
            if edge.to <= current {
                return Err(p::Error(format!(
                    "upcaster cycle or downgrade for {payload_type}: schema {} to {}",
                    current.0, edge.to.0
                )));
            }
            if edge.to > target {
                return Err(p::Error(format!(
                    "missing upcaster chain for {payload_type}: schema {} edge targets {} beyond requested schema {}",
                    current.0, edge.to.0, target.0
                )));
            }
            steps.push(UpcastStep {
                payload_type,
                from: current,
                to: edge.to,
                transform: Arc::clone(&edge.transform),
            });
            current = edge.to;
        }
        Ok(steps)
    }

    fn path_reaches(
        &self,
        payload_type: PayloadType,
        start: p::SchemaVersion,
        target: p::SchemaVersion,
    ) -> p::Result<bool> {
        let mut current = start;
        let mut visited = BTreeSet::new();
        loop {
            if current == target {
                return Ok(true);
            }
            if !visited.insert(current) {
                return Err(p::Error(format!(
                    "existing upcaster cycle detected for {payload_type} at schema {}",
                    current.0
                )));
            }
            let Some(edge) = self.edges.get(&UpcasterKey {
                payload_type,
                from: current,
            }) else {
                return Ok(false);
            };
            current = edge.to;
        }
    }
}

fn push_identity_field(encoded: &mut Vec<u8>, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("upcaster identity field length fits in u64");
    encoded.extend_from_slice(&length.to_le_bytes());
    encoded.extend_from_slice(value);
}

pub(crate) struct UpcastStep {
    payload_type: PayloadType,
    from: p::SchemaVersion,
    to: p::SchemaVersion,
    transform: Arc<Transform>,
}

impl UpcastStep {
    pub(crate) fn apply(&self, bytes: Vec<u8>) -> p::Result<Vec<u8>> {
        (self.transform)(bytes).map_err(|error| {
            p::Error(format!(
                "upcaster transform failed for {} schema {} to {}: {error}",
                self.payload_type, self.from.0, self.to.0
            ))
        })
    }
}
