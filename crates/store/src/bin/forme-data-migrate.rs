//! Offline local-owner migration: stop the authority daemon before use.
//!
//! `plan <source> <new-target>` emits an immutable plan without creating a
//! database or key vault. `apply <source> <new-target> <approved-digest>` reads
//! that plan from stdin (at most 256 KiB) and emits only a safe JSON receipt.
//! Host OS access and FORME_OWNER_ID bound to source brain_authority are the
//! authentication boundary; stdin cannot supply the owner. FORME_WORKSPACE
//! defaults to workspace:default. Keys use target.with_extension("content-keys").
//! The source stays unchanged and UNPROTECTED; authority is never switched.
//! Failed targets are retained and may contain plaintext. Both databases and
//! the local key vault must continue to be treated as sensitive owner data.
#![forbid(unsafe_code)]

use forme_protocol as p;
use forme_store::{
    ContentKeyResolver, FileContentKeyResolver, LegacyMigrationPlan, LegacyPlaintextMigration,
    StoreContentProtection,
};
use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

const MAX_PLAN_BYTES: u64 = 256 * 1024;
const FAILURE: &[u8] = b"forme-data-migrate: migration not completed; check owner, exact approval, offline source and NEW target. Stop the authority daemon. Source and any incomplete target/key vault remain sensitive; source is unprotected and authority is unchanged.\n";

// Planning and approval preflight must never allocate, read or destroy keys.
struct NoAllocationKeys;
impl ContentKeyResolver for NoAllocationKeys {
    fn resolve(&self, _: &p::SchemaDigest) -> p::Result<Vec<u8>> {
        Err(p::Error("migration planning cannot access keys".into()))
    }

    fn destroy(&self, _: &p::SchemaDigest) -> p::Result<()> {
        Err(p::Error("migration planning cannot access keys".into()))
    }
}

fn diagnostic() {
    let _ = io::stderr().lock().write_all(FAILURE);
}

fn main() -> ExitCode {
    // Even an unexpected lower-level panic must not disclose owner paths/data.
    std::panic::set_hook(Box::new(|_| diagnostic()));
    match run().and_then(|mut output| {
        output.push(b'\n');
        io::stdout().lock().write_all(&output).map_err(|_| ())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => {
            diagnostic();
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<Vec<u8>, ()> {
    let mut args = env::args_os().skip(1);
    let operation = args.next().ok_or(())?;
    let source = PathBuf::from(args.next().ok_or(())?);
    let target = PathBuf::from(args.next().ok_or(())?);
    let approved = if operation == "plan" {
        None
    } else if operation == "apply" {
        Some(p::SchemaDigest(
            args.next().ok_or(())?.into_string().map_err(|_| ())?,
        ))
    } else {
        return Err(());
    };
    if args.next().is_some() {
        return Err(());
    }

    let owner = p::VerifiedPrincipal(env::var("FORME_OWNER_ID").map_err(|_| ())?);
    if owner.0.trim().is_empty() {
        return Err(());
    }
    let scope = p::Scope(match env::var("FORME_WORKSPACE") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => "workspace:default".into(),
        Err(_) => return Err(()),
    });
    let (target, key_vault) = new_target_paths(&target)?;
    let preview = LegacyPlaintextMigration::new(
        StoreContentProtection::new(&owner, scope.clone(), Arc::new(NoAllocationKeys))
            .map_err(|_| ())?,
    );

    let Some(approved) = approved else {
        let plan = preview.plan(&owner, &source, &target).map_err(|_| ())?;
        return serde_json::to_vec(&plan).map_err(|_| ());
    };
    let plan = read_plan()?;
    plan.validate().map_err(|_| ())?;
    p::sha256_digest_bytes(&approved).map_err(|_| ())?;
    if approved != plan.digest {
        return Err(());
    }

    // Reinspect the real source and every binding before creating a key vault.
    // Only expiry differs between repeated previews; validate the approved
    // expiry above, then compare all other fields including source bytes.
    let mut current = preview.plan(&owner, &source, &target).map_err(|_| ())?;
    current.expires_at = plan.expires_at;
    current.digest = plan.digest.clone();
    if current != plan {
        return Err(());
    }

    // create_dir is an exclusive reservation: never reuse another key vault,
    // including an empty directory or a symlink. Retain it on any later error.
    fs::create_dir(&key_vault).map_err(|_| ())?;
    let keys = Arc::new(FileContentKeyResolver::new(&key_vault).map_err(|_| ())?);
    let migration = LegacyPlaintextMigration::new(
        StoreContentProtection::new(&owner, scope, keys).map_err(|_| ())?,
    );
    let receipt = migration
        .execute(&owner, &source, &target, &plan, &approved)
        .map_err(|_| ())?;
    serde_json::to_vec(&receipt).map_err(|_| ())
}

fn read_plan() -> Result<LegacyMigrationPlan, ()> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_PLAN_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_PLAN_BYTES {
        return Err(());
    }
    // The API DTO rejects unknown fields; do not accept an owner-bearing wrapper.
    serde_json::from_slice(&bytes).map_err(|_| ())
}

fn new_target_paths(target: &Path) -> Result<(PathBuf, PathBuf), ()> {
    let name = target.file_name().ok_or(())?;
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|_| ())?;
    let target = parent.join(name);
    // Reject vault/database aliasing, including Windows case-insensitive names.
    if target.extension().is_some_and(|extension| {
        extension
            .to_str()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("content-keys"))
    }) {
        return Err(());
    }
    let key_vault = target.with_extension("content-keys");
    require_missing(&target)?;
    require_missing(&key_vault)?;
    Ok((target, key_vault))
}

fn require_missing(path: &Path) -> Result<(), ()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(()),
    }
}
