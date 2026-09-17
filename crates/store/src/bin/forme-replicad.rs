#![forbid(unsafe_code)]

use std::{io::Read, path::PathBuf};

use forme_protocol as p;
use forme_store::{ReplicaProjectionStore, ReplicaSqliteStore};

const MAXIMUM_BATCH_BYTES: u64 = 1_048_576;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme-replicad: {}", error.0);
        std::process::exit(1);
    }
}

fn run() -> p::Result<()> {
    let peer = p::FederatedPeerRef(required("FORME_REPLICA_PEER")?);
    let grant = p::FederatedPeerGrantRef(required("FORME_REPLICA_GRANT")?);
    let epoch = p::AuthorityEpoch(required_u64("FORME_AUTHORITY_EPOCH")?);
    let scope = p::ReplicaScope {
        schema_version: p::M4_SCHEMA_VERSION,
        scopes: parse_scopes(&required("FORME_REPLICA_SCOPES")?)?,
        allow_owner_view: optional_bool("FORME_REPLICA_ALLOW_OWNER_VIEW", false)?,
    };
    scope.validate()?;
    let store = ReplicaSqliteStore::open(
        required_path("FORME_REPLICA_DB_PATH")?,
        peer.clone(),
        grant,
        epoch,
        scope,
    )?;

    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAXIMUM_BATCH_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| p::Error("replication batch cannot be read".into()))?;
    if bytes.is_empty() || bytes.len() as u64 > MAXIMUM_BATCH_BYTES {
        return Err(p::Error(
            "replication batch body is empty or oversized".into(),
        ));
    }
    let batch: p::ReplicationBatch = serde_json::from_slice(&bytes)
        .map_err(|_| p::Error("replication batch is malformed".into()))?;
    let expected = store.cursor(&peer, &batch.aggregate)?;
    let report = store.apply(batch, expected)?;
    let output = serde_json::to_string(&report)
        .map_err(|_| p::Error("replica apply report cannot be encoded".into()))?;
    println!("{output}");
    Ok(())
}

fn parse_scopes(value: &str) -> p::Result<Vec<p::Scope>> {
    let mut scopes = value
        .split(',')
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(|scope| p::Scope(scope.to_owned()))
        .collect::<Vec<_>>();
    scopes.sort();
    scopes.dedup();
    if scopes.is_empty() {
        return Err(p::Error("replica scope list is empty".into()));
    }
    Ok(scopes)
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

fn required_u64(name: &str) -> p::Result<u64> {
    required(name)?
        .parse()
        .map_err(|_| p::Error(format!("configuration {name} is not an unsigned integer")))
}

fn optional_bool(name: &str, default: bool) -> p::Result<bool> {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Err(p::Error(format!("configuration {name} is not a boolean"))),
        },
        Err(_) => Ok(default),
    }
}
