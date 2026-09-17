#[allow(dead_code)]
mod support;

use forme_protocol as p;
use forme_store::{
    EcosystemEventStore, EcosystemPackageArchive, EcosystemProjection, EcosystemRuntimeLedger,
    EventStore, SqliteEventStore, StoreOptions,
};
use rusqlite::Connection;

use support::TestDatabase;

const EMPTY_SHA256: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn aggregate() -> p::EcosystemAggregateRef {
    p::EcosystemAggregateRef("ecosystem".into())
}

fn version(value: u64) -> p::EcosystemAggregateVersion {
    p::EcosystemAggregateVersion {
        schema_version: p::M5_SCHEMA_VERSION,
        value,
    }
}

fn publisher() -> p::CapabilityPublisherGrant {
    p::CapabilityPublisherGrant {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPublisherGrantRef("grant:publisher:store:v1".into()),
        publisher: p::CapabilityPublisherRef("publisher:store".into()),
        public_key_digest: p::SchemaDigest(EMPTY_SHA256.into()),
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        scope: p::Scope("workspace:store".into()),
        expires_at: 10_000,
        version: p::Version(1),
        status: p::CapabilityPublisherStatus::Active,
    }
}

fn admission() -> p::CapabilityPackageAdmission {
    p::CapabilityPackageAdmission {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityAdmissionRef("admission:store:v1".into()),
        package: p::CapabilityPackageRef("package:store".into()),
        release: p::CapabilityReleaseRef("release:store:v1".into()),
        package_digest: p::SchemaDigest(EMPTY_SHA256.into()),
        publisher_grant: publisher().reference,
        publisher_version: p::Version(1),
        policy: p::CapabilityPolicyRef("policy:store".into()),
        policy_version: p::Version(1),
        checks: p::CapabilityAdmissionCheckKind::ALL
            .into_iter()
            .map(|kind| p::CapabilityAdmissionCheck {
                schema_version: p::M5_SCHEMA_VERSION,
                kind,
                verdict: p::CapabilityAdmissionVerdict::Pass,
                evidence: p::EvidenceRef(format!("evidence:store:{kind:?}")),
            })
            .collect(),
        dependencies: Vec::new(),
        admitted_at: 10,
    }
}

