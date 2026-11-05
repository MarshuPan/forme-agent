//! Bounded source-tree custody and create-new copies, never a merge or cleanup.
use super::invalid;
use forme_harness::offline_data::BootstrapSourceCustody;
use forme_protocol as p;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(super) struct BootstrapTree {
    root: PathBuf,
    keys: bool,
    records: Vec<(PathBuf, Option<p::SchemaDigest>)>,
    custody: BootstrapSourceCustody,
}

impl BootstrapTree {
    pub fn open(root: &Path, keys: bool) -> p::Result<Self> {
        let mut custody = BootstrapSourceCustody::default();
        let mut pending = vec![(root.to_path_buf(), 0)];
        let mut records = Vec::new();
        let mut total = 0_u64;
        let limit = if keys {
            16 * 1024 * 1024
        } else {
            1024 * 1024 * 1024
        };
        while let Some((directory, depth)) = pending.pop() {
            if depth > 8 {
                return Err(invalid());
            }
            super::super::web::config::check_path(&directory)?;
            for entry in fs::read_dir(&directory).map_err(|_| invalid())? {
                if records.len() >= 4096 {
                    return Err(invalid());
                }
                let path = entry.map_err(|_| invalid())?.path();
                super::super::web::config::check_path(&path)?;
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| invalid())?
                    .to_path_buf();
                if path.is_dir() {
                    pending.push((path, depth + 1));
                    records.push((relative, None));
                } else {
                    let bytes = zeroize::Zeroizing::new(
                        custody.read(&path, if keys { 1024 * 1024 } else { 64 * 1024 * 1024 })?,
                    );
                    total = total.checked_add(bytes.len() as u64).ok_or_else(invalid)?;
                    if total > limit {
                        return Err(invalid());
                    }
                    records.push((relative, Some(p::sha256_content_digest(&bytes))));
                }
            }
        }
        records.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Self {
            root: root.to_path_buf(),
            keys,
            records,
            custody,
        })
    }

    pub fn digest(&self) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&("forme-bootstrap-tree-v2", self.keys, &self.records))
    }

    pub fn recheck(&self) -> p::Result<()> {
        self.custody.recheck()?;
        if Self::open(&self.root, self.keys)?.records != self.records {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn copy_into_empty(&self, target: &Path) -> p::Result<()> {
        self.recheck()?;
        super::super::web::config::check_path(target)?;
        if target.exists() {
            if !target.is_dir()
                || fs::read_dir(target)
                    .map_err(|_| invalid())?
                    .next()
                    .is_some()
            {
                return Err(invalid());
            }
        } else {
            directory(target)?;
        }
        let mut directories: Vec<_> = self
            .records
            .iter()
            .filter(|(_, digest)| digest.is_none())
            .map(|(path, _)| path)
            .collect();
        directories.sort_by_key(|path| path.components().count());
        for relative in directories {
            directory(&target.join(relative))?;
        }
        for (relative, digest) in &self.records {
            let Some(digest) = digest else { continue };
            let mut custody = BootstrapSourceCustody::default();
            let bytes = zeroize::Zeroizing::new(custody.read(
                &self.root.join(relative),
                if self.keys {
                    1024 * 1024
                } else {
                    64 * 1024 * 1024
                },
            )?);
            if p::sha256_content_digest(&bytes) != *digest {
                return Err(invalid());
            }
            immutable(&target.join(relative), &bytes)?;
            custody.recheck()?;
        }
        self.recheck()?;
        let copied = Self::open(target, self.keys)?;
        if copied.records != self.records {
            return Err(invalid());
        }
        sync_directory(target)
    }
}

pub(super) fn directory(path: &Path) -> p::Result<()> {
    super::super::web::config::check_path(path)?;
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder.create(path).map_err(|_| invalid())?;
    sync_directory(path.parent().ok_or_else(invalid)?)
}

pub(super) fn immutable(path: &Path, bytes: &[u8]) -> p::Result<()> {
    super::super::web::config::check_path(path)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| invalid())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| invalid())?;
    sync_directory(path.parent().ok_or_else(invalid)?)
}

pub(super) fn sync_directory(path: &Path) -> p::Result<()> {
    #[cfg(unix)]
    fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| invalid())?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
