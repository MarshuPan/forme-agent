use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

const SCHEMA_VERSION: p::SchemaVersion = p::M4_SCHEMA_VERSION;
const PEER_KIND: &str = "m4-federated-peer-manifest";
const RECEIPT_KIND: &str = "m4-remote-execution-receipt";
const REPLICATION_KIND: &str = "m4-replication-manifest";
const TRACE_KIND: &str = "m4-federated-trace-manifest";
const REPORT_KIND: &str = "m4-federation-golden-report";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteGroundTruth {
    pub schema_version: p::SchemaVersion,
    pub outcome: p::RemoteReceiptOutcome,
    pub result_digest: Option<p::SchemaDigest>,
    pub evidence: Vec<p::EvidenceRef>,
    pub verification: Vec<p::EvidenceRef>,
}

impl RemoteGroundTruth {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != SCHEMA_VERSION
            || self.evidence.is_empty()
            || self.verification.is_empty()
            || self
                .evidence
                .iter()
                .chain(&self.verification)
                .any(|reference| reference.0.trim().is_empty())
        {
            return Err(p::Error(
                "remote ground truth is incomplete or unversioned".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct RemoteAuthorityVerifier;

impl RemoteAuthorityVerifier {
    pub fn verify(
        &self,
        plan: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
        driver: &p::RemoteDriverReceipt,
        ground_truth: RemoteGroundTruth,
    ) -> p::Result<p::RemoteExecutionReceipt> {
        plan.validate()?;
        lease.validate()?;
        driver.validate()?;
        ground_truth.validate()?;
        if lease.state != p::RemoteLeaseState::Acquired
            || lease.placement != plan.reference()?
            || lease.executor != plan.executor
            || lease.peer_grant != plan.peer_grant
            || lease.grant_version != plan.grant_version
            || lease.authority_epoch != plan.authority_epoch
            || driver.lease != lease.lease
            || driver.dispatch != lease.dispatch
            || driver.intent != lease.intent
            || driver.plan_digest != lease.plan_digest
            || driver.operation_digest != plan.operation.digest
            || driver.executor != lease.executor
            || driver.authority_epoch != lease.authority_epoch
            || driver.fence != lease.fence
        {
            return Err(p::Error(
                "remote receipt is not bound to the authority plan and lease".into(),
            ));
        }
        if driver.outcome != ground_truth.outcome
            || driver.result_digest != ground_truth.result_digest
            || matches!(driver.outcome, p::RemoteReceiptOutcome::Unknown)
        {
            return Err(p::Error(
                "remote self-report does not match independent ground truth".into(),
            ));
        }
        let driver_receipt_digest = p::canonical_digest(driver)?;
        let receipt_digest = p::canonical_digest(&(
            &driver_receipt_digest,
            &lease.lease,
            &lease.plan_digest,
            &plan.operation.digest,
            &ground_truth,
        ))?;
        let receipt = p::RemoteExecutionReceipt {
            schema_version: SCHEMA_VERSION,
            receipt: p::RemoteExecutionReceiptRef(format!(
                "authority-receipt:{}",
                receipt_digest.0
            )),
            driver_receipt: driver.receipt.clone(),
            driver_receipt_digest,
            lease: lease.lease.clone(),
            intent: lease.intent.clone(),
            plan_digest: lease.plan_digest.clone(),
            operation_digest: plan.operation.digest.clone(),
            executor: lease.executor.clone(),
            rollback_boundary: plan.operation.rollback_boundary.clone(),
            outcome: driver.outcome,
            verification: ground_truth.verification,
            ground_truth: ground_truth.evidence,
            authority_verified: p::RequiredTrue,
        };
        receipt.validate()?;
        Ok(receipt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationArtifactBundle {
    pub peer: p::FederatedPeerManifest,
    pub receipt: p::RemoteExecutionReceipt,
    pub replication: p::ReplicationManifest,
    pub trace: p::FederatedTraceManifest,
    pub report: p::FederationGoldenReport,
}

impl FederationArtifactBundle {
    pub fn validate(&self) -> p::Result<()> {
        self.peer.validate()?;
        self.receipt.validate()?;
        self.replication.validate()?;
        self.trace.validate()?;
        self.report.validate()?;
        if !self.peer.roles.contains(&p::FederatedPeerRole::Executor)
            || self.peer.peer != self.receipt.executor
            || self.report.replica_cursor.as_ref() != Some(&self.replication.to)
            || self.report.trace_digest != p::canonical_digest(&self.trace)?
            || self.trace.actions.is_empty()
            || self.trace.verifications.is_empty()
            || self.trace.replication.is_empty()
            || self.trace.revocations.is_empty()
            || !is_subsequence(
                &self.report.event_kinds,
                &[
                    p::EventKind::FederatedPeerRegistered,
                    p::EventKind::RunAccepted,
                    p::EventKind::SessionBound,
                    p::EventKind::ToolCallProposed,
                    p::EventKind::ToolPolicyEvaluated,
                    p::EventKind::ActionPlanned,
                    p::EventKind::ApprovalRequested,
                    p::EventKind::RunWaiting,
                    p::EventKind::ApprovalResolved,
                    p::EventKind::RunResumed,
                    p::EventKind::CompetenceGateEvaluated,
                    p::EventKind::RemoteExecutionLeaseChanged,
                    p::EventKind::ActionStarted,
                    p::EventKind::ActionCompleted,
                    p::EventKind::RemoteExecutionLeaseChanged,
                    p::EventKind::VerificationStarted,
                    p::EventKind::VerificationFinished,
                    p::EventKind::ReplicationCheckpointAdvanced,
                    p::EventKind::FederatedPeerRevoked,
                ],
            )
        {
            return Err(p::Error(
                "federation artifact lineage is incomplete or inconsistent".into(),
            ));
        }
        if self.receipt.outcome != p::RemoteReceiptOutcome::Completed {
            return Err(p::Error(
                "federation golden requires an authority-verified completion".into(),
            ));
        }
        let value = serde_json::to_value(self)
            .map_err(|_| p::Error("federation artifact could not be inspected".into()))?;
        scan_portable_value(&value)
    }
}

impl Serialize for FederationArtifactBundle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        (
            &self.peer,
            &self.receipt,
            &self.replication,
            &self.trace,
            &self.report,
        )
            .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableFederationArtifact<T> {
    pub schema_version: p::SchemaVersion,
    pub artifact_type: String,
    pub digest: p::SchemaDigest,
    pub body: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationArtifactReceipt {
    pub schema_version: p::SchemaVersion,
    pub root: PathBuf,
    pub digest: p::SchemaDigest,
    pub content_ref: p::ContentRef,
}

pub struct FederationArtifactStore {
    root: PathBuf,
}

impl FederationArtifactStore {
    pub fn new(root: impl AsRef<Path>) -> p::Result<Self> {
        fs::create_dir_all(root.as_ref())
            .map_err(|_| p::Error("federation artifact root could not be created".into()))?;
        let root = fs::canonicalize(root.as_ref())
            .map_err(|_| p::Error("federation artifact root could not be resolved".into()))?;
        if !root.is_dir() {
            return Err(p::Error(
                "federation artifact root is not a directory".into(),
            ));
        }
        Ok(Self { root })
    }

    pub fn write(&self, bundle: &FederationArtifactBundle) -> p::Result<FederationArtifactReceipt> {
        bundle.validate()?;
        let existing = directory_entries(&self.root)?;
        if !existing.is_empty() {
            return Err(p::Error(
                "federation artifact root must be empty before generation".into(),
            ));
        }
        write_artifact(&self.root, "peer", PEER_KIND, &bundle.peer)?;
        write_artifact(&self.root, "receipt", RECEIPT_KIND, &bundle.receipt)?;
        write_artifact(
            &self.root,
            "replication",
            REPLICATION_KIND,
            &bundle.replication,
        )?;
        write_artifact(&self.root, "trace", TRACE_KIND, &bundle.trace)?;
        write_artifact(&self.root, "report", REPORT_KIND, &bundle.report)?;
        self.verify_complete_set()
    }

    pub fn verify_complete_set(&self) -> p::Result<FederationArtifactReceipt> {
        let entries = directory_entries(&self.root)?;
        if entries.len() != 5 || entries.iter().any(|entry| !entry.is_file()) {
            return Err(p::Error(
                "federation artifact root is not a closed five-file set".into(),
            ));
        }
        let mut values = BTreeMap::<String, (PathBuf, serde_json::Value)>::new();
        let mut digests = Vec::<p::SchemaDigest>::new();
        for path in entries {
            if path.parent() != Some(self.root.as_path())
                || path.extension().and_then(|value| value.to_str()) != Some("json")
            {
                return Err(p::Error(
                    "federation artifact path or type is invalid".into(),
                ));
            }
            let bytes = fs::read(&path)
                .map_err(|_| p::Error("federation artifact could not be read".into()))?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|_| p::Error("federation artifact is malformed".into()))?;
            scan_portable_value(&value)?;
            let kind = value
                .get("artifact_type")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| p::Error("federation artifact type is absent".into()))?
                .to_owned();
            validate_artifact_body_schema(&kind, value.get("body"))?;
            if values.insert(kind, (path, value)).is_some() {
                return Err(p::Error("federation artifact type is duplicated".into()));
            }
        }
        let peer = decode_artifact::<p::FederatedPeerManifest>(
            values.remove(PEER_KIND),
            PEER_KIND,
            "peer",
            &mut digests,
        )?;
        let receipt = decode_artifact::<p::RemoteExecutionReceipt>(
            values.remove(RECEIPT_KIND),
            RECEIPT_KIND,
            "receipt",
            &mut digests,
        )?;
        let replication = decode_artifact::<p::ReplicationManifest>(
            values.remove(REPLICATION_KIND),
            REPLICATION_KIND,
            "replication",
            &mut digests,
        )?;
        let trace = decode_artifact::<p::FederatedTraceManifest>(
            values.remove(TRACE_KIND),
            TRACE_KIND,
            "trace",
            &mut digests,
        )?;
        let report = decode_artifact::<p::FederationGoldenReport>(
            values.remove(REPORT_KIND),
            REPORT_KIND,
            "report",
            &mut digests,
        )?;
        if !values.is_empty() {
            return Err(p::Error(
                "federation artifact set contains an unknown type".into(),
            ));
        }
        FederationArtifactBundle {
            peer,
            receipt,
            replication,
            trace,
            report,
        }
        .validate()?;
        digests.sort_by(|left, right| left.0.cmp(&right.0));
        let digest = p::canonical_digest(&digests)?;
        Ok(FederationArtifactReceipt {
            schema_version: SCHEMA_VERSION,
            root: self.root.clone(),
            content_ref: p::ContentRef(format!("artifact:{}", digest.0)),
            digest,
        })
    }
}

fn write_artifact<T: Serialize>(root: &Path, prefix: &str, kind: &str, body: &T) -> p::Result<()> {
    let body_digest = p::canonical_digest(body)?;
    let artifact = PortableFederationArtifact {
        schema_version: SCHEMA_VERSION,
        artifact_type: kind.to_owned(),
        digest: body_digest.clone(),
        body,
    };
    let suffix = digest_suffix(&body_digest)?;
    let path = root.join(format!("{prefix}-{suffix}.json"));
    if path.parent() != Some(root) {
        return Err(p::Error("federation artifact escaped its root".into()));
    }
    let bytes = serde_json::to_vec_pretty(&artifact)
        .map_err(|_| p::Error("federation artifact could not be encoded".into()))?;
    fs::write(path, bytes)
        .map_err(|_| p::Error("federation artifact could not be persisted".into()))
}

fn decode_artifact<T: DeserializeOwned + Serialize>(
    entry: Option<(PathBuf, serde_json::Value)>,
    expected_kind: &str,
    prefix: &str,
    digests: &mut Vec<p::SchemaDigest>,
) -> p::Result<T> {
    let (path, value) =
        entry.ok_or_else(|| p::Error("federation artifact type is missing".into()))?;
    let artifact: PortableFederationArtifact<T> = serde_json::from_value(value)
        .map_err(|_| p::Error("federation artifact schema is invalid".into()))?;
    let digest = p::canonical_digest(&artifact.body)?;
    if artifact.schema_version != SCHEMA_VERSION
        || artifact.artifact_type != expected_kind
        || artifact.digest != digest
    {
        return Err(p::Error(
            "federation artifact type, schema, or digest is invalid".into(),
        ));
    }
    let expected_name = format!("{prefix}-{}.json", digest_suffix(&digest)?);
    if path.file_name().and_then(|value| value.to_str()) != Some(expected_name.as_str()) {
        return Err(p::Error(
            "federation artifact filename is not bound to its body digest".into(),
        ));
    }
    digests.push(digest);
    Ok(artifact.body)
}

fn directory_entries(root: &Path) -> p::Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(root)
        .map_err(|_| p::Error("federation artifact root could not be listed".into()))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|_| p::Error("federation artifact entry could not be inspected".into()))?
        .into_iter()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries)
}

fn digest_suffix(digest: &p::SchemaDigest) -> p::Result<&str> {
    digest
        .0
        .strip_prefix("sha256:")
        .filter(|suffix| suffix.len() == 64 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| p::Error("federation artifact digest is invalid".into()))
}

fn is_subsequence(actual: &[p::EventKind], expected: &[p::EventKind]) -> bool {
    let mut cursor = 0;
    for expected_kind in expected {
        let Some(offset) = actual[cursor..]
            .iter()
            .position(|kind| kind == expected_kind)
        else {
            return false;
        };
        cursor += offset + 1;
    }
    true
}

fn scan_portable_value(value: &serde_json::Value) -> p::Result<()> {
    match value {
        serde_json::Value::String(value) => scan_string(value),
        serde_json::Value::Array(values) => {
            for value in values {
                scan_portable_value(value)?;
            }
            Ok(())
        }
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                scan_string(key)?;
                scan_portable_value(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn scan_string(value: &str) -> p::Result<()> {
    let lower = value.to_ascii_lowercase();
    if [
        "secretref",
        "secret_ref",
        "credential",
        "authorization:",
        "bearer ",
        "private key",
        "private-key",
        "session key",
        "http://",
        "https://",
        "ws://",
        "wss://",
        "ssh://",
        "127.0.0.1:",
        "localhost:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || is_absolute_path(value)
    {
        return Err(p::Error(
            "federation artifact contains restricted or host-private material".into(),
        ));
    }
    Ok(())
}

fn validate_artifact_body_schema(kind: &str, body: Option<&serde_json::Value>) -> p::Result<()> {
    let body = body.ok_or_else(|| p::Error("federation artifact body is absent".into()))?;
    match kind {
        PEER_KIND => require_exact_keys(
            body,
            &[
                "schema_version",
                "peer",
                "roles",
                "scopes",
                "transport_identity",
                "authority_epoch",
                "grant_version",
                "expires_at",
                "grant_ref",
            ],
        ),
        RECEIPT_KIND => require_exact_keys(
            body,
            &[
                "schema_version",
                "receipt",
                "driver_receipt",
                "driver_receipt_digest",
                "lease",
                "intent",
                "plan_digest",
                "operation_digest",
                "executor",
                "rollback_boundary",
                "outcome",
                "verification",
                "ground_truth",
                "authority_verified",
            ],
        ),
        REPLICATION_KIND => {
            let object = require_exact_keys(
                body,
                &[
                    "schema_version",
                    "peer",
                    "aggregate",
                    "from",
                    "to",
                    "batch",
                    "batch_digest",
                    "event_digests",
                    "redaction",
                    "authority_epoch",
                ],
            )?;
            require_exact_keys(
                object
                    .get("from")
                    .ok_or_else(|| p::Error("replication from cursor is absent".into()))?,
                &[
                    "schema_version",
                    "peer",
                    "aggregate",
                    "stream_seq",
                    "authority_epoch",
                ],
            )?;
            require_exact_keys(
                object
                    .get("to")
                    .ok_or_else(|| p::Error("replication to cursor is absent".into()))?,
                &[
                    "schema_version",
                    "peer",
                    "aggregate",
                    "stream_seq",
                    "authority_epoch",
                ],
            )?;
            Ok(object)
        }
        TRACE_KIND => require_exact_keys(
            body,
            &[
                "schema_version",
                "run",
                "owner_commands",
                "approvals",
                "leases",
                "actions",
                "verifications",
                "replication",
                "recovery",
                "revocations",
            ],
        ),
        REPORT_KIND => {
            let object = require_exact_keys(
                body,
                &[
                    "schema_version",
                    "scenario",
                    "event_kinds",
                    "authority_driver_calls",
                    "mutation_ordinal",
                    "replica_cursor",
                    "secret_scan_matches",
                    "negative_assertions",
                    "trace_digest",
                ],
            )?;
            if let Some(cursor) = object
                .get("replica_cursor")
                .filter(|value| !value.is_null())
            {
                require_exact_keys(
                    cursor,
                    &[
                        "schema_version",
                        "peer",
                        "aggregate",
                        "stream_seq",
                        "authority_epoch",
                    ],
                )?;
            }
            Ok(object)
        }
        _ => Err(p::Error("federation artifact type is unknown".into())),
    }
    .map(|_| ())
}

fn require_exact_keys<'a>(
    value: &'a serde_json::Value,
    expected: &[&str],
) -> p::Result<&'a serde_json::Map<String, serde_json::Value>> {
    let object = value
        .as_object()
        .ok_or_else(|| p::Error("federation artifact body is not an object".into()))?;
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(p::Error(
            "federation artifact body is not a closed schema".into(),
        ));
    }
    Ok(object)
}

fn is_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_verifier_rejects_worker_self_report_without_ground_truth() {
        let verifier = RemoteAuthorityVerifier;
        let result = verifier.verify(
            &fixture_plan(),
            &fixture_lease(),
            &fixture_driver_receipt(),
            RemoteGroundTruth {
                schema_version: SCHEMA_VERSION,
                outcome: p::RemoteReceiptOutcome::Completed,
                result_digest: Some(p::SchemaDigest("sha256:result".into())),
                evidence: Vec::new(),
                verification: vec![p::EvidenceRef("verify:fixture".into())],
            },
        );
        assert!(result.is_err());
    }

    fn fixture_plan() -> p::RemotePlacementPlan {
        let mut operation = p::RemoteOperation {
            schema_version: SCHEMA_VERSION,
            backend: p::BackendKind::File,
            parameters: p::ActionParameters::File {
                operation: p::FileOperation::Write,
                path: "state.json".into(),
                content: Some(b"one".to_vec()),
            },
            capability: p::CapabilityRef("fixture.mutate".into()),
            scope: p::Scope("workspace:fixture".into()),
            action_type: p::ActionType::Write,
            expected_effect: p::ExpectedEffect::Outward,
            rollback_boundary: p::RollbackBoundary("fixture reset".into()),
            credential_slot: None,
            digest: p::SchemaDigest(String::new()),
        };
        operation.refresh_digest().unwrap();
        let mut plan = p::RemotePlacementPlan {
            schema_version: SCHEMA_VERSION,
            executor: p::FederatedPeerRef("peer:executor".into()),
            peer_grant: p::FederatedPeerGrantRef("grant:executor".into()),
            grant_version: p::PeerGrantVersion(1),
            authority_epoch: p::AuthorityEpoch(1),
            executor_profile: p::ExecutorProfileRef("profile:fixture".into()),
            operation,
            digest: p::SchemaDigest(String::new()),
        };
        plan.refresh_digest().unwrap();
        plan
    }

    fn fixture_lease() -> p::RemoteExecutionLease {
        let plan = fixture_plan();
        p::RemoteExecutionLease {
            schema_version: SCHEMA_VERSION,
            lease: p::RemoteExecutionLeaseRef("lease:fixture".into()),
            dispatch: p::RemoteDispatchId("dispatch:fixture".into()),
            intent: p::ActionId("intent:fixture".into()),
            plan_digest: p::PlanDigest("sha256:plan".into()),
            placement: plan.reference().unwrap(),
            executor: plan.executor,
            peer_grant: plan.peer_grant,
            grant_version: plan.grant_version,
            authority_epoch: plan.authority_epoch,
            fence: p::FenceToken(1),
            expires_at: i64::MAX - 1,
            state: p::RemoteLeaseState::Acquired,
        }
    }

    fn fixture_driver_receipt() -> p::RemoteDriverReceipt {
        let plan = fixture_plan();
        let lease = fixture_lease();
        p::RemoteDriverReceipt {
            schema_version: SCHEMA_VERSION,
            receipt: p::RemoteDriverReceiptRef("receipt:driver".into()),
            lease: lease.lease,
            dispatch: lease.dispatch,
            intent: lease.intent,
            plan_digest: lease.plan_digest,
            operation_digest: plan.operation.digest,
            executor: lease.executor,
            authority_epoch: lease.authority_epoch,
            fence: lease.fence,
            outcome: p::RemoteReceiptOutcome::Completed,
            result_digest: Some(p::SchemaDigest("sha256:result".into())),
            observations: vec![p::EvidenceRef("worker:observation".into())],
            observed_at: 1,
        }
    }
}