fn package() -> p::SignedCapabilityPackage {
    let mut package = p::SignedCapabilityPackage {
        schema_version: p::M5_SCHEMA_VERSION,
        manifest: p::CapabilityPackageManifest {
            schema_version: p::M5_SCHEMA_VERSION,
            package: p::CapabilityPackageRef("package:store".into()),
            release: p::CapabilityReleaseRef("release:store:v1".into()),
            version: p::Version(1),
            kind: p::CapabilityPackageKind::Skill,
            publisher: p::CapabilityPublisherRef("publisher:store".into()),
            scope: p::Scope("workspace:store".into()),
            contributions: vec![p::CapabilityContributionDescriptor {
                schema_version: p::M5_SCHEMA_VERSION,
                kind: p::CapabilityPackageKind::Skill,
                capability: p::CapabilityRef("skill:store".into()),
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
            max_unpacked_bytes: 1_024,
            contains_executable: false,
        },
        resources: vec![p::CapabilityPackageResource {
            schema_version: p::M5_SCHEMA_VERSION,
            relative_path: "skills/store.txt".into(),
            content: "bounded declarative skill".into(),
            digest: p::SchemaDigest(EMPTY_SHA256.into()),
        }],
        package_digest: p::SchemaDigest(EMPTY_SHA256.into()),
        signature: p::PackageSignature(format!("ed25519:{}", "00".repeat(64))),
    };
    package.refresh_digests().unwrap();
    package.manifest.contributions[0].payload_digest = package.resources[0].digest.clone();
    package.refresh_digests().unwrap();
    package
}

fn owner_event(id: &str, payload: p::EventPayload, timestamp: i64) -> p::Event {
    p::Event::new(
        p::EventId(id.into()),
        p::RunId("run:m5-store".into()),
        None,
        payload,
        p::M5_SCHEMA_VERSION,
        timestamp,
        p::Provenance {
            source: p::Source::OwnerControl,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    )
}

fn verified_event(id: &str, payload: p::EventPayload, timestamp: i64) -> p::Event {
    p::Event::new(
        p::EventId(id.into()),
        p::RunId("run:m5-store".into()),
        None,
        payload,
        p::M5_SCHEMA_VERSION,
        timestamp,
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        },
    )
}

fn append_publisher(store: &SqliteEventStore) {
    let event = owner_event(
        "event:m5-publisher",
        p::EventPayload::CapabilityPublisherChanged(p::CapabilityPublisherChangedPayload {
            grant: publisher(),
            previous: None,
            committed_version: version(1),
        }),
        1,
    );
    let result = store
        .append_ecosystem_expected(event, &aggregate(), version(0))
        .unwrap();
    assert_eq!(result.status, p::ExpectedAppendStatus::Applied);
}

fn append_admission(store: &SqliteEventStore) {
    let event = verified_event(
        "event:m5-admission",
        p::EventPayload::CapabilityPackageAdmitted(p::CapabilityPackageAdmittedPayload {
            admission: admission(),
            committed_version: version(2),
        }),
        11,
    );
    let result = store
        .append_ecosystem_expected(event, &aggregate(), version(1))
        .unwrap();
    assert_eq!(result.status, p::ExpectedAppendStatus::Applied);
}

#[test]
fn legacy_m4_database_opens_with_an_empty_ecosystem_projection() {
    let database = TestDatabase::new();
    drop(SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap());
    let connection = Connection::open(database.path()).unwrap();
    connection
        .execute_batch(
            "DROP TABLE ecosystem_package_bundles;
             DROP TABLE ecosystem_distribution_attempts;
             DROP TABLE ecosystem_nonces;
             DROP TABLE ecosystem_history;
             DROP TABLE ecosystem_distributions;
             DROP TABLE ecosystem_package_states;
             DROP TABLE ecosystem_admissions;
             DROP TABLE ecosystem_publishers;
             DROP TABLE ecosystem_versions;
             PRAGMA user_version = 4;",
        )
        .unwrap();
    drop(connection);

    let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    assert_eq!(store.ecosystem_version(&aggregate()).unwrap(), version(0));
    let snapshot =
        EcosystemProjection::snapshot(&store, p::Scope("workspace:store".into())).unwrap();
    assert_eq!(
        snapshot,
        p::CapabilityEcosystemSnapshot::empty(snapshot.scope.clone())
    );
}

#[test]
fn package_archive_is_content_addressed_conflict_safe_and_restart_durable() {
    let database = TestDatabase::new();
    let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let package = package();
    assert!(store.archive_package(&package).unwrap());
    assert!(!store.archive_package(&package).unwrap());

    let mut conflict = package.clone();
    conflict.resources[0].content = "different body".into();
    conflict.refresh_digests().unwrap();
    conflict.manifest.contributions[0].payload_digest = conflict.resources[0].digest.clone();
    conflict.refresh_digests().unwrap();
    assert!(store.archive_package(&conflict).is_err());
    drop(store);

    let reopened = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    assert_eq!(
        reopened
            .archived_package(&package.manifest.release)
            .unwrap(),
        Some(package)
    );
}

#[test]
fn ecosystem_cas_projects_publisher_admission_and_lifecycle_atomically() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    append_publisher(&store);
    append_admission(&store);

    let install = p::CapabilityPackageStateChange {
        schema_version: p::M5_SCHEMA_VERSION,
        plan: p::CapabilityInstallPlanRef("plan:store:install".into()),
        approval: p::ApprovalId("approval:store:install".into()),
        package: admission().package,
        release: admission().release,
        from: p::CapabilityLifecycleState::Admitted,
        to: p::CapabilityLifecycleState::Installed,
        active_generation: 1,
        reason: p::ReasonRef("owner approved install".into()),
        external_effects_reverted: false,
    };
    let event = owner_event(
        "event:m5-installed",
        p::EventPayload::CapabilityPackageStateChanged(p::CapabilityPackageStateChangedPayload {
            change: install,
            committed_version: version(3),
        }),
        12,
    );
    assert_eq!(
        store
            .append_ecosystem_expected(event.clone(), &aggregate(), version(2))
            .unwrap()
            .status,
        p::ExpectedAppendStatus::Applied
    );
    assert_eq!(
        store
            .append_ecosystem_expected(event, &aggregate(), version(2))
            .unwrap()
            .status,
        p::ExpectedAppendStatus::Duplicate
    );

    let stale = owner_event(
        "event:m5-stale",
        p::EventPayload::CapabilityPackageStateChanged(p::CapabilityPackageStateChangedPayload {
            change: p::CapabilityPackageStateChange {
                schema_version: p::M5_SCHEMA_VERSION,
                plan: p::CapabilityInstallPlanRef("plan:store:enable".into()),
                approval: p::ApprovalId("approval:store:enable".into()),
                package: admission().package,
                release: admission().release,
                from: p::CapabilityLifecycleState::Installed,
                to: p::CapabilityLifecycleState::Enabled,
                active_generation: 2,
                reason: p::ReasonRef("owner approved enable".into()),
                external_effects_reverted: false,
            },
            committed_version: version(3),
        }),
        13,
    );
    assert_eq!(
        store
            .append_ecosystem_expected(stale, &aggregate(), version(2))
            .unwrap()
            .status,
        p::ExpectedAppendStatus::Conflict
    );
    let state = EcosystemProjection::package_state(&store, &admission().package)
        .unwrap()
        .unwrap();
    assert_eq!(state.lifecycle, p::CapabilityLifecycleState::Installed);
    assert_eq!(state.active_generation, 1);
    assert_eq!(store.ecosystem_version(&aggregate()).unwrap(), version(3));
}

#[test]
fn ecosystem_controls_cannot_use_generic_append_and_nonce_is_restart_durable() {
    let database = TestDatabase::new();
    let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let event = owner_event(
        "event:m5-wrong-path",
        p::EventPayload::CapabilityPublisherChanged(p::CapabilityPublisherChangedPayload {
            grant: publisher(),
            previous: None,
            committed_version: version(1),
        }),
        1,
    );
    assert!(store.append(event).is_err());
    let nonce = p::Nonce("nonce:m5-store".into());
    let plan = p::PlanDigest("plan-digest:m5-store".into());
    assert!(store.claim_ecosystem_nonce(&nonce, &plan).unwrap());
    drop(store);

    let reopened = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    assert!(!reopened.claim_ecosystem_nonce(&nonce, &plan).unwrap());
    assert!(reopened
        .claim_ecosystem_nonce(&nonce, &p::PlanDigest("different".into()))
        .is_err());
}
