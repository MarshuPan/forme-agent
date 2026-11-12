//! Host-owned exact-message authorization. Secrets and HTTP stay outside workers.
use super::{management_repair::storage::ReadCustody, ProductConfigStore};
use forme_protocol as p;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, TryLockError},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[cfg(windows)]
mod exchange;
#[cfg(windows)]
pub(crate) mod isolation;
pub(crate) mod proxy;

pub struct InstalledChannelDelivery {
    configuration: Arc<Mutex<ProductConfigStore>>,
    credentials: PathBuf,
    intents: PathBuf,
}
#[derive(Clone)]
pub(crate) struct Admission {
    pub(crate) input: Arc<p::ManagedChannelDeliveryInputV1>,
    pub(crate) source: p::ProductChannelSource,
    pub(crate) grant: p::ManagedChannelDeliveryGrantV1,
    pub(crate) deadline: Instant,
}
impl InstalledChannelDelivery {
    /// Both roots are derived by the installed Host from its private state root.
    pub fn new(configuration: Arc<Mutex<ProductConfigStore>>, root: &Path) -> p::Result<Arc<Self>> {
        if !root.is_absolute() {
            return Err(invalid());
        }
        super::reject_linked_ancestors(root)?;
        fs::create_dir_all(root).map_err(|_| invalid())?;
        super::process::set_private_directory(root)?;
        let intents = root.join("intents");
        fs::create_dir_all(&intents).map_err(|_| invalid())?;
        super::reject_linked_ancestors(&intents)?;
        super::process::set_private_directory(&intents)?;
        Ok(Arc::new(Self {
            configuration,
            credentials: root.join("credentials"),
            intents,
        }))
    }
    #[cfg(windows)]
    pub(crate) fn bind_consumer(self: &Arc<Self>) -> p::Result<()> {
        self.configuration
            .lock()
            .map_err(|_| invalid())?
            .bind_channel_consumer(self);
        Ok(())
    }
    pub(crate) fn with_admission<T>(
        &self,
        input: p::ManagedChannelDeliveryInputV1,
        installation: &str,
        now: p::Timestamp,
        entered: Instant,
        action: impl FnOnce(Admission) -> p::Result<T>,
    ) -> p::Result<T> {
        let remaining = input.execution.job.deadline.saturating_sub(now);
        if !(1..=60_000).contains(&remaining) {
            return Err(invalid());
        }
        let deadline = entered + Duration::from_millis(remaining as u64);
        let cancel = crate::CancelToken::default();
        let mut config = self.config_until(deadline, &cancel)?;
        let (source, custody) = config.channel_delivery_source(&input.execution)?;
        let grant = input.validate_for(installation, &source, now)?;
        // Check secret availability before journal admission, without exposing it.
        let _secret = self.secret(&source)?;
        custody.recheck_records()?;
        live(deadline, &cancel)?;
        action(Admission {
            input: Arc::new(input),
            source,
            grant,
            deadline,
        })
    }
    pub(crate) fn deliver(
        &self,
        admission: &Admission,
        cancel: &crate::CancelToken,
        attempted: &std::sync::atomic::AtomicBool,
    ) -> p::Result<Vec<u8>> {
        let mut config = self.config_until(admission.deadline, cancel)?;
        let (source, custody) = config.channel_delivery_source(&admission.input.execution)?;
        if source != admission.source {
            return Err(invalid());
        }
        let now = now_ms();
        let grant =
            admission
                .input
                .validate_for(&admission.grant.installation_ref, &source, now)?;
        if grant != admission.grant {
            return Err(invalid());
        }
        let (secret, secret_custody) = self.secret(&source)?;
        custody.recheck_records()?;
        secret_custody.recheck_records()?;
        live(admission.deadline, cancel)?;
        // Exclusive create survives crashes. A partial intent is also consumed;
        // neither restart nor a duplicate invocation can reconnect the recipient.
        let id = p::canonical_digest(&("forme.channel.send-once.v1", &grant.owner, &grant.nonce))?;
        super::process::write_private_json(
            &self.intents.join(format!("{}.json", &id.0[7..])),
            &grant,
        )?;
        live(admission.deadline, cancel)?;
        attempted.store(true, std::sync::atomic::Ordering::SeqCst);
        proxy::post(
            &grant,
            &source,
            &admission.input.content_bytes,
            &secret,
            admission.deadline,
            cancel,
        )
    }
    fn secret(
        &self,
        source: &p::ProductChannelSource,
    ) -> p::Result<(Zeroizing<String>, ReadCustody)> {
        let delivery = source.delivery.as_ref().ok_or_else(invalid)?;
        let id = p::sha256_content_digest(delivery.credential_ref.as_bytes());
        let path = self.credentials.join(format!("{}.token", &id.0[7..]));
        let mut custody = ReadCustody::default();
        let bytes = Zeroizing::new(custody.read(&path, 4096)?);
        let secret = std::str::from_utf8(&bytes)
            .map_err(|_| invalid())?
            .trim_end_matches(['\r', '\n']);
        if !(32..=4096).contains(&secret.len())
            || !secret.bytes().all(|b| b.is_ascii_graphic())
            || p::sha256_content_digest(secret.as_bytes()) != delivery.credential_digest
        {
            return Err(invalid());
        }
        Ok((Zeroizing::new(secret.to_owned()), custody))
    }
    pub(crate) fn available(&self, source: &p::ProductChannelSource) -> p::Result<()> {
        source.validate()?;
        self.secret(source).map(|_| ())
    }
    fn config_until(
        &self,
        deadline: Instant,
        cancel: &crate::CancelToken,
    ) -> p::Result<MutexGuard<'_, ProductConfigStore>> {
        loop {
            live(deadline, cancel)?;
            match self.configuration.try_lock() {
                Ok(value) => return Ok(value),
                Err(TryLockError::Poisoned(_)) => return Err(invalid()),
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }
}
fn live(deadline: Instant, cancel: &crate::CancelToken) -> p::Result<()> {
    if cancel.is_cancelled() || Instant::now() >= deadline {
        Err(invalid())
    } else {
        Ok(())
    }
}
fn now_ms() -> p::Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(i64::MAX)
}
fn invalid() -> p::Error {
    p::Error("channel delivery authority or original outcome is unavailable".into())
}

#[cfg(test)]
mod tests;
