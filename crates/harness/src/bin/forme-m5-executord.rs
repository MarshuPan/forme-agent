#![forbid(unsafe_code)]

use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

use forme_capabilities::{InMemoryPublisherKeyring, PublisherPublicKey};
use forme_execution::{
    ExecutorAdmissionProfile, FileExecutorLedger, GuardedRemoteExecutor, SystemRemoteClock,
    TlsIdentityFiles, TlsRemoteExecutorServer, TlsRemoteServerConfig,
};
use forme_harness::{CapabilityPackageReceiver, FileCapabilityPackageLedger};
use forme_protocol as p;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme-m5-executord: {}", error.0);
        std::process::exit(1);
    }
}

fn run() -> p::Result<()> {
    let peer_grant: p::FederatedPeerGrant = serde_json::from_slice(&read_file(
        &required_path("FORME_EXECUTOR_GRANT_PATH")?,
        "executor grant",
    )?)
    .map_err(|_| p::Error("executor grant file is malformed".into()))?;
    peer_grant.validate()?;
    let peer = p::FederatedPeerRef(required("FORME_EXECUTOR_PEER")?);
    let authority_epoch = p::AuthorityEpoch(required_u64("FORME_AUTHORITY_EPOCH")?);
    if peer != peer_grant.peer {
        return Err(p::Error(
            "executor identity does not match its grant".into(),
        ));
    }

    let publisher_grant: p::CapabilityPublisherGrant = serde_json::from_slice(&read_file(
        &required_path("FORME_M5_PUBLISHER_GRANT_PATH")?,
        "publisher grant",
    )?)
    .map_err(|_| p::Error("publisher grant file is malformed".into()))?;
    publisher_grant.validate()?;
    let policy: p::CapabilityAdmissionPolicy = serde_json::from_slice(&read_file(
        &required_path("FORME_M5_ADMISSION_POLICY_PATH")?,
        "admission policy",
    )?)
    .map_err(|_| p::Error("admission policy file is malformed".into()))?;
    policy.validate()?;
    let public_key_bytes = std::fs::read(required_path("FORME_M5_PUBLISHER_PUBLIC_KEY_PATH")?)
        .map_err(|_| p::Error("publisher public key file cannot be read".into()))?;
    let public_key = PublisherPublicKey::from_bytes(
        public_key_bytes
            .try_into()
            .map_err(|_| p::Error("publisher public key must contain exactly 32 bytes".into()))?,
    );
    if public_key.digest() != publisher_grant.public_key_digest {
        return Err(p::Error(
            "publisher public key does not match its provisioned grant".into(),
        ));
    }
    let keyring = Arc::new(InMemoryPublisherKeyring::default());
    keyring.provision(publisher_grant.publisher.clone(), public_key)?;

    let package_ledger = Arc::new(FileCapabilityPackageLedger::open(required_path(
        "FORME_M5_PACKAGE_LEDGER_ROOT",
    )?)?);
    let receiver = Arc::new(CapabilityPackageReceiver::new(
        peer.clone(),
        publisher_grant,
        policy,
        authority_epoch,
        keyring,
        package_ledger,
        Arc::new(SystemRemoteClock),
    )?);
    let profile = ExecutorAdmissionProfile {
        schema_version: p::M5_SCHEMA_VERSION,
        peer: peer.clone(),
        grant: peer_grant,
        profile: p::ExecutorProfileRef(required("FORME_EXECUTOR_PROFILE")?),
        authority_epoch,
        minimum_fence: p::FenceToken(optional_u64("FORME_EXECUTOR_MINIMUM_FENCE", 1)?),
        enabled_backends: BTreeSet::from([p::BackendKind::File]),
    };
    let replay_ledger = Arc::new(FileExecutorLedger::open(required_path(
        "FORME_EXECUTOR_LEDGER_ROOT",
    )?)?);
    let service = Arc::new(
        GuardedRemoteExecutor::new(profile, receiver, Arc::new(SystemRemoteClock))?
            .with_ledger(replay_ledger),
    );
    let server = TlsRemoteExecutorServer::bind(
        TlsRemoteServerConfig {
            bind: required("FORME_EXECUTOR_BIND")?
                .parse()
                .map_err(|_| p::Error("executor bind address is invalid".into()))?,
            authority: p::AuthorityRef(required("FORME_AUTHORITY_ID")?),
            peer,
            expected_authority_identity: p::TransportIdentityDigest(required(
                "FORME_EXPECTED_AUTHORITY_IDENTITY",
            )?),
            identity: TlsIdentityFiles {
                certificate_der: required_path("FORME_EXECUTOR_CERT_DER")?,
                private_key_der: required_path("FORME_EXECUTOR_KEY_DER")?,
                trust_anchor_der: required_path("FORME_AUTHORITY_CA_DER")?,
            },
            replay_root: required_path("FORME_EXECUTOR_REPLAY_ROOT")?,
            timeout: p::DurationMs(optional_u64("FORME_EXECUTOR_TIMEOUT_MS", 10_000)?),
            max_body_bytes: usize::try_from(optional_u64(
                "FORME_EXECUTOR_MAX_BODY_BYTES",
                1_048_576,
            )?)
            .map_err(|_| p::Error("executor body limit is too large".into()))?,
        },
        service,
    )?;
    if let Ok(path) = std::env::var("FORME_EXECUTOR_READY_PATH") {
        std::fs::write(path, b"ready")
            .map_err(|_| p::Error("executor ready marker cannot be written".into()))?;
    }
    let stop = std::sync::atomic::AtomicBool::new(false);
    server.serve_until(&stop)
}

fn read_file(path: &PathBuf, label: &str) -> p::Result<Vec<u8>> {
    std::fs::read(path).map_err(|_| p::Error(format!("{label} file cannot be read")))
}

fn required(name: &str) -> p::Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| p::Error(format!("required configuration {name} is missing")))
}

fn required_path(name: &str) -> p::Result<PathBuf> {
    Ok(PathBuf::from(required(name)?))
}

fn optional_u64(name: &str, default: u64) -> p::Result<u64> {
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|_| p::Error(format!("configuration {name} is not an unsigned integer"))),
        Err(_) => Ok(default),
    }
}

fn required_u64(name: &str) -> p::Result<u64> {
    required(name)?
        .parse()
        .map_err(|_| p::Error(format!("configuration {name} is not an unsigned integer")))
}
