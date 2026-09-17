use std::sync::Arc;

use ed25519_dalek::{Signer, SigningKey};
use forme_capabilities::{
    Capability, CapabilityPackageRegistry, CapabilityPackageVerifier, CapabilityRegistry,
    Ed25519CapabilityPackageVerifier, InMemoryCapabilityRegistry, InMemoryPublisherKeyring,
    PublisherPublicKey,
};
use forme_protocol as p;

const EMPTY_SHA256: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn signed_package(
    key: &SigningKey,
    version: u32,
    capability: &str,
    content: &str,
) -> p::SignedCapabilityPackage {
    let release = p::CapabilityReleaseRef(format!("release:ecosystem:v{version}"));
    let mut package = p::SignedCapabilityPackage {
        schema_version: p::M5_SCHEMA_VERSION,
        manifest: p::CapabilityPackageManifest {
            schema_version: p::M5_SCHEMA_VERSION,
            package: p::CapabilityPackageRef("package:ecosystem".into()),
            release,
            version: p::Version(version),
            kind: p::CapabilityPackageKind::Skill,
            publisher: p::CapabilityPublisherRef("publisher:ecosystem".into()),
            scope: p::Scope("workspace:ecosystem".into()),
            contributions: vec![p::CapabilityContributionDescriptor {
                schema_version: p::M5_SCHEMA_VERSION,
                kind: p::CapabilityPackageKind::Skill,
                capability: p::CapabilityRef(capability.into()),
                payload_digest: p::SchemaDigest(EMPTY_SHA256.into()),
                required_permissions: vec![p::PermissionRef("permission:read".into())],
                risk: p::Risk::Low,
                network: false,
                hook: false,
            }],
            dependencies: Vec::new(),
            sbom_digest: p::SchemaDigest(EMPTY_SHA256.into()),
            license_expression: "Apache-2.0".into(),
            body_digest: p::SchemaDigest(EMPTY_SHA256.into()),
            max_unpacked_bytes: 4_096,
            contains_executable: false,
        },
        resources: vec![p::CapabilityPackageResource {
            schema_version: p::M5_SCHEMA_VERSION,
            relative_path: format!("skills/v{version}.txt"),
            content: content.into(),
            digest: p::SchemaDigest(EMPTY_SHA256.into()),
        }],
        package_digest: p::SchemaDigest(EMPTY_SHA256.into()),
        signature: p::PackageSignature(format!("ed25519:{}", "00".repeat(64))),
    };
    resign(&mut package, key);
    package
}

fn resign(package: &mut p::SignedCapabilityPackage, key: &SigningKey) {
    package.refresh_digests().unwrap();
    package.manifest.contributions[0].payload_digest = package.resources[0].digest.clone();
    package.refresh_digests().unwrap();
    let digest = p::sha256_digest_bytes(&package.package_digest).unwrap();
    let signature = key.sign(&digest);
    package.signature = p::PackageSignature(format!(
        "ed25519:{}",
        signature
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ));
    package.validate().unwrap();
}

fn keyring_and_grant(
    key: &SigningKey,
) -> (Arc<InMemoryPublisherKeyring>, p::CapabilityPublisherGrant) {
    let public = PublisherPublicKey::from_bytes(key.verifying_key().to_bytes());
    let keyring = Arc::new(InMemoryPublisherKeyring::default());
    keyring
        .provision(
            p::CapabilityPublisherRef("publisher:ecosystem".into()),
            public.clone(),
        )
        .unwrap();
    let grant = p::CapabilityPublisherGrant {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPublisherGrantRef("grant:ecosystem:v1".into()),
        publisher: p::CapabilityPublisherRef("publisher:ecosystem".into()),
        public_key_digest: public.digest(),
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        scope: p::Scope("workspace:ecosystem".into()),
        expires_at: 10_000,
        version: p::Version(1),
        status: p::CapabilityPublisherStatus::Active,
    };
    (keyring, grant)
}

fn policy() -> p::CapabilityAdmissionPolicy {
    p::CapabilityAdmissionPolicy {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPolicyRef("policy:ecosystem".into()),
        version: p::Version(1),
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        allowed_licenses: vec!["Apache-2.0".into()],
        max_package_bytes: 4_096,
        max_dependencies: 4,
        max_depth: 3,
        allow_network: false,
        allow_hooks: false,
    }
}

