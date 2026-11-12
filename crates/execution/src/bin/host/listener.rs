//! A broken incoming TCP connection must not take down every Host domain.
use forme_protocol as p;
use std::{
    io,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct AcceptRetry {
    next: Option<Instant>,
    failures: u32,
}
impl AcceptRetry {
    pub(super) fn poll<T>(
        &mut self,
        now: Instant,
        accept: impl FnOnce() -> io::Result<T>,
    ) -> p::Result<Option<T>> {
        if self.next.is_some_and(|next| now < next) {
            return Ok(None);
        }
        match accept() {
            Ok(value) => {
                self.next = None;
                self.failures = 0;
                Ok(Some(value))
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) if transient(&error) => {
                self.failures = self.failures.saturating_add(1).min(6);
                let millis = (100_u64 << (self.failures - 1)).min(2000);
                self.next = Some(now + Duration::from_millis(millis));
                Ok(None)
            }
            Err(error) => Err(p::Error(format!(
                "host broker accept failed: {:?}/{:?}",
                error.kind(),
                error.raw_os_error()
            ))),
        }
    }
}
fn transient(error: &io::Error) -> bool {
    if matches!(
        error.kind(),
        io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::TimedOut
            | io::ErrorKind::ConnectionRefused
    ) {
        return true;
    }
    // WSAENETDOWN, WSAENETRESET, WSAENOBUFS. The existing bound socket remains
    // owned; no new listener or authority is created while retrying.
    cfg!(windows) && matches!(error.raw_os_error(), Some(10050 | 10052 | 10055))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aborted_connection_does_not_close_other_domains_and_success_resets_backoff() {
        let now = Instant::now();
        let mut failed = AcceptRetry::default();
        let mut other = AcceptRetry::default();
        assert_eq!(
            failed
                .poll::<u8>(now, || Err(io::ErrorKind::ConnectionAborted.into()))
                .unwrap(),
            None
        );
        assert_eq!(other.poll(now, || Ok(7)).unwrap(), Some(7));
        assert_eq!(
            failed
                .poll::<u8>(now + Duration::from_millis(99), || panic!(
                    "retry called before deadline"
                ))
                .unwrap(),
            None
        );
        assert_eq!(
            failed
                .poll(now + Duration::from_millis(100), || Ok(8))
                .unwrap(),
            Some(8)
        );
        assert_eq!(
            failed
                .poll::<u8>(now + Duration::from_millis(101), || Err(
                    io::ErrorKind::WouldBlock.into()
                ))
                .unwrap(),
            None
        );
        assert_eq!(
            failed
                .poll(now + Duration::from_millis(101), || Ok(9))
                .unwrap(),
            Some(9)
        );
    }
    #[test]
    fn retry_is_bounded_but_fatal_errors_still_fail_closed() {
        let mut now = Instant::now();
        let mut retry = AcceptRetry::default();
        for _ in 0..20 {
            assert!(retry
                .poll::<u8>(now, || Err(io::ErrorKind::Interrupted.into()))
                .unwrap()
                .is_none());
            let next = retry.next.unwrap();
            assert!(next.duration_since(now) <= Duration::from_secs(2));
            now = next;
        }
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::InvalidInput,
            io::ErrorKind::Unsupported,
        ] {
            assert!(retry.poll::<u8>(now, || Err(kind.into())).is_err());
        }
    }
}
