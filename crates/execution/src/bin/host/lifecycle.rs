//! One-shot startup coordination, joined by the Host that owns its listeners.
use forme_protocol as p;

pub(super) fn serve_after_ready(
    ready: impl FnOnce() -> p::Result<()>,
    reconcile: impl FnOnce() + Send,
    serve: impl FnOnce() -> p::Result<()>,
) -> p::Result<()> {
    // Brain depends on the Host's OS service. Do not enter synchronous native
    // service control while the Host still reports StartPending.
    ready()?;
    std::thread::scope(|scope| {
        let startup = std::thread::Builder::new()
            .name("forme-core-startup".into())
            .spawn_scoped(scope, reconcile)
            .map_err(|_| p::Error("Host startup coordination is unavailable".into()))?;
        // Keep accepting internal requests while service startup waits for
        // configuration/worker probes. This is not a service restart loop.
        let serving = serve();
        let joined = startup
            .join()
            .map_err(|_| p::Error("Host startup coordination did not finish".into()));
        serving.and(joined)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    #[test]
    fn readiness_precedes_startup_and_serving_can_answer_its_request() {
        let ready = AtomicBool::new(false);
        let finished = AtomicBool::new(false);
        let (request_tx, request_rx) = mpsc::sync_channel(0);
        let (reply_tx, reply_rx) = mpsc::sync_channel(0);
        let startup_ready = &ready;
        let startup_finished = &finished;
        serve_after_ready(
            || {
                ready.store(true, Ordering::Release);
                Ok(())
            },
            move || {
                assert!(startup_ready.load(Ordering::Acquire));
                request_tx.send(()).unwrap();
                reply_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                startup_finished.store(true, Ordering::Release);
            },
            || {
                request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                reply_tx.send(()).unwrap();
                Ok(())
            },
        )
        .unwrap();
        assert!(finished.load(Ordering::Acquire));
    }

    #[test]
    fn failed_readiness_starts_neither_coordination_nor_serving() {
        let called = AtomicBool::new(false);
        assert!(serve_after_ready(
            || Err(p::Error("fixture readiness rejected".into())),
            || called.store(true, Ordering::Release),
            || {
                called.store(true, Ordering::Release);
                Ok(())
            },
        )
        .is_err());
        assert!(!called.load(Ordering::Acquire));
    }

    #[test]
    fn serving_failure_still_joins_startup_before_returning() {
        let finished = AtomicBool::new(false);
        let (finish_tx, finish_rx) = mpsc::sync_channel(0);
        let startup_finished = &finished;
        let result = serve_after_ready(
            || Ok(()),
            move || {
                finish_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                startup_finished.store(true, Ordering::Release);
            },
            || {
                finish_tx.send(()).unwrap();
                Err(p::Error("fixture listener failed".into()))
            },
        );
        assert_eq!(result.unwrap_err().0, "fixture listener failed");
        assert!(finished.load(Ordering::Acquire));
    }

    #[test]
    fn startup_panic_is_a_host_failure_and_is_joined() {
        assert!(serve_after_ready(
            || Ok(()),
            || panic!("fixture startup worker failed"),
            || Ok(()),
        )
        .is_err());
    }
}
