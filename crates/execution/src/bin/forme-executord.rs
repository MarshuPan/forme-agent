#![forbid(unsafe_code)]

use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

use forme_execution::{
    ExecutorAdmissionProfile, FileExecutorLedger, GuardedRemoteExecutor, RepositoryMutationDriver,
    SystemRemoteClock, TlsIdentityFiles, TlsRemoteExecutorServer, TlsRemoteServerConfig,
};
use forme_protocol as p;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme-executord: {}", error.0);
        std::process::exit(1);
    }
}

fn run() -> p::Result<()> {
    let grant_path = required_path("FORME_EXECUTOR_GRANT_PATH")?;
    let grant_bytes = std::fs::read(&grant_path)
        .map_err(|_| p::Error("executor grant file cannot be read".into()))?;
    let grant: p::FederatedPeerGrant = serde_json::from_slice(&grant_bytes)
        .map_err(|_| p::Error("executor grant file is malformed".into()))?;
    grant.validate()?;
    let peer = p::FederatedPeerRef(required("FORME_EXECUTOR_PEER")?);
    if peer != grant.peer {
        return Err(p::Error("executor peer does not match its grant".into()));
    }
    let driver = Arc::new(RepositoryMutationDriver::new(
        required("FORME_EXECUTOR_LOGICAL_PATH")?,
        required_path("FORME_EXECUTOR_STATE_PATH")?,
    )?);
    let profile = ExecutorAdmissionProfile {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: peer.clone(),
        grant: grant.clone(),
        profile: p::ExecutorProfileRef(required("FORME_EXECUTOR_PROFILE")?),
        authority_epoch: p::AuthorityEpoch(required_u64("FORME_AUTHORITY_EPOCH")?),
        minimum_fence: p::FenceToken(optional_u64("FORME_EXECUTOR_MINIMUM_FENCE", 1)?),
        enabled_backends: BTreeSet::from([p::BackendKind::File]),
    };
    let ledger = Arc::new(FileExecutorLedger::open(required_path(
        "FORME_EXECUTOR_LEDGER_ROOT",
    )?)?);
    let service = Arc::new(
        GuardedRemoteExecutor::new(profile, driver, Arc::new(SystemRemoteClock))?
            .with_ledger(ledger),
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