#[test]
fn s87_real_ed25519_verification_is_digest_key_and_grant_bound() {
    let key = signing_key(7);
    let (keyring, grant) = keyring_and_grant(&key);
    let verifier = Ed25519CapabilityPackageVerifier::new(keyring.clone());
    let package = signed_package(&key, 1, "skill:ecosystem:v1", "bounded skill body");
    let admission = verifier
        .verify(&package, &grant, &policy(), &[], 100)
        .unwrap();
    assert_eq!(admission.checks.len(), 13);
    assert!(admission
        .checks
        .iter()
        .all(|check| check.verdict == p::CapabilityAdmissionVerdict::Pass));

    let mut tampered = package.clone();
    tampered.resources[0].content.push_str(" changed");
    assert!(verifier
        .verify(&tampered, &grant, &policy(), &[], 100)
        .is_err());

    let wrong_key = signing_key(9);
    keyring
        .provision(
            grant.publisher.clone(),
            PublisherPublicKey::from_bytes(wrong_key.verifying_key().to_bytes()),
        )
        .unwrap();
    assert!(verifier
        .verify(&package, &grant, &policy(), &[], 100)
        .is_err());

    let (_, mut rebound) = keyring_and_grant(&key);
    rebound.publisher = p::CapabilityPublisherRef("publisher:other".into());
    assert!(verifier
        .verify(&package, &rebound, &policy(), &[], 100)
        .is_err());
}

#[test]
fn s88_every_supply_chain_hard_failure_blocks_admission() {
    let key = signing_key(11);
    let (keyring, grant) = keyring_and_grant(&key);
    let verifier = Ed25519CapabilityPackageVerifier::new(keyring);

    let secret = signed_package(&key, 1, "skill:ecosystem:v1", "api_key = forbidden");
    assert!(verifier
        .verify(&secret, &grant, &policy(), &[], 100)
        .is_err());

    let package = signed_package(&key, 1, "skill:ecosystem:v1", "ordinary body");
    let mut wrong_license = policy();
    wrong_license.allowed_licenses = vec!["MIT".into()];
    assert!(verifier
        .verify(&package, &grant, &wrong_license, &[], 100)
        .is_err());

    let mut too_small = policy();
    too_small.max_package_bytes = 1;
    assert!(verifier
        .verify(&package, &grant, &too_small, &[], 100)
        .is_err());

    let mut network = package.clone();
    network.manifest.contributions[0].network = true;
    resign(&mut network, &key);
    assert!(verifier
        .verify(&network, &grant, &policy(), &[], 100)
        .is_err());

    let mut missing_dependency = package;
    missing_dependency
        .manifest
        .dependencies
        .push(p::CapabilityPackageDependency {
            schema_version: p::M5_SCHEMA_VERSION,
            package: p::CapabilityPackageRef("package:dependency".into()),
            release: p::CapabilityReleaseRef("release:dependency:v1".into()),
            version: p::Version(1),
            digest: p::SchemaDigest(EMPTY_SHA256.into()),
        });
    resign(&mut missing_dependency, &key);
    assert!(verifier
        .verify(&missing_dependency, &grant, &policy(), &[], 100)
        .is_err());
}

