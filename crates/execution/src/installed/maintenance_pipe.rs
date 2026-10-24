//! Bounded, supervised exchange with a fixed installed maintenance program.
use forme_protocol as p;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) struct ReleaseDataProgram {
    pub(super) installation: p::InstallationManifest,
    manifest_path: PathBuf,
    program: PathBuf,
    verified: Option<super::VerifiedStagedRelease>,
}

impl ReleaseDataProgram {
    pub(super) fn new(installation: p::InstallationManifest) -> p::Result<Self> {
        super::validate_installation_paths(&installation)?;
        let name = if installation.target == p::ProductTarget::WindowsX86_64 {
            "forme-hostctl.exe"
        } else {
            "forme-hostctl"
        };
        Ok(Self {
            manifest_path: installation.paths.config.join("installation.json"),
            program: installation.paths.releases.join("bin").join(name),
            installation,
            verified: None,
        })
    }

    pub(super) fn from_verified(
        installation: p::InstallationManifest,
        verified: super::VerifiedStagedRelease,
    ) -> p::Result<Self> {
        verified.recheck()?;
        if verified.manifest().target != installation.target {
            return Err(unavailable());
        }
        let suffix = if installation.target == p::ProductTarget::WindowsX86_64 {
            ".exe"
        } else {
            ""
        };
        let program = verified
            .content_root()
            .join(format!("bin/forme-hostctl{suffix}"));
        let mut instance = Self::new(installation)?;
        instance.program = program;
        instance.verified = Some(verified);
        Ok(instance)
    }

    pub(super) fn preflight(&self) -> p::Result<()> {
        if let Some(verified) = &self.verified {
            verified.recheck()?;
        }
        for path in [&self.manifest_path, &self.program] {
            super::reject_linked_ancestors(path)?;
            if !std::fs::metadata(path)
                .map_err(|_| unavailable())?
                .is_file()
            {
                return Err(unavailable());
            }
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&self.manifest_path)
            .map_err(|_| unavailable())?
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable())?;
        if bytes.len() > 256 * 1024 {
            return Err(unavailable());
        }
        let current: p::InstallationManifest =
            serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        if current.digest()? != self.installation.digest()? {
            return Err(unavailable());
        }
        Ok(())
    }

    pub(super) fn spawn<T: DeserializeOwned + Send + 'static>(
        &self,
        deadline: Instant,
    ) -> p::Result<MaintenancePipe<T>> {
        self.preflight()?;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        let scratch = self
            .installation
            .paths
            .host_state
            .join("release-data-scratch");
        super::reject_linked_ancestors(&scratch)?;
        let builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        match builder.create(&scratch) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && scratch.is_dir() => {
            }
            Err(_) => return Err(unavailable()),
        }
        let mut command = Command::new(&self.program);
        command
            .args(["maintenance-release-data", "--installation-manifest"])
            .arg(&self.manifest_path)
            .current_dir(&self.installation.paths.host_state)
            .env_clear()
            .env("TEMP", &scratch)
            .env("TMP", &scratch)
            .env("TMPDIR", &scratch)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
            if let Some(root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", root);
            }
        }
        MaintenancePipe::spawn(command, deadline)
    }
}

pub(super) struct MaintenancePipe<T> {
    child: Child,
    requests: Option<SyncSender<Vec<u8>>>,
    replies: Option<Receiver<p::Result<T>>>,
    io: Option<JoinHandle<()>>,
    deadline: Instant,
    finished: bool,
}

impl<T: DeserializeOwned + Send + 'static> MaintenancePipe<T> {
    fn spawn(mut command: Command, deadline: Instant) -> p::Result<Self> {
        let mut child = command.spawn().map_err(|_| unavailable())?;
        let Some(mut input) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(unavailable());
        };
        let Some(output) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(unavailable());
        };
        let (requests, incoming) = mpsc::sync_channel::<Vec<u8>>(1);
        let (outgoing, replies) = mpsc::sync_channel(1);
        // Both write and read run off the caller. A child that refuses stdin
        // cannot defeat the global deadline by blocking write_all in Host.
        let io = std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            while let Ok(frame) = incoming.recv() {
                let result = input
                    .write_all(&frame)
                    .and_then(|_| input.flush())
                    .map_err(|_| unavailable())
                    .and_then(|_| {
                        let mut line = Vec::new();
                        (&mut output)
                            .take((p::RELEASE_WORKER_MAX_FRAME + 1) as u64)
                            .read_until(b'\n', &mut line)
                            .map_err(|_| unavailable())?;
                        if line.len() > p::RELEASE_WORKER_MAX_FRAME || line.last() != Some(&b'\n') {
                            return Err(unavailable());
                        }
                        serde_json::from_slice(&line).map_err(|_| unavailable())
                    });
                let failed = result.is_err();
                if outgoing.try_send(result).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            requests: Some(requests),
            replies: Some(replies),
            io: Some(io),
            deadline,
            finished: false,
        })
    }

    pub(super) fn exchange(&mut self, value: &impl Serialize) -> p::Result<T> {
        let result = (|| {
            if self.finished {
                return Err(unavailable());
            }
            let mut bytes = serde_json::to_vec(value).map_err(|_| unavailable())?;
            bytes.push(b'\n');
            if bytes.len() > p::RELEASE_WORKER_MAX_FRAME {
                return Err(unavailable());
            }
            let remaining = self
                .deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(unavailable)?;
            self.requests
                .as_ref()
                .ok_or_else(unavailable)?
                .try_send(bytes)
                .map_err(|_| unavailable())?;
            self.replies
                .as_ref()
                .ok_or_else(unavailable)?
                .recv_timeout(remaining)
                .map_err(|_| unavailable())?
        })();
        if result.is_err() {
            self.abort();
        }
        result
    }

    pub(super) fn finish(&mut self) -> p::Result<()> {
        self.requests.take();
        let result = loop {
            match self.child.try_wait() {
                Ok(Some(status)) if status.success() => {
                    self.finished = true;
                    break Ok(());
                }
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => break Err(unavailable()),
            }
        };
        if result.is_err() {
            self.abort();
        } else {
            self.join_io();
        }
        result
    }
}

impl<T> MaintenancePipe<T> {
    pub(super) fn abort(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        self.finished = true;
        self.requests.take();
        self.replies.take();
        self.join_io();
    }
    fn join_io(&mut self) {
        if let Some(io) = self.io.take() {
            let _ = io.join();
        }
    }
}
impl<T> Drop for MaintenancePipe<T> {
    fn drop(&mut self) {
        self.abort();
    }
}

fn unavailable() -> p::Error {
    p::Error("installed release data worker is unavailable or interrupted; inspect the original operation".into())
}
