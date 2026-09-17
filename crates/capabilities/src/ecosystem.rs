//! M5 publisher verification and declarative package registry.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use ed25519_dalek::{Signature, VerifyingKey};
use forme_protocol as p;
use sha2::{Digest, Sha256};

use crate::{Capability, CapabilityDescriptor, CapabilitySource, InMemoryCapabilityRegistry};

#[derive(Clone, PartialEq, Eq)]
pub struct PublisherPublicKey([u8; 32]);

impl PublisherPublicKey {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn digest(&self) -> p::SchemaDigest {
        sha256_prefixed(&self.0)
    }
}

impl fmt::Debug for PublisherPublicKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PublisherPublicKey")
            .field(&self.digest())
            .finish()
    }
}

pub trait PublisherKeyring: Send + Sync {
    fn public_key(&self, publisher: &p::CapabilityPublisherRef) -> p::Result<PublisherPublicKey>;
}

#[derive(Default)]
pub struct InMemoryPublisherKeyring {
    keys: Mutex<BTreeMap<p::CapabilityPublisherRef, PublisherPublicKey>>,
}

impl InMemoryPublisherKeyring {
    pub fn provision(
        &self,
        publisher: p::CapabilityPublisherRef,
        key: PublisherPublicKey,
    ) -> p::Result<()> {
        if publisher.0.trim().is_empty() {
            return Err(p::Error("publisher key identity is empty".into()));
        }
        self.keys
            .lock()
            .map_err(|_| p::Error("publisher keyring is unavailable".into()))?
            .insert(publisher, key);
        Ok(())
    }

    pub fn revoke(&self, publisher: &p::CapabilityPublisherRef) -> p::Result<bool> {
        Ok(self
            .keys
            .lock()
            .map_err(|_| p::Error("publisher keyring is unavailable".into()))?
            .remove(publisher)
            .is_some())
    }
}

impl PublisherKeyring for InMemoryPublisherKeyring {
    fn public_key(&self, publisher: &p::CapabilityPublisherRef) -> p::Result<PublisherPublicKey> {
        self.keys
            .lock()
            .map_err(|_| p::Error("publisher keyring is unavailable".into()))?
            .get(publisher)
            .cloned()
            .ok_or_else(|| p::Error("publisher key is not provisioned".into()))
    }
}

