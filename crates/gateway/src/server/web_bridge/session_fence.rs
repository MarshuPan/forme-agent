//! Durable product-authentication fences and one-shot request proofs.
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::Mutex;

use forme_protocol as p;
use rusqlite::{params, Connection, OpenFlags};

const APPLICATION_ID: i64 = 0x46574231;
const MAX_LIVE_NONCES: i64 = 32_768;

#[cfg(test)]
mod tests;

pub(super) struct SessionFence {
    db: Mutex<Connection>,
    _lock: File,
    install: String,
    owner: p::VerifiedPrincipal,
}

fn unavailable(_: impl std::fmt::Debug) -> p::Error {
    p::Error("Web session fence requires recovery".into())
}

impl SessionFence {
    pub(super) fn open(
        path: &Path,
        install: &str,
        owner: &p::VerifiedPrincipal,
    ) -> p::Result<Self> {
        check_path(path)?;
        let parent = path
            .parent()
            .filter(|p| p.parent().is_some())
            .ok_or_else(|| unavailable(()))?;
        std::fs::create_dir_all(parent).map_err(unavailable)?;
        let lock_path = path.with_extension("auth-lock");
        let sidecar = |suffix: &str| {
            let mut value = path.as_os_str().to_owned();
            value.push(suffix);
            std::path::PathBuf::from(value)
        };
        for file in [
            lock_path.clone(),
            sidecar("-wal"),
            sidecar("-shm"),
            sidecar("-journal"),
        ] {
            check_path(&file)?;
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(unavailable)?;
        lock.try_lock().map_err(unavailable)?;
        let existed = path.exists();
        if existed {
            let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(unavailable)?;
            let app: i64 = db
                .pragma_query_value(None, "application_id", |r| r.get(0))
                .map_err(unavailable)?;
            let version: i64 = db
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .map_err(unavailable)?;
            if app != APPLICATION_ID || version != 1 {
                return Err(unavailable(()));
            }
        } else {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .and_then(|file| file.sync_all())
                .map_err(unavailable)?;
        }
        let mut db = Connection::open(path).map_err(unavailable)?;
        db.busy_timeout(std::time::Duration::from_secs(2))
            .map_err(unavailable)?;
        db.pragma_update(None, "synchronous", "FULL")
            .map_err(unavailable)?;
        if !existed {
            let tx = db.transaction().map_err(unavailable)?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(unavailable)?;
            tx.pragma_update(None, "user_version", 1)
                .map_err(unavailable)?;
            tx.execute_batch("CREATE TABLE binding (id INTEGER PRIMARY KEY CHECK(id=1), install TEXT NOT NULL, owner TEXT NOT NULL);
                CREATE TABLE epochs (session TEXT PRIMARY KEY, epoch INTEGER NOT NULL CHECK(epoch>0));
                CREATE TABLE nonces (nonce TEXT PRIMARY KEY, expires INTEGER NOT NULL);
                CREATE INDEX nonce_expiry ON nonces(expires);").map_err(unavailable)?;
            tx.execute(
                "INSERT INTO binding VALUES(1,?1,?2)",
                params![install, owner.0],
            )
            .map_err(unavailable)?;
            tx.commit().map_err(unavailable)?;
        }
        let identity: (String, String) = db
            .query_row("SELECT install,owner FROM binding WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(unavailable)?;
        let integrity: String = db
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(unavailable)?;
        if identity != (install.to_owned(), owner.0.clone()) || integrity != "ok" {
            return Err(unavailable(()));
        }
        Ok(Self {
            db: Mutex::new(db),
            _lock: lock,
            install: install.into(),
            owner: owner.clone(),
        })
    }

    /// A nonce is committed before invoking any core handler. Reuse is rejected
    /// even after restart; retrying a command requires a new signed request and
    /// the original brain command idempotency key.
    pub(super) fn admit(
        &self,
        assertion: &p::CoreSessionAssertion,
        now: i64,
        fence_only: bool,
    ) -> p::Result<()> {
        assertion.validate()?;
        if assertion.install_ref != self.install
            || assertion.owner.0 != self.owner.0
            || assertion.expires_at <= now
            || assertion.expires_at > now.saturating_add(30_000)
        {
            return Err(p::Error("Web session assertion is not current".into()));
        }
        let mut db = self.db.lock().map_err(unavailable)?;
        let tx = db.transaction().map_err(unavailable)?;
        let minimum: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(epoch),0) FROM epochs WHERE session='*' OR session=?1",
                [&assertion.session_digest.0],
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        let epoch = i64::try_from(assertion.auth_epoch).map_err(unavailable)?;
        if epoch < minimum && !fence_only {
            return Err(p::Error("Web session is revoked".into()));
        }
        tx.execute("DELETE FROM nonces WHERE expires<=?1", [now])
            .map_err(unavailable)?;
        let count: i64 = tx
            .query_row("SELECT COUNT(*) FROM nonces", [], |r| r.get(0))
            .map_err(unavailable)?;
        if count >= MAX_LIVE_NONCES {
            return Err(p::Error("Web request proof capacity exceeded".into()));
        }
        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO nonces VALUES(?1,?2)",
                params![assertion.nonce, assertion.expires_at],
            )
            .map_err(unavailable)?;
        if inserted != 1 {
            return Err(p::Error("Web request proof was already consumed".into()));
        }
        tx.commit().map_err(unavailable)
    }

    pub(super) fn apply(&self, update: &p::WebSessionFenceUpdate) -> p::Result<()> {
        update.validate()?;
        if update.install_ref != self.install || update.owner.0 != self.owner.0 {
            return Err(unavailable(()));
        }
        let epoch = i64::try_from(update.minimum_auth_epoch).map_err(unavailable)?;
        let mut db = self.db.lock().map_err(unavailable)?;
        let tx = db.transaction().map_err(unavailable)?;
        let session = update
            .session_digest
            .as_ref()
            .map(|v| v.0.as_str())
            .unwrap_or("*");
        tx.execute("INSERT INTO epochs VALUES(?1,?2) ON CONFLICT(session) DO UPDATE SET epoch=MAX(epoch,excluded.epoch)", params![session, epoch]).map_err(unavailable)?;
        tx.commit().map_err(unavailable)
    }
}

fn check_path(path: &Path) -> p::Result<()> {
    if !path.is_absolute()
        || path.components().any(|p| {
            matches!(
                p,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(unavailable(()));
    }
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(unavailable(()));
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(unavailable(()));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(unavailable(error)),
        }
    }
    Ok(())
}
