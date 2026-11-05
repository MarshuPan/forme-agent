//! Fixed installed release-data worker, to be owned by the independent updater.
use super::maintenance_pipe::{MaintenancePipe, ReleaseDataProgram};
use forme_protocol as p;
use std::time::Instant;

pub struct InstalledReleaseDataWorker {
    program: ReleaseDataProgram,
}
impl InstalledReleaseDataWorker {
    pub(crate) fn from_verified(
        installation: p::InstallationManifest,
        verified: super::VerifiedStagedRelease,
    ) -> p::Result<Self> {
        Ok(Self {
            program: ReleaseDataProgram::from_verified(installation, verified)?,
        })
    }
    pub fn new(installation: p::InstallationManifest) -> p::Result<Self> {
        Ok(Self {
            program: ReleaseDataProgram::new(installation)?,
        })
    }
    pub fn preflight(&self) -> p::Result<()> {
        self.program.preflight()
    }

    pub fn start(
        &self,
        request: p::ProductReleaseDataRequest,
        deadline: Instant,
    ) -> p::Result<ReleaseDataWorkerProcess> {
        request.validate()?;
        if request.release.install_ref != self.program.installation.install_id {
            return Err(invalid());
        }
        let mut pipe: MaintenancePipe<p::ProductReleaseDataResponse> =
            self.program.spawn(deadline)?;
        let inspected = pipe.exchange(&request)?;
        inspected.validate_for(&request)?;
        let p::ProductReleaseDataResult::Inspected { identity } = &inspected.result else {
            return Err(invalid());
        };
        Ok(ReleaseDataWorkerProcess {
            request,
            identity: identity.clone(),
            inspected,
            pipe,
            backup_attempted: false,
            released: false,
        })
    }
}

pub struct ReleaseDataWorkerProcess {
    request: p::ProductReleaseDataRequest,
    identity: p::ProductReleaseDataIdentity,
    inspected: p::ProductReleaseDataResponse,
    pipe: MaintenancePipe<p::ProductReleaseDataResponse>,
    backup_attempted: bool,
    released: bool,
}
impl ReleaseDataWorkerProcess {
    pub fn inspected(&self) -> &p::ProductReleaseDataResponse {
        &self.inspected
    }
    pub fn identity(&self) -> &p::ProductReleaseDataIdentity {
        &self.identity
    }

    /// The verified, fixed worker retains its store fence until Release or
    /// exit. A cached identity is insufficient once that process has ended.
    /// This checks the same child without sending a request or acquiring a new
    /// fence. It is not a fresh database inspection or historical evidence.
    pub fn ensure_read_only_hold(&mut self, deadline: Instant) -> p::Result<()> {
        if self.released || self.backup_attempted {
            return Err(invalid());
        }
        let result = self.pipe.ensure_running(deadline);
        if result.is_err() {
            self.released = true;
        }
        result
    }

    /// Backup is consumed before dispatch, including uncertain/failed results.
    /// The owning updater must have persisted its matching stage intent first.
    pub fn step(
        &mut self,
        step: p::ProductReleaseDataStep,
    ) -> p::Result<p::ProductReleaseDataResponse> {
        if self.released || (step == p::ProductReleaseDataStep::Backup && self.backup_attempted) {
            return Err(invalid());
        }
        if step == p::ProductReleaseDataStep::Backup {
            self.backup_attempted = true;
        }
        let response = self.pipe.exchange(&p::ProductReleaseDataStepRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: self.request.release.operation_id.clone(),
            command_digest: self.request.release.command_digest.clone(),
            step,
        })?;
        let verified =
            response
                .validate_for(&self.request)
                .and_then(|_| match (step, &response.result) {
                    (
                        p::ProductReleaseDataStep::Backup,
                        p::ProductReleaseDataResult::BackedUp { identity, .. },
                    ) if identity == &self.identity => Ok(()),
                    (
                        p::ProductReleaseDataStep::Release,
                        p::ProductReleaseDataResult::Released {},
                    ) => Ok(()),
                    _ => Err(invalid()),
                });
        if let Err(error) = verified {
            self.pipe.abort();
            self.released = true;
            return Err(error);
        }
        if step == p::ProductReleaseDataStep::Release {
            self.pipe.finish()?;
            self.released = true;
        }
        Ok(response)
    }
}
fn invalid() -> p::Error {
    p::Error("release data worker binding or step is invalid".into())
}
