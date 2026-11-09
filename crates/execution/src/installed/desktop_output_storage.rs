//! A single retained staging slot. Drop releases custody, never deletes evidence.
use super::{reject_linked_ancestors, set_private_directory};
use forme_protocol as p;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub(super) struct OutputStage {
    pub(super) image: File,
    metadata: Vec<File>,
    root: PathBuf,
    // Windows denies ancestor renames while any staged bytes are in use.
    _directories: Vec<File>,
}
impl OutputStage {
    pub(super) fn available(root: &Path) -> p::Result<()> {
        reject_linked_ancestors(root)?;
        match fs::read_dir(root) {
            Ok(mut entries) => {
                if entries.next().is_none() {
                    Ok(())
                } else {
                    Err(invalid())
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(invalid()),
        }
    }
    pub(super) fn create(root: &Path, transfer: &p::DesktopOutputTransfer) -> p::Result<Self> {
        Self::available(root)?;
        fs::create_dir_all(root).map_err(|_| invalid())?;
        set_private_directory(root)?;
        let directories = hold_directories(root)?;
        Self::available(root)?;
        // Fixed create_new slot is the quota across jobs AND restarts. Any
        // incomplete attempt keeps the slot occupied until explicit recovery.
        let mut metadata = create_private(&root.join("transfer.json"))?;
        persist(&mut metadata, transfer)?;
        let image = create_private(&root.join("original.png"))?;
        Ok(Self {
            image,
            metadata: vec![metadata],
            root: root.into(),
            _directories: directories,
        })
    }
    pub(super) fn finished(&mut self, value: &impl serde::Serialize) -> p::Result<()> {
        let mut file = create_private(&self.root.join("finish.json"))?;
        persist(&mut file, value)?;
        self.metadata.push(file);
        Ok(())
    }
    pub(super) fn sealed(self) -> p::Result<()> {
        // The Host calls this only after manifest AND descriptor are durable.
        // Windows deletes the retained objects, not paths reopened after close.
        delete_owned(&self.image, &self.root.join("original.png"))?;
        for (file, name) in self.metadata.iter().zip(["transfer.json", "finish.json"]) {
            delete_owned(file, &self.root.join(name))?;
        }
        Ok(())
    }
}
fn persist(file: &mut File, value: &impl serde::Serialize) -> p::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    if bytes.len() > 16 * 1024 {
        return Err(invalid());
    }
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| invalid())
}
fn create_private(path: &Path) -> p::Result<File> {
    reject_linked_ancestors(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::{
            Foundation::{GENERIC_READ, GENERIC_WRITE},
            Storage::FileSystem::{DELETE, FILE_FLAG_OPEN_REPARSE_POINT},
        };
        options
            .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|_| invalid())?;
    if !file.metadata().map_err(|_| invalid())?.is_file() {
        return Err(invalid());
    }
    #[cfg(windows)]
    {
        if super::windows_appcontainer_acl::inspect_file(&file)?.1 != 1 {
            return Err(invalid());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if file.metadata().map_err(|_| invalid())?.nlink() != 1 {
            return Err(invalid());
        }
    }
    Ok(file)
}
#[cfg(windows)]
fn hold_directories(root: &Path) -> p::Result<Vec<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let mut paths: Vec<_> = root.ancestors().collect();
    paths.reverse();
    let mut held = Vec::new();
    for path in paths {
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|_| invalid())?;
        if !file.metadata().map_err(|_| invalid())?.is_dir() {
            return Err(invalid());
        }
        super::windows_appcontainer_acl::inspect_file(&file)?;
        held.push(file);
    }
    Ok(held)
}
#[cfg(not(windows))]
fn hold_directories(root: &Path) -> p::Result<Vec<File>> {
    reject_linked_ancestors(root)?;
    Ok(Vec::new())
}
#[cfg(windows)]
fn delete_owned(file: &File, _path: &Path) -> p::Result<()> {
    if super::windows_appcontainer_acl::inspect_file(file)?.1 != 1 {
        return Err(invalid());
    }
    super::windows_appcontainer_acl::mark_deleted(file)
}
#[cfg(unix)]
fn delete_owned(file: &File, path: &Path) -> p::Result<()> {
    use std::os::unix::fs::MetadataExt;
    reject_linked_ancestors(path)?;
    let actual = fs::symlink_metadata(path).map_err(|_| invalid())?;
    let original = file.metadata().map_err(|_| invalid())?;
    if !actual.is_file()
        || actual.dev() != original.dev()
        || actual.ino() != original.ino()
        || actual.nlink() != 1
        || original.nlink() != 1
    {
        return Err(invalid());
    }
    fs::remove_file(path).map_err(|_| invalid())
}
#[cfg(not(any(windows, unix)))]
fn delete_owned(_file: &File, _path: &Path) -> p::Result<()> {
    Err(invalid())
}
fn invalid() -> p::Error {
    p::Error("desktop output staging requires verification".into())
}