#[test]
fn s90_s91_registry_staging_is_invisible_and_switches_complete_sources() {
    let key = signing_key(13);
    let capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let packages = CapabilityPackageRegistry::new(capabilities.clone());
    let v1 = signed_package(&key, 1, "skill:ecosystem:v1", "v1 body");
    packages.archive(v1.clone()).unwrap();
    let staged_v1 = packages.stage(&v1).unwrap();
    assert!(CapabilityRegistry::resolve_toolset(
        capabilities.as_ref(),
        &resolve_context(&["skill:ecosystem:v1"]),
    )
    .unwrap()
    .items
    .is_empty());
    packages.enable(staged_v1, 2).unwrap();
    assert_eq!(
        CapabilityRegistry::resolve_toolset(
            capabilities.as_ref(),
            &resolve_context(&["skill:ecosystem:v1"]),
        )
        .unwrap()
        .items
        .len(),
        1
    );

    let mut invalid_v2 = signed_package(&key, 2, "skill:ecosystem:v2", "v2 body");
    invalid_v2.resources[0].relative_path = "../escape".into();
    assert!(packages.stage(&invalid_v2).is_err());
    assert!(packages
        .active(&p::CapabilityPackageRef("package:ecosystem".into()))
        .unwrap()
        .is_some());

    let v2 = signed_package(&key, 2, "skill:ecosystem:v2", "v2 body");
    packages.enable(packages.stage(&v2).unwrap(), 3).unwrap();
    let switched = CapabilityRegistry::resolve_toolset(
        capabilities.as_ref(),
        &resolve_context(&["skill:ecosystem:v1", "skill:ecosystem:v2"]),
    )
    .unwrap();
    assert_eq!(
        switched
            .items
            .iter()
            .map(|item| item.id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["skill:ecosystem:v2"]
    );
    assert!(packages
        .disable(&p::CapabilityPackageRef("package:ecosystem".into()), 4)
        .unwrap());
    assert!(CapabilityRegistry::resolve_toolset(
        capabilities.as_ref(),
        &resolve_context(&["skill:ecosystem:v2"]),
    )
    .unwrap()
    .items
    .is_empty());
}

#[test]
fn s94_package_hook_and_agent_profile_remain_declarative_registry_entries() {
    let key = signing_key(17);
    let capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let packages = CapabilityPackageRegistry::new(capabilities.clone());

    let mut plugin = signed_package(
        &key,
        1,
        "plugin:ecosystem:bounded",
        "declarative hook metadata only",
    );
    plugin.manifest.package = p::CapabilityPackageRef("package:ecosystem-plugin".into());
    plugin.manifest.release = p::CapabilityReleaseRef("release:ecosystem-plugin:v1".into());
    plugin.manifest.kind = p::CapabilityPackageKind::Plugin;
    plugin.manifest.contributions[0].kind = p::CapabilityPackageKind::Plugin;
    plugin.manifest.contributions[0].hook = true;
    plugin.resources[0].relative_path = "plugins/bounded.txt".into();
    resign(&mut plugin, &key);
    packages
        .enable(packages.stage(&plugin).unwrap(), 1)
        .unwrap();
    let resolved = CapabilityRegistry::resolve_toolset(
        capabilities.as_ref(),
        &resolve_context(&["plugin:ecosystem:bounded"]),
    )
    .unwrap();
    assert!(matches!(
        resolved.items.as_slice(),
        [item]
            if item.capability
                == Capability::PluginContribution(p::PluginContributionRef(
                    "plugin:ecosystem:bounded".into()
                ))
    ));
    assert!(!resolved
        .items
        .iter()
        .any(|item| matches!(item.capability, Capability::Hook(_))));

    let agent_capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let agent_packages = CapabilityPackageRegistry::new(agent_capabilities.clone());
    let mut agent = signed_package(
        &key,
        2,
        "agent-profile:ecosystem:reviewer",
        "role=reviewer; toolset=bounded; memory=none; policy_owner=none",
    );
    agent.manifest.package = p::CapabilityPackageRef("package:ecosystem-agent".into());
    agent.manifest.release = p::CapabilityReleaseRef("release:ecosystem-agent:v1".into());
    agent.manifest.kind = p::CapabilityPackageKind::AgentProfile;
    agent.manifest.contributions[0].kind = p::CapabilityPackageKind::AgentProfile;
    agent.resources[0].relative_path = "agents/reviewer.txt".into();
    resign(&mut agent, &key);
    agent_packages
        .enable(agent_packages.stage(&agent).unwrap(), 1)
        .unwrap();
    let resolved = CapabilityRegistry::resolve_toolset(
        agent_capabilities.as_ref(),
        &resolve_context(&["agent-profile:ecosystem:reviewer"]),
    )
    .unwrap();
    assert!(matches!(
        resolved.items.as_slice(),
        [item]
            if item.capability
                == Capability::AgentProfile(p::AgentProfileRef(
                    "agent-profile:ecosystem:reviewer".into()
                ))
    ));
}

fn resolve_context(capabilities: &[&str]) -> p::ResolveContext {
    let permission = p::PermissionRef("permission:read".into());
    let capabilities = capabilities
        .iter()
        .map(|value| p::CapabilityRef((*value).into()))
        .collect::<Vec<_>>();
    p::ResolveContext {
        schema_version: p::M5_SCHEMA_VERSION,
        session: p::SessionId("session:ecosystem".into()),
        toolset: p::ToolsetRef("toolset:ecosystem".into()),
        envelope: p::AutonomyEnvelope {
            schema_version: p::M5_SCHEMA_VERSION,
            scope: p::Scope("workspace:ecosystem".into()),
            capability: p::CapabilitySet {
                schema_version: p::M5_SCHEMA_VERSION,
                capabilities: capabilities.clone(),
                permissions: vec![permission],
            },
            action_type: vec![p::ActionType::Analyze],
            risk_limit: p::Risk::Low,
            approval_rule: p::ApprovalRule::Ask,
            budget: p::Budget("budget:ecosystem".into()),
            timebox: p::Timebox {
                schema_version: p::M5_SCHEMA_VERSION,
                starts_at: 1,
                expires_at: 10_000,
                max_turns: 1,
            },
            rollback: p::RollbackReq {
                schema_version: p::M5_SCHEMA_VERSION,
                required: false,
                boundary: None,
            },
        },
        policy_allowed_providers: Vec::new(),
        policy_allowed_capabilities: capabilities,
    }
}
