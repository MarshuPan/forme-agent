//! OS lifetime fence between production runtime and explicit offline maintenance.
#![forbid(unsafe_code)]
use crate::SqliteEventStore;
use forme_protocol as p;
use std::{
    fs::{self, File, OpenOptions},
    path::{Component, Path, PathBuf},
};

/// Online holders share this guard; the existing authority lease still elects
/// the only writer. Offline maintenance must take an exclusive guard instead.
/// The lock file is permanent: never unlink it when a process stops.
pub struct StoreAccessGuard {
    _file: File,
    #[cfg(unix)]
    _database_file: Option<File>,
    database: PathBuf,
}

impl StoreAccessGuard {
    /// Acquire before opening/initializing a production store. The parent must
    /// already exist and be protected by the installation or local owner.
    pub fn online(database: &Path) -> p::Result<Self> {
        Self::acquire(database, false, false, true)
    }

    /// Existing database only; refuses an online holder before any brain/key IO.
    /// This fences participating runtimes, not historical binaries lacking it.
    pub fn offline(database: &Path) -> p::Result<Self> {
        Self::acquire(database, true, false, true)
    }

    // Legacy planning must not create files. Participating online runtimes
    // also lock an existing database on Unix; Windows OfflineSource denies
    // writable handles. This covers the race where the fence is first created.
    pub(crate) fn existing_offline_fence(database: &Path) -> p::Result<Option<Self>> {
        let database = normalized(database)?;
        match fs::symlink_metadata(database.with_extension("access-lock")) {
            Ok(_) => Self::acquire(&database, true, false, false).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(invalid()),
        }
    }

    pub(crate) fn new_offline_target(database: &Path) -> p::Result<Self> {
        Self::acquire(database, true, true, true)
    }

    fn acquire(
        database: &Path,
        exclusive: bool,
        new_target: bool,
        create_fence: bool,
    ) -> p::Result<Self> {
        let database = normalized(database)?;
        if (exclusive && !new_target && !database.is_file())
            || (new_target && fs::symlink_metadata(&database).is_ok())
        {
            return Err(invalid());
        }
        let path = database.with_extension("access-lock");
        reject_indirection(&path)?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(create_fence)
            .truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|_| invalid())?;
        if !file.metadata().map_err(|_| invalid())?.is_file() {
            return Err(invalid());
        }
        if exclusive {
            file.try_lock().map_err(|_| busy())?;
        } else {
            file.try_lock_shared().map_err(|_| busy())?;
        }
        reject_indirection(&path)?;
        #[cfg(unix)]
        let database_file = match File::open(&database) {
            Ok(file) => {
                if exclusive {
                    file.try_lock().map_err(|_| busy())?;
                } else {
                    file.try_lock_shared().map_err(|_| busy())?;
                }
                Some(file)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !exclusive => None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && new_target => None,
            Err(_) => return Err(invalid()),
        };
        Ok(Self {
            _file: file,
            #[cfg(unix)]
            _database_file: database_file,
            database,
        })
    }

    /// A caller supplying an already opened store may not lock another path.
    /// No write, migration or authority claim is performed by this check.
    pub fn require_store(&self, store: &SqliteEventStore) -> p::Result<()> {
        let connection = store.core_connection()?;
        let path = connection
            .path()
            .filter(|path| !path.is_empty())
            .ok_or_else(|| {
                p::Error(
                    "the production brain requires a durable file-backed authority store".into(),
                )
            })?;
        if normalized(Path::new(path))? != self.database {
            return Err(invalid());
        }
        Ok(())
    }
}

pub(crate) fn normalized(path: &Path) -> p::Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_err(|_| invalid())?.join(path)
    };
    reject_indirection(&path)?;
    let parent = path
        .parent()
        .ok_or_else(invalid)?
        .canonicalize()
        .map_err(|_| invalid())?;
    let name = path.file_name().ok_or_else(invalid)?;
    if name.to_string_lossy().contains(':') {
        return Err(invalid());
    }
    Ok(parent.join(name))
}

fn reject_indirection(path: &Path) -> p::Result<()> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(invalid());
                    }
                }
                if meta.file_type().is_symlink() {
                    return Err(invalid());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid()),
        }
    }
    Ok(())
}
fn invalid() -> p::Error {
    p::Error("brain maintenance path or access guard is invalid".into())
}
fn busy() -> p::Error {
    p::Error("brain access conflicts with an online or offline holder".into())
}
