//! Runtime lifetime guards; dropped only after the Harness releases its authority.
use super::*;
use std::sync::Condvar;

/// Keeps the sole-writer lease alive while the runtime exists, including idle
/// periods and long model calls that perform no durable write. Without it an
/// idle authority could silently lose its lease to a second process.
pub(crate) struct LeaseHeartbeat {
    stop: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl LeaseHeartbeat {
    pub(crate) fn start(guard: AuthorityLeaseGuard, ttl: p::DurationMs) -> p::Result<Self> {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let signal = stop.clone();
        let period = std::time::Duration::from_millis((ttl.0 / 3).max(1));
        let handle = std::thread::Builder::new()
            .name("forme-authority-lease".into())
            .spawn(move || {
                let (lock, wake) = &*signal;
                let Ok(mut stopped) = lock.lock() else {
                    return;
                };
                loop {
                    match wake.wait_timeout(stopped, period) {
                        Ok((next, _)) => stopped = next,
                        Err(_) => return,
                    }
                    // A lost lease is final: writes are already fenced, and
                    // the heartbeat must never try to reclaim authority.
                    if *stopped || guard.ensure(now_ms()).is_err() {
                        return;
                    }
                }
            })
            .map_err(|_| p::Error("authority lease heartbeat could not start".into()))?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }
}

impl Drop for LeaseHeartbeat {
    fn drop(&mut self) {
        let (lock, wake) = &*self.stop;
        if let Ok(mut stopped) = lock.lock() {
            *stopped = true;
            wake.notify_all();
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
