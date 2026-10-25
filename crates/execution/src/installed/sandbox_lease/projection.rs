use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{
    SandboxAclMutation, SandboxAclOperation, SandboxAclReadback, SandboxFileIdentity,
    SandboxLeaseBinding, SandboxLeaseRecoveryView, SandboxProcessTreeTermination,
    SandboxRecoveryStage, SandboxWritebackReceipt,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum LeaseFact {
    LeaseOpened {
        lease_id: String,
        binding: SandboxLeaseBinding,
    },
    AclMutationIntent {
        mutation: SandboxAclMutation,
    },
    AclReadback {
        mutation: SandboxAclMutation,
        readback: SandboxAclReadback,
    },
    ProcessTreeTerminated {
        lease_id: String,
        proof: SandboxProcessTreeTermination,
    },
    AppContainerRemovalIntent {
        lease_id: String,
        appcontainer_identity: String,
    },
    AppContainerRemoved {
        lease_id: String,
        appcontainer_identity: String,
    },
    JobDirectoryRemovalIntent {
        lease_id: String,
        path: String,
        file_identity: SandboxFileIdentity,
    },
    JobDirectoryRemoved {
        lease_id: String,
        path: String,
        file_identity: SandboxFileIdentity,
    },
    CleanupReceipt {
        lease_id: String,
    },
    WorkspaceWritebackVerified {
        lease_id: String,
        receipt: SandboxWritebackReceipt,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LeaseEnvelope {
    pub(super) schema_version: p::SchemaVersion,
    pub(super) lease_seq: u64,
    pub(super) previous: p::SchemaDigest,
    pub(super) fact: LeaseFact,
    pub(super) digest: p::SchemaDigest,
}

impl LeaseEnvelope {
    pub(super) fn calculate(
        lease_seq: u64,
        previous: &p::SchemaDigest,
        fact: &LeaseFact,
    ) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&(p::SchemaVersion(1), lease_seq, previous, fact))
    }
}

#[derive(Debug, Clone)]
pub(super) struct MutationRecord {
    pub(super) mutation: SandboxAclMutation,
    pub(super) readback: Option<SandboxAclReadback>,
}

#[derive(Debug, Clone)]
pub(super) struct LeaseRecord {
    pub(super) binding: SandboxLeaseBinding,
    pub(super) mutations: BTreeMap<String, MutationRecord>,
    pub(super) process_tree_termination: Option<SandboxProcessTreeTermination>,
    pub(super) appcontainer_removal_intent: bool,
    pub(super) appcontainer_removed: bool,
    pub(super) job_directory_removal_intent: bool,
    pub(super) job_directory_removed: bool,
    pub(super) cleanup_complete: bool,
    pub(super) writeback_receipt: Option<SandboxWritebackReceipt>,
}

impl LeaseRecord {
    pub(super) fn view(&self, lease_id: &str) -> SandboxLeaseRecoveryView {
        SandboxLeaseRecoveryView {
            lease_id: lease_id.to_owned(),
            binding: self.binding.clone(),
            acl_mutations: self
                .mutations
                .values()
                .map(|record| super::SandboxAclMutationState {
                    mutation: record.mutation.clone(),
                    readback: record.readback.clone(),
                })
                .collect(),
            process_tree_termination: self.process_tree_termination.clone(),
            appcontainer_removal_intent: self.appcontainer_removal_intent,
            appcontainer_removed: self.appcontainer_removed,
            job_directory_removal_intent: self.job_directory_removal_intent,
            job_directory_removed: self.job_directory_removed,
            cleanup_complete: self.cleanup_complete,
            writeback_receipt: self.writeback_receipt.clone(),
        }
    }

    fn acl_cleanup_verified(&self) -> bool {
        self.mutations.values().all(|record| {
            if record.mutation.operation == SandboxAclOperation::Grant {
                self.mutations.values().any(|revoke| {
                    revoke.mutation.operation == SandboxAclOperation::Revoke
                        && same_acl_target(&record.mutation, &revoke.mutation)
                        && revoke.readback.is_some()
                })
            } else {
                record.readback.is_some()
            }
        })
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct Projection {
    pub(super) sequence: u64,
    pub(super) head: Option<p::SchemaDigest>,
    pub(super) leases: BTreeMap<String, LeaseRecord>,
    pub(super) fact_digests: BTreeSet<p::SchemaDigest>,
    jobs: BTreeMap<String, String>,
    appcontainer_identities: BTreeMap<String, String>,
}

impl Projection {
    pub(super) fn head(&self) -> p::Result<p::SchemaDigest> {
        self.head.clone().map_or_else(
            || p::canonical_digest(&"forme-sandbox-lease-journal-v1"),
            Ok,
        )
    }

    pub(super) fn fold(&mut self, envelope: &LeaseEnvelope) -> p::Result<()> {
        if envelope.schema_version != p::SchemaVersion(1)
            || envelope.lease_seq != self.sequence.checked_add(1).ok_or_else(invalid_journal)?
            || envelope.previous != self.head()?
            || envelope.digest
                != LeaseEnvelope::calculate(envelope.lease_seq, &envelope.previous, &envelope.fact)?
        {
            return Err(invalid_journal());
        }
        let fact_digest = p::canonical_digest(&envelope.fact)?;
        if self.fact_digests.contains(&fact_digest) {
            return Err(invalid_journal());
        }

        match &envelope.fact {
            LeaseFact::LeaseOpened { lease_id, binding } => {
                binding.validate()?;
                if calculate_lease_id(binding)? != *lease_id
                    || self.leases.contains_key(lease_id)
                    || self.jobs.contains_key(&binding.job_id)
                    || self
                        .appcontainer_identities
                        .contains_key(&binding.appcontainer_identity)
                    || self.leases.values().any(|record| {
                        !record.cleanup_complete
                            && (record.binding.job_directory == binding.job_directory
                                || record.binding.job_directory_identity
                                    == binding.job_directory_identity)
                    })
                {
                    return Err(invalid_journal());
                }
                self.jobs.insert(binding.job_id.clone(), lease_id.clone());
                self.appcontainer_identities
                    .insert(binding.appcontainer_identity.clone(), lease_id.clone());
                self.leases.insert(
                    lease_id.clone(),
                    LeaseRecord {
                        binding: binding.clone(),
                        mutations: BTreeMap::new(),
                        process_tree_termination: None,
                        appcontainer_removal_intent: false,
                        appcontainer_removed: false,
                        job_directory_removal_intent: false,
                        job_directory_removed: false,
                        cleanup_complete: false,
                        writeback_receipt: None,
                    },
                );
            }
            LeaseFact::AclMutationIntent { mutation } => {
                let record = self
                    .leases
                    .get_mut(&mutation.lease_id)
                    .ok_or_else(invalid_journal)?;
                validate_mutation_for(&record.binding, mutation)?;
                if record.cleanup_complete
                    || record.appcontainer_removal_intent
                    || record.job_directory_removal_intent
                    || record.mutations.contains_key(&mutation.operation_id)
                {
                    return Err(invalid_journal());
                }
                match mutation.operation {
                    SandboxAclOperation::Grant => {
                        if record.process_tree_termination.is_some()
                            || record.mutations.values().any(|previous| {
                                previous.mutation.operation == SandboxAclOperation::Grant
                                    && same_acl_target(&previous.mutation, mutation)
                            })
                        {
                            return Err(invalid_journal());
                        }
                    }
                    SandboxAclOperation::Revoke => {
                        if record.process_tree_termination.is_none()
                            || !record.mutations.values().any(|previous| {
                                previous.mutation.operation == SandboxAclOperation::Grant
                                    && same_acl_target(&previous.mutation, mutation)
                            })
                            || record.mutations.values().any(|previous| {
                                previous.mutation.operation == SandboxAclOperation::Revoke
                                    && same_acl_target(&previous.mutation, mutation)
                            })
                        {
                            return Err(invalid_journal());
                        }
                    }
                }
                record.mutations.insert(
                    mutation.operation_id.clone(),
                    MutationRecord {
                        mutation: mutation.clone(),
                        readback: None,
                    },
                );
            }
            LeaseFact::AclReadback { mutation, readback } => {
                let record = self
                    .leases
                    .get_mut(&mutation.lease_id)
                    .ok_or_else(invalid_journal)?;
                validate_mutation_for(&record.binding, mutation)?;
                readback.validate_for(mutation)?;
                let saved = record
                    .mutations
                    .get_mut(&mutation.operation_id)
                    .ok_or_else(invalid_journal)?;
                if saved.mutation != *mutation || saved.readback.is_some() {
                    return Err(invalid_journal());
                }
                saved.readback = Some(readback.clone());
            }
            LeaseFact::ProcessTreeTerminated { lease_id, proof } => {
                let record = self.leases.get_mut(lease_id).ok_or_else(invalid_journal)?;
                proof.validate_for(lease_id, &record.binding)?;
                if record.process_tree_termination.is_some()
                    || record.appcontainer_removal_intent
                    || record.cleanup_complete
                {
                    return Err(invalid_journal());
                }
                record.process_tree_termination = Some(proof.clone());
            }
            LeaseFact::AppContainerRemovalIntent {
                lease_id,
                appcontainer_identity,
            } => {
                let record = self.leases.get_mut(lease_id).ok_or_else(invalid_journal)?;
                if appcontainer_identity != &record.binding.appcontainer_identity
                    || record.process_tree_termination.is_none()
                    || !record.acl_cleanup_verified()
                    || record.appcontainer_removal_intent
                    || record.cleanup_complete
                {
                    return Err(invalid_journal());
                }
                record.appcontainer_removal_intent = true;
            }
            LeaseFact::AppContainerRemoved {
                lease_id,
                appcontainer_identity,
            } => {
                let record = self.leases.get_mut(lease_id).ok_or_else(invalid_journal)?;
                if appcontainer_identity != &record.binding.appcontainer_identity
                    || !record.appcontainer_removal_intent
                    || record.appcontainer_removed
                    || record.cleanup_complete
                {
                    return Err(invalid_journal());
                }
                record.appcontainer_removed = true;
            }
            LeaseFact::JobDirectoryRemovalIntent {
                lease_id,
                path,
                file_identity,
            } => {
                let record = self.leases.get_mut(lease_id).ok_or_else(invalid_journal)?;
                if path != &record.binding.job_directory
                    || file_identity != &record.binding.job_directory_identity
                    || !record.appcontainer_removed
                    || record.job_directory_removal_intent
                    || record.cleanup_complete
                {
                    return Err(invalid_journal());
                }
                record.job_directory_removal_intent = true;
            }
            LeaseFact::JobDirectoryRemoved {
                lease_id,
                path,
                file_identity,
            } => {
                let record = self.leases.get_mut(lease_id).ok_or_else(invalid_journal)?;
                if path != &record.binding.job_directory
                    || file_identity != &record.binding.job_directory_identity
                    || !record.job_directory_removal_intent
                    || record.job_directory_removed
                    || record.cleanup_complete
                {
                    return Err(invalid_journal());
                }
                record.job_directory_removed = true;
            }
            LeaseFact::CleanupReceipt { lease_id } => {
                let record = self.leases.get_mut(lease_id).ok_or_else(invalid_journal)?;
                if !record.appcontainer_removed
                    || !record.job_directory_removed
                    || record.cleanup_complete
                {
                    return Err(invalid_journal());
                }
                record.cleanup_complete = true;
            }
            LeaseFact::WorkspaceWritebackVerified { lease_id, receipt } => {
                let record = self.leases.get_mut(lease_id).ok_or_else(invalid_journal)?;
                receipt.validate_for(&record.binding)?;
                if record.process_tree_termination.is_none()
                    || record.job_directory_removal_intent
                    || record.cleanup_complete
                    || record.writeback_receipt.is_some()
                {
                    return Err(invalid_journal());
                }
                record.writeback_receipt = Some(receipt.clone());
            }
        }

        self.sequence = envelope.lease_seq;
        self.head = Some(envelope.digest.clone());
        self.fact_digests.insert(fact_digest);
        Ok(())
    }
}

pub(super) fn validate_mutation_for(
    binding: &SandboxLeaseBinding,
    mutation: &SandboxAclMutation,
) -> p::Result<()> {
    let expected_stage = match mutation.operation {
        SandboxAclOperation::Grant => SandboxRecoveryStage::GrantAclPending,
        SandboxAclOperation::Revoke => SandboxRecoveryStage::RevokeAclPending,
    };
    if mutation.schema_version != p::SchemaVersion(1)
        || mutation.job_id != binding.job_id
        || mutation.plan_digest != binding.plan_digest
        || mutation.appcontainer_identity != binding.appcontainer_identity
        || mutation.recovery_stage != expected_stage
        || !Path::new(&mutation.target_path).is_absolute()
        || !is_lexically_normalized(Path::new(&mutation.target_path))
        || calculate_mutation_id(mutation)? != mutation.operation_id
    {
        return Err(invalid_journal());
    }
    p::validate_product_digest(&mutation.plan_digest)?;
    mutation.target_file_identity.validate()
}

pub(super) fn same_acl_target(left: &SandboxAclMutation, right: &SandboxAclMutation) -> bool {
    left.target_path == right.target_path
        && left.target_file_identity == right.target_file_identity
        && left.target_access == right.target_access
}

pub(super) fn calculate_lease_id(binding: &SandboxLeaseBinding) -> p::Result<String> {
    let digest = p::canonical_digest(&("forme-sandbox-lease-v1", binding))?.0;
    Ok(format!(
        "sandbox-lease-{}",
        digest.trim_start_matches("sha256:")
    ))
}

pub(super) fn calculate_mutation_id(mutation: &SandboxAclMutation) -> p::Result<String> {
    let digest = p::canonical_digest(&(
        "forme-sandbox-acl-mutation-v1",
        &mutation.lease_id,
        mutation.operation,
        &mutation.target_path,
        &mutation.target_file_identity,
        mutation.target_access,
    ))?
    .0;
    Ok(format!(
        "sandbox-acl-{}",
        digest.trim_start_matches("sha256:")
    ))
}

pub(super) fn validate_appcontainer_identity(identity: &str) -> p::Result<()> {
    if identity.is_empty()
        || identity.len() > 256
        || !identity
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(p::Error("AppContainer identity is invalid".into()));
    }
    Ok(())
}

pub(super) fn validate_absolute_normalized_path(path: &str) -> p::Result<()> {
    let path = Path::new(path);
    if !path.is_absolute() || !is_lexically_normalized(path) {
        return Err(p::Error(
            "sandbox path is not absolute and normalized".into(),
        ));
    }
    Ok(())
}

fn is_lexically_normalized(path: &Path) -> bool {
    use std::path::Component;
    !path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

pub(super) fn invalid_journal() -> p::Error {
    p::Error("sandbox lease journal requires recovery; worker admission is blocked".into())
}

pub(super) fn storage_error() -> p::Error {
    p::Error("sandbox lease journal storage is unavailable; worker admission is blocked".into())
}
