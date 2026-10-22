//! Shared cold-source guard for explicit migration, recovery and inspection.
use crate::StoreAccessGuard;
use forme_protocol as p;
use rusqlite::{Connection, OpenFlags};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

pub(crate) fn require_cold_source(path: &Path) -> p::Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        match fs::symlink_metadata(PathBuf::from(sidecar)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => {
                return Err(failure(
                    "database must be offline and checkpointed without SQLite sidecars",
                ))
            }
        }
    }
    Ok(())
}

// Only safe after the exclusive OS guard and cold-sidecar checks. This avoids
// even transient WAL/SHM creation or recovery of the inspected artifact.
pub(crate) fn immutable_source(path: &Path) -> p::Result<Connection> {
    let raw = path
        .to_str()
        .ok_or_else(|| failure("source path encoding unsupported"))?;
    let raw = raw.strip_prefix(r"\\?\").unwrap_or(raw).replace('\\', "/");
    let mut uri = String::from("file:");
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || b"/:_-.".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|_| failure("offline source cannot be opened read-only"))
}

pub(crate) struct OfflineSource {
    _file: File,
    _access: Option<StoreAccessGuard>,
    #[cfg(not(windows))]
    writer_reservation: Option<Connection>,
}
impl OfflineSource {
    pub(crate) fn acquire(path: &Path) -> p::Result<Self> {
        Self::acquire_with_fence(path, Some(StoreAccessGuard::offline(path)?))
    }

    pub(crate) fn acquire_legacy(path: &Path) -> p::Result<Self> {
        Self::acquire_with_fence(path, StoreAccessGuard::existing_offline_fence(path)?)
    }

    fn acquire_with_fence(path: &Path, access: Option<StoreAccessGuard>) -> p::Result<Self> {
        require_cold_source(path)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Deny existing/new writable and deleting handles for the interval.
            options.share_mode(1);
        }
        let file = options
            .open(path)
            .map_err(|_| failure("source is not offline or cannot be locked"))?;
        #[cfg(not(windows))]
        {
            #[cfg(unix)]
            if access.is_none() {
                file.try_lock()
                    .map_err(|_| failure("source has an online or offline holder"))?;
            }
            use std::io::Read;
            let mut header = [0_u8; 20];
            (&file)
                .read_exact(&mut header)
                .map_err(|_| failure("source header is unavailable"))?;
            // BEGIN IMMEDIATE on a cold WAL database creates WAL/SHM. Never
            // mutate the inspected source just to establish an advisory check.
            // WAL sources are fenced by StoreAccessGuard and cold-sidecar
            // checks; historical binaries without that fence must stay stopped.
            let reservation = if header[18] == 2 || header[19] == 2 {
                None
            } else {
                let reservation =
                    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
                        .map_err(|_| failure("offline source writer reservation failed"))?;
                reservation
                    .busy_timeout(std::time::Duration::ZERO)
                    .map_err(|_| failure("source lock setup failed"))?;
                reservation
                    .execute_batch("BEGIN IMMEDIATE")
                    .map_err(|_| failure("source has an active writer"))?;
                Some(reservation)
            };
            Ok(Self {
                _file: file,
                _access: access,
                writer_reservation: reservation,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                _file: file,
                _access: access,
            })
        }
    }
    pub(crate) fn release(self) -> p::Result<()> {
        #[cfg(not(windows))]
        {
            if let Some(reservation) = self.writer_reservation {
                reservation
                    .execute_batch("ROLLBACK")
                    .map_err(|_| failure("source writer reservation release failed"))?;
                reservation
                    .close()
                    .map_err(|_| failure("source writer reservation close failed"))?;
            }
        }
        Ok(())
    }
}

fn failure(reason: &str) -> p::Error {
    p::Error(format!("offline brain access: {reason}"))
}
