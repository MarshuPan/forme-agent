//! Disposable files and actual OS locks; no owner vault or service installation.
#![forbid(unsafe_code)]
use forme_protocol as p;
use forme_store::{
    ContentKeyResolver, FileContentKeyResolver, SqliteEventStore, StoreAccessGuard, StoreOptions,
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "forme-installed-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}
fn binding() -> p::ContentBinding {
    p::ContentBinding {
        schema_version: p::SchemaVersion(1),
        owner_digest: p::sha256_content_digest(b"owner:fixture"),
        scope: p::Scope("workspace:fixture".into()),
        object: p::ObjectRef("fixture:content".into()),
    }
}

#[test]
fn managed_vault_does_not_need_an_interactive_username() {
    if let Some(path) = std::env::var_os("FORME_KEY_SERVICE_FIXTURE") {
        assert!(std::env::var_os("USERNAME").is_none());
        let keys = FileContentKeyResolver::new_managed(PathBuf::from(path)).unwrap();
        let digest = keys
            .subject_key(&binding(), &p::sha256_content_digest(b"subject"))
            .unwrap();
        assert_eq!(keys.resolve(&digest).unwrap().len(), 32);
        return;
    }
    let root = root("service-keys");
    let result = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "managed_vault_does_not_need_an_interactive_username",
        ])
        .env("FORME_KEY_SERVICE_FIXTURE", root.join("keys"))
        .env_remove("USERNAME")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn readonly_resolver_never_allocates_destroys_or_creates_missing_material() {
    let root = root("readonly-keys");
    let path = root.join("keys");
    assert!(FileContentKeyResolver::open_existing(&path).is_err());
    assert!(!path.exists());
    let keys = FileContentKeyResolver::new_managed(&path).unwrap();
    let subject = p::sha256_content_digest(b"subject");
    let digest = keys.subject_key(&binding(), &subject).unwrap();
    let original = keys.resolve(&digest).unwrap();
    let readonly = FileContentKeyResolver::open_existing(&path).unwrap();
    assert_eq!(readonly.resolve(&digest).unwrap(), original);
    assert!(readonly.subject_key(&binding(), &subject).is_err());
    assert!(readonly.destroy(&digest).is_err());
    assert_eq!(keys.resolve(&digest).unwrap(), original);
    keys.destroy(&digest).unwrap();
    assert!(readonly.resolve(&digest).is_err());
    assert!(keys.subject_key(&binding(), &subject).is_err());
    assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn key_replacement_and_incomplete_files_fail_closed_without_repair() {
    let root = root("corrupt-keys");
    let path = root.join("keys");
    let keys = FileContentKeyResolver::new_managed(&path).unwrap();
    let digest = keys
        .subject_key(&binding(), &p::sha256_content_digest(b"subject"))
        .unwrap();
    let file = fs::read_dir(&path).unwrap().next().unwrap().unwrap().path();
    fs::write(&file, [9; 32]).unwrap();
    assert!(keys.resolve(&digest).is_err());
    fs::write(&file, [9; 31]).unwrap();
    assert!(FileContentKeyResolver::open_existing(&path).is_err());
    assert!(FileContentKeyResolver::new_managed(&path).is_err());
    assert_eq!(fs::read(&file).unwrap().len(), 31);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn managed_and_readonly_vaults_refuse_links_and_public_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = root("linked-keys");
    let real = root.join("real");
    fs::create_dir(&real).unwrap();
    symlink(&real, root.join("alias")).unwrap();
    assert!(FileContentKeyResolver::new_managed(root.join("alias")).is_err());
    fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(FileContentKeyResolver::open_existing(&real).is_err());
    assert!(FileContentKeyResolver::new_managed(&real).is_err());
    assert_eq!(
        fs::metadata(&real).unwrap().permissions().mode() & 0o777,
        0o755
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn os_access_lock_excludes_online_and_offline_across_processes() {
    if let Some(path) = std::env::var_os("FORME_ACCESS_FIXTURE") {
        let path = PathBuf::from(path);
        match std::env::var("FORME_ACCESS_MODE").unwrap().as_str() {
            "blocked-offline" => assert!(StoreAccessGuard::offline(&path).is_err()),
            "blocked-online" => assert!(StoreAccessGuard::online(&path).is_err()),
            "shared-online" => {
                let _guard = StoreAccessGuard::online(&path).unwrap();
            }
            "exclusive-offline" => {
                let _guard = StoreAccessGuard::offline(&path).unwrap();
            }
            _ => panic!("invalid fixture mode"),
        }
        return;
    }
    let root = root("access-lock");
    let database = root.join("brain.db");
    fs::write(&database, b"unchanged fixture database bytes").unwrap();
    let child = |mode| {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "os_access_lock_excludes_online_and_offline_across_processes",
            ])
            .env("FORME_ACCESS_FIXTURE", &database)
            .env("FORME_ACCESS_MODE", mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    };
    let online = StoreAccessGuard::online(&database).unwrap();
    child("shared-online");
    child("blocked-offline");
    drop(online);
    let offline = StoreAccessGuard::offline(&database).unwrap();
    child("blocked-online");
    child("blocked-offline");
    drop(offline);
    child("exclusive-offline");
    assert_eq!(
        fs::read(&database).unwrap(),
        b"unchanged fixture database bytes"
    );
    assert!(database.with_extension("access-lock").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn access_guard_binds_exact_store_and_never_creates_a_missing_offline_brain() {
    let root = root("access-binding");
    let path = root.join("brain.db");
    assert!(StoreAccessGuard::offline(&path).is_err());
    assert!(!path.with_extension("access-lock").exists());
    assert!(!path.exists());
    let store = SqliteEventStore::open(&path, StoreOptions::default()).unwrap();
    let access = StoreAccessGuard::online(&path).unwrap();
    access.require_store(&store).unwrap();
    let other = SqliteEventStore::open(root.join("other.db"), StoreOptions::default()).unwrap();
    assert!(access.require_store(&other).is_err());
    assert!(access
        .require_store(&SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap())
        .is_err());
    drop((access, store, other));
    fs::remove_dir_all(root).unwrap();
}
