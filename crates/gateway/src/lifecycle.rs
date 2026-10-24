//! Admission and drain shared by the foreground daemon and OS service hosts.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use forme_protocol as p;
use tokio::sync::Notify;

#[derive(Debug, Clone)]
pub struct GatewayLifecycleConfig {
    pub schema_version: p::SchemaVersion,
    pub drain_timeout: Duration,
}

impl Default for GatewayLifecycleConfig {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            drain_timeout: Duration::from_secs(30),
        }
    }
}

impl GatewayLifecycleConfig {
    pub fn from_environment() -> p::Result<Self> {
        let timeout = match std::env::var("FORME_SERVICE_DRAIN_TIMEOUT_MS") {
            Ok(value) => value.parse::<u64>().map_err(|_| {
                p::Error("FORME_SERVICE_DRAIN_TIMEOUT_MS must be an integer".into())
            })?,
            Err(std::env::VarError::NotPresent) => 30_000,
            Err(_) => return Err(p::Error("service drain configuration is invalid".into())),
        };
        let config = Self {
            schema_version: p::SchemaVersion(1),
            drain_timeout: Duration::from_millis(timeout),
        };
        config.validate()?;
        Ok(config)
    }

    pub(crate) fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.drain_timeout.is_zero()
            || self.drain_timeout > Duration::from_secs(600)
        {
            return Err(p::Error(
                "service drain must be versioned and bounded".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
struct AdmissionState {
    closing: bool,
    active: usize,
}

#[derive(Default)]
pub(crate) struct WorkAdmission {
    state: Mutex<AdmissionState>,
    changed: Notify,
    stopped: Notify,
    check: Option<Arc<dyn Fn() -> p::Result<()> + Send + Sync>>,
}

impl WorkAdmission {
    /// Independent product holds must all allow work. Combining their guards
    /// keeps one active/drain counter instead of acquiring nested work leases.
    pub(crate) fn and(self, other: Self) -> Self {
        Self::guarded(move || {
            if let Some(check) = &self.check {
                check()?;
            }
            if let Some(check) = &other.check {
                check()?;
            }
            Ok(())
        })
    }

    pub(crate) fn guarded(check: impl Fn() -> p::Result<()> + Send + Sync + 'static) -> Self {
        Self {
            check: Some(Arc::new(check)),
            ..Self::default()
        }
    }

    pub(crate) fn acquire(self: &Arc<Self>) -> p::Result<WorkLease> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| p::Error("core admission is unavailable".into()))?;
        if state.closing {
            return Err(p::Error("core is draining; new work is unavailable".into()));
        }
        if let Some(check) = &self.check {
            check()?;
        }
        state.active = state
            .active
            .checked_add(1)
            .ok_or_else(|| p::Error("core admission capacity is exhausted".into()))?;
        Ok(WorkLease(self.clone()))
    }

    pub(crate) fn close(&self) -> p::Result<()> {
        self.state
            .lock()
            .map_err(|_| p::Error("core admission is unavailable".into()))?
            .closing = true;
        self.stopped.notify_one();
        Ok(())
    }

    pub(crate) async fn stopped(&self) {
        self.stopped.notified().await;
    }

    pub(crate) async fn idle(&self) -> p::Result<()> {
        loop {
            if self
                .state
                .lock()
                .map_err(|_| p::Error("core admission is unavailable".into()))?
                .active
                == 0
            {
                return Ok(());
            }
            // There is one drain waiter. notify_one retains a wake even if the
            // final lease finishes between the counter check and this await.
            self.changed.notified().await;
        }
    }
}

pub(crate) struct WorkLease(Arc<WorkAdmission>);

impl Drop for WorkLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.active -= 1;
        }
        self.0.changed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn closing_admission_preserves_existing_work_and_rejects_later_claims() {
        let admission = Arc::new(WorkAdmission::default());
        let lease = admission.acquire().unwrap();
        admission.close().unwrap();
        admission.close().unwrap();
        assert!(admission.acquire().is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), admission.idle())
                .await
                .is_err()
        );
        drop(lease);
        tokio::time::timeout(Duration::from_secs(1), admission.idle())
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn drain_configuration_rejects_unversioned_or_unbounded_waits() {
        GatewayLifecycleConfig::default().validate().unwrap();
        for (version, timeout) in [
            (0, Duration::from_secs(1)),
            (2, Duration::from_secs(1)),
            (1, Duration::ZERO),
            (1, Duration::from_secs(601)),
        ] {
            assert!(GatewayLifecycleConfig {
                schema_version: p::SchemaVersion(version),
                drain_timeout: timeout,
            }
            .validate()
            .is_err());
        }
    }
}