pub trait CapabilityPackageVerifier: Send + Sync {
    fn verify(
        &self,
        package: &p::SignedCapabilityPackage,
        grant: &p::CapabilityPublisherGrant,
        policy: &p::CapabilityAdmissionPolicy,
        admitted_dependencies: &[p::CapabilityPackageAdmission],
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageAdmission>;
}

pub struct Ed25519CapabilityPackageVerifier<K> {
    keyring: Arc<K>,
}

impl<K> Ed25519CapabilityPackageVerifier<K> {
    pub fn new(keyring: Arc<K>) -> Self {
        Self { keyring }
    }
}

impl<K> CapabilityPackageVerifier for Ed25519CapabilityPackageVerifier<K>
where
    K: PublisherKeyring,
{
    fn verify(
        &self,
        package: &p::SignedCapabilityPackage,
        grant: &p::CapabilityPublisherGrant,
        policy: &p::CapabilityAdmissionPolicy,
        admitted_dependencies: &[p::CapabilityPackageAdmission],
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageAdmission> {
        if now <= 0 {
            return Err(p::Error("package admission time is invalid".into()));
        }
        package.validate()?;
        grant.validate()?;
        policy.validate()?;
        verify_size(package, policy)?;
        verify_publisher(package, grant, now)?;
        verify_signature(package, grant, self.keyring.as_ref())?;
        verify_dependencies(package, admitted_dependencies, policy)?;
        verify_license_and_sbom(package, policy)?;
        verify_secret_boundary(package)?;
        verify_risk_and_policy(package, policy)?;

        let evidence_root = package
            .package_digest
            .0
            .strip_prefix("sha256:")
            .unwrap_or(&package.package_digest.0);
        let checks = p::CapabilityAdmissionCheckKind::ALL
            .into_iter()
            .map(|kind| p::CapabilityAdmissionCheck {
                schema_version: p::M5_SCHEMA_VERSION,
                kind,
                verdict: p::CapabilityAdmissionVerdict::Pass,
                evidence: p::EvidenceRef(format!(
                    "evidence:admission:{}:{}",
                    admission_check_name(kind),
                    evidence_root
                )),
            })
            .collect();
        let admission = p::CapabilityPackageAdmission {
            schema_version: p::M5_SCHEMA_VERSION,
            reference: p::CapabilityAdmissionRef(format!("admission:{evidence_root}")),
            package: package.manifest.package.clone(),
            release: package.manifest.release.clone(),
            package_digest: package.package_digest.clone(),
            publisher_grant: grant.reference.clone(),
            publisher_version: grant.version,
            policy: policy.reference.clone(),
            policy_version: policy.version,
            checks,
            dependencies: package.manifest.dependencies.clone(),
            admitted_at: now,
        };
        admission.validate()?;
        Ok(admission)
    }
}

fn verify_size(
    package: &p::SignedCapabilityPackage,
    policy: &p::CapabilityAdmissionPolicy,
) -> p::Result<()> {
    let total = package.resources.iter().try_fold(0_u64, |sum, resource| {
        sum.checked_add(resource.content.len() as u64)
            .ok_or_else(|| p::Error("package size exceeds the supported range".into()))
    })?;
    if total > policy.max_package_bytes || total > package.manifest.max_unpacked_bytes {
        return Err(p::Error("package exceeds the admission size limit".into()));
    }
    Ok(())
}

fn verify_publisher(
    package: &p::SignedCapabilityPackage,
    grant: &p::CapabilityPublisherGrant,
    now: p::Timestamp,
) -> p::Result<()> {
    if grant.status != p::CapabilityPublisherStatus::Active
        || grant.expires_at <= now
        || package.manifest.publisher != grant.publisher
        || !grant.allowed_kinds.contains(&package.manifest.kind)
        || !scope_contains(&grant.scope, &package.manifest.scope)
    {
        return Err(p::Error(
            "package is outside its active publisher grant".into(),
        ));
    }
    Ok(())
}

fn verify_signature<K: PublisherKeyring>(
    package: &p::SignedCapabilityPackage,
    grant: &p::CapabilityPublisherGrant,
    keyring: &K,
) -> p::Result<()> {
    let key = keyring.public_key(&grant.publisher)?;
    if key.digest() != grant.public_key_digest {
        return Err(p::Error(
            "publisher key does not match the owner grant".into(),
        ));
    }
    let verifying_key = VerifyingKey::from_bytes(key.as_bytes())
        .map_err(|_| p::Error("publisher verification key is invalid".into()))?;
    let signature_bytes = decode_signature(&package.signature)?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| p::Error("package signature is invalid".into()))?;
    let digest = p::sha256_digest_bytes(&package.package_digest)?;
    verifying_key
        .verify_strict(&digest, &signature)
        .map_err(|_| p::Error("package signature verification failed".into()))
}

fn verify_dependencies(
    package: &p::SignedCapabilityPackage,
    admitted: &[p::CapabilityPackageAdmission],
    policy: &p::CapabilityAdmissionPolicy,
) -> p::Result<()> {
    if package.manifest.dependencies.len() > policy.max_dependencies as usize
        || admitted.len() > policy.max_dependencies as usize
    {
        return Err(p::Error("package dependency count exceeds policy".into()));
    }
    let by_release = admitted
        .iter()
        .map(|admission| (admission.release.clone(), admission))
        .collect::<BTreeMap<_, _>>();
    if by_release.len() != admitted.len() {
        return Err(p::Error(
            "admitted dependency identity is duplicated".into(),
        ));
    }
    for dependency in &package.manifest.dependencies {
        let admission = by_release
            .get(&dependency.release)
            .ok_or_else(|| p::Error("package dependency is not admitted".into()))?;
        admission.validate()?;
        if admission.package != dependency.package || admission.package_digest != dependency.digest
        {
            return Err(p::Error("package dependency binding does not match".into()));
        }
    }
    if by_release.len() != package.manifest.dependencies.len() {
        return Err(p::Error(
            "dependency closure contains undeclared admissions".into(),
        ));
    }
    let root = &package.manifest.release;
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for dependency in &package.manifest.dependencies {
        dependency_depth(
            &dependency.release,
            root,
            &by_release,
            1,
            policy.max_depth,
            &mut visiting,
            &mut visited,
        )?;
    }
    Ok(())
}

fn dependency_depth(
    release: &p::CapabilityReleaseRef,
    root: &p::CapabilityReleaseRef,
    admitted: &BTreeMap<p::CapabilityReleaseRef, &p::CapabilityPackageAdmission>,
    depth: u32,
    limit: u32,
    visiting: &mut BTreeSet<p::CapabilityReleaseRef>,
    visited: &mut BTreeSet<p::CapabilityReleaseRef>,
) -> p::Result<()> {
    if depth > limit || release == root || !visiting.insert(release.clone()) {
        return Err(p::Error(
            "package dependency graph is cyclic or too deep".into(),
        ));
    }
    if visited.contains(release) {
        visiting.remove(release);
        return Ok(());
    }
    let admission = admitted
        .get(release)
        .ok_or_else(|| p::Error("dependency closure is incomplete".into()))?;
    for dependency in &admission.dependencies {
        let child = admitted
            .get(&dependency.release)
            .ok_or_else(|| p::Error("transitive dependency closure is incomplete".into()))?;
        if child.package != dependency.package || child.package_digest != dependency.digest {
            return Err(p::Error("transitive dependency binding changed".into()));
        }
        dependency_depth(
            &dependency.release,
            root,
            admitted,
            depth.saturating_add(1),
            limit,
            visiting,
            visited,
        )?;
    }
    visiting.remove(release);
    visited.insert(release.clone());
    Ok(())
}

fn verify_license_and_sbom(
    package: &p::SignedCapabilityPackage,
    policy: &p::CapabilityAdmissionPolicy,
) -> p::Result<()> {
    p::sha256_digest_bytes(&package.manifest.sbom_digest)?;
    if !policy
        .allowed_licenses
        .contains(&package.manifest.license_expression)
    {
        return Err(p::Error("package license is not admitted by policy".into()));
    }
    Ok(())
}

fn verify_secret_boundary(package: &p::SignedCapabilityPackage) -> p::Result<()> {
    for resource in &package.resources {
        let path = resource.relative_path.to_ascii_lowercase();
        if path.split('/').any(|segment| {
            segment == ".env"
                || segment == "id_rsa"
                || segment.ends_with(".pem")
                || segment.ends_with(".key")
                || segment.contains("secret")
        }) || contains_secret_marker(&resource.content)
        {
            return Err(p::Error(
                "package resource crosses the credential boundary".into(),
            ));
        }
    }
    Ok(())
}

fn contains_secret_marker(content: &str) -> bool {
    let normalized = content.to_ascii_lowercase();
    [
        "secretref",
        "begin private key",
        "authorization:",
        "bearer ",
        "api_key",
        "api-key",
        "private endpoint",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn verify_risk_and_policy(
    package: &p::SignedCapabilityPackage,
    policy: &p::CapabilityAdmissionPolicy,
) -> p::Result<()> {
    if !policy.allowed_kinds.contains(&package.manifest.kind) {
        return Err(p::Error("package kind is outside admission policy".into()));
    }
    for contribution in &package.manifest.contributions {
        if (contribution.network && !policy.allow_network)
            || (contribution.hook && !policy.allow_hooks)
        {
            return Err(p::Error(
                "package declares behavior excluded by admission policy".into(),
            ));
        }
    }
    Ok(())
}

fn decode_signature(signature: &p::PackageSignature) -> p::Result<[u8; 64]> {
    signature.validate()?;
    let encoded = signature
        .0
        .strip_prefix("ed25519:")
        .ok_or_else(|| p::Error("package signature encoding is invalid".into()))?;
    let mut bytes = [0_u8; 64];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let pair = core::str::from_utf8(pair)
            .map_err(|_| p::Error("package signature encoding is invalid".into()))?;
        bytes[index] = u8::from_str_radix(pair, 16)
            .map_err(|_| p::Error("package signature encoding is invalid".into()))?;
    }
    Ok(bytes)
}

fn sha256_prefixed(bytes: &[u8]) -> p::SchemaDigest {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::from("sha256:");
    for byte in digest {
        use core::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    p::SchemaDigest(encoded)
}

fn admission_check_name(kind: p::CapabilityAdmissionCheckKind) -> &'static str {
    match kind {
        p::CapabilityAdmissionCheckKind::Schema => "schema",
        p::CapabilityAdmissionCheckKind::Size => "size",
        p::CapabilityAdmissionCheckKind::ClosedSet => "closed-set",
        p::CapabilityAdmissionCheckKind::Path => "path",
        p::CapabilityAdmissionCheckKind::Digest => "digest",
        p::CapabilityAdmissionCheckKind::Publisher => "publisher",
        p::CapabilityAdmissionCheckKind::Signature => "signature",
        p::CapabilityAdmissionCheckKind::Dependency => "dependency",
        p::CapabilityAdmissionCheckKind::Sbom => "sbom",
        p::CapabilityAdmissionCheckKind::License => "license",
        p::CapabilityAdmissionCheckKind::Secret => "secret",
        p::CapabilityAdmissionCheckKind::Risk => "risk",
        p::CapabilityAdmissionCheckKind::Policy => "policy",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedCapabilityPackage {
    pub package: p::CapabilityPackageRef,
    pub release: p::CapabilityReleaseRef,
    pub package_digest: p::SchemaDigest,
    pub source: CapabilitySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePackageSource {
    pub release: p::CapabilityReleaseRef,
    pub source: p::CapabilitySourceRef,
    pub generation: u64,
}

pub struct CapabilityPackageRegistry {
    registry: Arc<InMemoryCapabilityRegistry>,
    state: Mutex<PackageRegistryState>,
}

#[derive(Default)]
struct PackageRegistryState {
    archived: BTreeMap<p::CapabilityReleaseRef, p::SignedCapabilityPackage>,
    active: BTreeMap<p::CapabilityPackageRef, ActivePackageSource>,
    generations: BTreeMap<p::CapabilityPackageRef, u64>,
}

impl CapabilityPackageRegistry {
    pub fn new(registry: Arc<InMemoryCapabilityRegistry>) -> Self {
        Self {
            registry,
            state: Mutex::new(PackageRegistryState::default()),
        }
    }

    pub fn archive(&self, package: p::SignedCapabilityPackage) -> p::Result<bool> {
        package.validate()?;
        let mut state = self.lock_state()?;
        match state.archived.get(&package.manifest.release) {
            Some(existing) if existing == &package => Ok(false),
            Some(_) => Err(p::Error(
                "package release identity changed immutable content".into(),
            )),
            None => {
                state
                    .archived
                    .insert(package.manifest.release.clone(), package);
                Ok(true)
            }
        }
    }

    pub fn archived(
        &self,
        release: &p::CapabilityReleaseRef,
    ) -> p::Result<Option<p::SignedCapabilityPackage>> {
        Ok(self.lock_state()?.archived.get(release).cloned())
    }

    pub fn stage(
        &self,
        package: &p::SignedCapabilityPackage,
    ) -> p::Result<StagedCapabilityPackage> {
        package.validate()?;
        let source_ref = package_source_ref(&package.manifest.package, &package.manifest.release);
        let entries = package
            .manifest
            .contributions
            .iter()
            .map(|contribution| {
                Ok(CapabilityDescriptor {
                    schema_version: p::M5_SCHEMA_VERSION,
                    id: contribution.capability.clone(),
                    capability: contribution_capability(contribution),
                    scope: package.manifest.scope.clone(),
                    permissions: contribution.required_permissions.clone(),
                    risk: Some(contribution.risk),
                    enabled: true,
                })
            })
            .collect::<p::Result<Vec<_>>>()?;
        let staged = StagedCapabilityPackage {
            package: package.manifest.package.clone(),
            release: package.manifest.release.clone(),
            package_digest: package.package_digest.clone(),
            source: CapabilitySource {
                schema_version: p::M5_SCHEMA_VERSION,
                source_ref,
                trust: p::TrustTier::ApprovedSource,
                entries,
            },
        };
        if staged.source.entries.is_empty() {
            return Err(p::Error("package contribution source is empty".into()));
        }
        Ok(staged)
    }

    pub fn enable(&self, staged: StagedCapabilityPackage, generation: u64) -> p::Result<()> {
        if generation == 0 {
            return Err(p::Error("package active generation is zero".into()));
        }
        let mut state = self.lock_state()?;
        let mut managed = vec![staged.source.source_ref.clone()];
        if let Some(previous) = state.generations.get(&staged.package) {
            if generation != previous.checked_add(1).unwrap_or(0) {
                return Err(p::Error("package registry generation is stale".into()));
            }
        }
        if let Some(current) = state.active.get(&staged.package) {
            managed.push(current.source.clone());
        }
        managed.sort();
        managed.dedup();
        self.registry
            .replace_sources(&managed, vec![staged.source.clone()])?;
        state.generations.insert(staged.package.clone(), generation);
        state.active.insert(
            staged.package,
            ActivePackageSource {
                release: staged.release,
                source: staged.source.source_ref,
                generation,
            },
        );
        Ok(())
    }

    pub fn disable(&self, package: &p::CapabilityPackageRef, generation: u64) -> p::Result<bool> {
        let mut state = self.lock_state()?;
        let previous = state
            .generations
            .get(package)
            .copied()
            .ok_or_else(|| p::Error("package registry has no lifecycle generation".into()))?;
        if generation != previous.checked_add(1).unwrap_or(0) {
            return Err(p::Error("package registry generation is stale".into()));
        }
        let current = state.active.get(package).cloned();
        if let Some(current) = &current {
            self.registry.remove_source(&current.source)?;
        }
        state.active.remove(package);
        state.generations.insert(package.clone(), generation);
        Ok(current.is_some())
    }

    pub fn active(
        &self,
        package: &p::CapabilityPackageRef,
    ) -> p::Result<Option<ActivePackageSource>> {
        Ok(self.lock_state()?.active.get(package).cloned())
    }

    pub fn fence(&self, package: &p::CapabilityPackageRef) -> p::Result<bool> {
        let mut state = self.lock_state()?;
        let Some(current) = state.active.get(package).cloned() else {
            return Ok(false);
        };
        self.registry.remove_source(&current.source)?;
        state.active.remove(package);
        Ok(true)
    }

    pub fn inner(&self) -> Arc<InMemoryCapabilityRegistry> {
        self.registry.clone()
    }

    fn lock_state(&self) -> p::Result<MutexGuard<'_, PackageRegistryState>> {
        self.state
            .lock()
            .map_err(|_| p::Error("capability package registry is unavailable".into()))
    }
}

fn package_source_ref(
    package: &p::CapabilityPackageRef,
    release: &p::CapabilityReleaseRef,
) -> p::CapabilitySourceRef {
    p::CapabilitySourceRef(format!("ecosystem:{}:{}", package.0, release.0))
}

fn contribution_capability(contribution: &p::CapabilityContributionDescriptor) -> Capability {
    match contribution.kind {
        p::CapabilityPackageKind::Connector => {
            Capability::AppApi(p::ProviderId(contribution.capability.0.clone()))
        }
        p::CapabilityPackageKind::Plugin => Capability::PluginContribution(
            p::PluginContributionRef(contribution.capability.0.clone()),
        ),
        p::CapabilityPackageKind::Skill => {
            Capability::Skill(p::SkillRef(contribution.capability.0.clone()))
        }
        p::CapabilityPackageKind::McpServer => {
            Capability::McpTool(p::McpToolRef(contribution.capability.0.clone()))
        }
        p::CapabilityPackageKind::AgentProfile => {
            Capability::AgentProfile(p::AgentProfileRef(contribution.capability.0.clone()))
        }
    }
}

fn scope_contains(granted: &p::Scope, requested: &p::Scope) -> bool {
    if granted.0 == "*" || granted == requested {
        return true;
    }
    requested
        .0
        .strip_prefix(&granted.0)
        .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with('/'))
}
