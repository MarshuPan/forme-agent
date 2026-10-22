//! Fixed-program local maintenance worker transport, independent of brain crates.
use forme_protocol as p;
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

pub struct InstalledRestoreWorker {
    installation: p::InstallationManifest,
    manifest_path: PathBuf,
    program: PathBuf,
}

pub trait RestoreWorkerProvider: Send + Sync {
    fn installation(&self) -> &p::InstallationManifest;
    fn preflight(&self) -> p::Result<()>;
    fn start(
        &self,
        request: p::ProductRestoreRequest,
        deadline: Instant,
    ) -> p::Result<Box<dyn RestoreWorkerSession>>;
}

pub trait RestoreWorkerSession: Send {
    fn validated(&self) -> p::Result<p::ProductRestoreWorkerResponse>;
    fn step(&mut self, step: p::ProductRestoreStep) -> p::Result<p::ProductRestoreWorkerResponse>;
}

impl InstalledRestoreWorker {
    pub fn new(installation: p::InstallationManifest) -> p::Result<Self> {
        super::validate_installation_paths(&installation)?;
        let manifest_path = installation.paths.config.join("installation.json");
        let name = if installation.target == p::ProductTarget::WindowsX86_64 {
            "forme-hostctl.exe"
        } else {
            "forme-hostctl"
        };
        let program = installation.paths.releases.join("bin").join(name);
        let worker = Self {
            installation,
            manifest_path,
            program,
        };
        Ok(worker)
    }

    pub fn installation(&self) -> &p::InstallationManifest {
        &self.installation
    }

    pub fn preflight(&self) -> p::Result<()> {
        for path in [&self.manifest_path, &self.program] {
            super::reject_linked_ancestors(path)?;
            if !std::fs::metadata(path)
                .map_err(|_| unavailable())?
                .is_file()
            {
                return Err(unavailable());
            }
        }
        let file = std::fs::File::open(&self.manifest_path).map_err(|_| unavailable())?;
        let mut bytes = Vec::new();
        file.take(256 * 1024 + 1)
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

    pub fn start(
        &self,
        request: p::ProductRestoreRequest,
        deadline: Instant,
    ) -> p::Result<RestoreWorkerProcess> {
        request.validate()?;
        self.preflight()?;
        if request.install_ref != self.installation.install_id || Instant::now() >= deadline {
            return Err(unavailable());
        }
        let scratch = self.installation.paths.host_state.join("restore-scratch");
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
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && scratch.is_dir() => {}
            Err(_) => return Err(unavailable()),
        }
        let mut command = Command::new(&self.program);
        command
            .args(["maintenance-restore", "--installation-manifest"])
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
        let mut child = command.spawn().map_err(|_| unavailable())?;
        let input = child.stdin.take().ok_or_else(unavailable)?;
        let output = child.stdout.take().ok_or_else(unavailable)?;
        let (sender, replies) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = String::new();
                let response = (&mut reader)
                    .take((p::RESTORE_WORKER_MAX_FRAME + 1) as u64)
                    .read_line(&mut line)
                    .map_err(|_| unavailable())
                    .and_then(|_| {
                        if !line.ends_with('\n') || line.len() > p::RESTORE_WORKER_MAX_FRAME {
                            return Err(unavailable());
                        }
                        serde_json::from_str(&line).map_err(|_| unavailable())
                    });
                let failed = response.is_err();
                if sender.send(response).is_err() || failed {
                    break;
                }
            }
        });
        let mut session = RestoreWorkerProcess {
            child,
            input: Some(input),
            replies,
            request,
            deadline,
            released: false,
        };
        session.send(&session.request.clone())?;
        Ok(session)
    }
}

pub struct RestoreWorkerProcess {
    child: Child,
    input: Option<ChildStdin>,
    replies: Receiver<p::Result<p::ProductRestoreWorkerResponse>>,
    request: p::ProductRestoreRequest,
    deadline: Instant,
    released: bool,
}

impl RestoreWorkerProcess {
    pub fn validated(&self) -> p::Result<p::ProductRestoreWorkerResponse> {
        let response = self.receive()?;
        if !matches!(
            response.result,
            p::ProductRestoreWorkerResult::Validated { .. }
        ) {
            return Err(unavailable());
        }
        Ok(response)
    }
    pub fn step(
        &mut self,
        step: p::ProductRestoreStep,
    ) -> p::Result<p::ProductRestoreWorkerResponse> {
        self.send(&p::ProductRestoreStepRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: self.request.operation_id.clone(),
            command_digest: self.request.command_digest.clone(),
            step,
        })?;
        let response = self.receive()?;
        let matching = matches!(
            (step, &response.result),
            (
                p::ProductRestoreStep::Backup,
                p::ProductRestoreWorkerResult::BackedUp { .. }
            ) | (
                p::ProductRestoreStep::FenceSessions,
                p::ProductRestoreWorkerResult::SessionsFenced { .. }
            ) | (
                p::ProductRestoreStep::Release,
                p::ProductRestoreWorkerResult::Released { .. }
            )
        );
        if !matching {
            return Err(unavailable());
        }
        if step == p::ProductRestoreStep::Release {
            self.input.take();
            loop {
                match self.child.try_wait().map_err(|_| unavailable())? {
                    Some(status) if status.success() => {
                        self.released = true;
                        break;
                    }
                    Some(_) => return Err(unavailable()),
                    None if Instant::now() < self.deadline => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    None => return Err(unavailable()),
                }
            }
        }
        Ok(response)
    }
    fn send(&mut self, value: &impl serde::Serialize) -> p::Result<()> {
        if Instant::now() >= self.deadline || self.released {
            return Err(unavailable());
        }
        let bytes = serde_json::to_vec(value).map_err(|_| unavailable())?;
        if bytes.len() + 1 > p::RESTORE_WORKER_MAX_FRAME {
            return Err(unavailable());
        }
        let input = self.input.as_mut().ok_or_else(unavailable)?;
        input
            .write_all(&bytes)
            .and_then(|_| input.write_all(b"\n"))
            .and_then(|_| input.flush())
            .map_err(|_| unavailable())
    }
    fn receive(&self) -> p::Result<p::ProductRestoreWorkerResponse> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(unavailable)?;
        let response = self
            .replies
            .recv_timeout(remaining)
            .map_err(|_| unavailable())??;
        response.validate_for(&self.request)?;
        Ok(response)
    }
}
impl Drop for RestoreWorkerProcess {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
impl RestoreWorkerProvider for InstalledRestoreWorker {
    fn installation(&self) -> &p::InstallationManifest {
        &self.installation
    }
    fn preflight(&self) -> p::Result<()> {
        InstalledRestoreWorker::preflight(self)
    }
    fn start(
        &self,
        request: p::ProductRestoreRequest,
        deadline: Instant,
    ) -> p::Result<Box<dyn RestoreWorkerSession>> {
        Ok(Box::new(InstalledRestoreWorker::start(
            self, request, deadline,
        )?))
    }
}
impl RestoreWorkerSession for RestoreWorkerProcess {
    fn validated(&self) -> p::Result<p::ProductRestoreWorkerResponse> {
        RestoreWorkerProcess::validated(self)
    }
    fn step(&mut self, step: p::ProductRestoreStep) -> p::Result<p::ProductRestoreWorkerResponse> {
        RestoreWorkerProcess::step(self, step)
    }
}
fn unavailable() -> p::Error {
    p::Error("installed restore worker is unavailable, unbound or interrupted; inspect the original operation".into())
}
