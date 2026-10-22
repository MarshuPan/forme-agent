//! Preserve the installer-selected read-only service group across new inodes.
//! Private/development parents never opt into shared access implicitly.
use super::storage::{self, invalid};
use forme_protocol as p;
use std::{fs::File, path::Path};

pub(super) fn directory(path: &Path) -> p::Result<()> {
    let parent = path.parent().ok_or_else(invalid)?;
    storage::directory(parent)?;
    let group = reader_group(parent)?;
    storage::directory(path)?;
    #[cfg(unix)]
    if let Some(group) = group {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = std::fs::metadata(path).map_err(|_| invalid())?;
        if meta.gid() != group {
            return Err(invalid());
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o2750))
            .map_err(|_| invalid())?;
        storage::sync_directory(path)?;
        storage::sync_directory(parent)?;
    }
    #[cfg(not(unix))]
    let _ = group;
    Ok(())
}

pub(super) fn immutable(path: &Path, bytes: &[u8]) -> p::Result<()> {
    reader_group(path.parent().ok_or_else(invalid)?)?;
    storage::immutable(path, bytes)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| invalid())?;
    prepare_file(path.parent().ok_or_else(invalid)?, &file)?;
    file.sync_all().map_err(|_| invalid())
}

pub(super) fn prepare_file(parent: &Path, file: &File) -> p::Result<()> {
    let group = reader_group(parent)?;
    #[cfg(unix)]
    if let Some(group) = group {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = file.metadata().map_err(|_| invalid())?;
        if !meta.is_file() || meta.gid() != group || meta.nlink() != 1 {
            return Err(invalid());
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o640))
            .map_err(|_| invalid())?;
    }
    #[cfg(not(unix))]
    let _ = (group, file);
    Ok(())
}

fn reader_group(parent: &Path) -> p::Result<Option<u32>> {
    storage::check(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = std::fs::metadata(parent).map_err(|_| invalid())?;
        if !meta.is_dir() {
            return Err(invalid());
        }
        let mode = meta.permissions().mode() & 0o7777;
        if mode & 0o2000 == 0 {
            return Ok(None);
        }
        if mode != 0o2750 {
            return Err(invalid());
        }
        Ok(Some(meta.gid()))
    }
    #[cfg(not(unix))]
    Ok(None)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[test]
    fn private_parents_do_not_grant_a_service_read_access() {
        let root = tempfile::tempdir().unwrap();
        directory(&root.path().join("revision")).unwrap();
        let path = root.path().join("revision/key");
        immutable(&path, b"private-fixture").unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn revisions_and_atomic_replacements_keep_the_preselected_reader_group() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o2750)).unwrap();
        let revision = root.path().join("revision");
        directory(&revision).unwrap();
        assert_eq!(
            std::fs::metadata(&revision).unwrap().permissions().mode() & 0o7777,
            0o2750
        );
        let key = revision.join("model.secret");
        immutable(&key, b"original-fixture").unwrap();
        let published = root.path().join("core.json");
        for value in [b"revision-one".as_slice(), b"revision-two".as_slice()] {
            storage::publish(&published, value).unwrap();
            let meta = std::fs::metadata(&published).unwrap();
            assert_eq!(meta.gid(), std::fs::metadata(root.path()).unwrap().gid());
            assert_eq!(meta.permissions().mode() & 0o7777, 0o640);
            assert_eq!(std::fs::read(&published).unwrap(), value);
        }
        assert_eq!(
            std::fs::metadata(key).unwrap().permissions().mode() & 0o7777,
            0o640
        );
    }

    #[test]
    fn a_writable_or_public_setgid_parent_cannot_publish_service_credentials() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o2770)).unwrap();
        assert!(directory(&root.path().join("revision")).is_err());
        assert!(!root.path().join("revision").exists());
        assert!(storage::publish(&root.path().join("core.json"), b"fixture").is_err());
        assert!(!root.path().join("core.json").exists());
    }
}
