use super::http::Request;
use forme_execution::installed::{HostAuth, IsolatedExecutorHost};
use forme_protocol as p;

pub(super) fn route(
    request: &Request,
    worker: &dyn IsolatedExecutorHost,
    auth: &HostAuth,
    now: p::Timestamp,
) -> Option<p::Result<Vec<u8>>> {
    if !request.path.starts_with("/v1/agent/workers/") {
        return None;
    }
    if request.method == "POST" && request.path == "/v1/agent/workers/channel/dispatch" {
        return Some(
            serde_json::from_slice(&request.body)
                .map_err(|_| p::Error("channel dispatch input is invalid".into()))
                .and_then(|input| worker.dispatch_channel(auth, input, now))
                .and_then(|observation| {
                    serde_json::to_vec(&observation)
                        .map_err(|_| p::Error("channel observation encoding failed".into()))
                }),
        );
    }
    if request.method == "GET" && request.path.ends_with("/desktop-output") {
        return Some(
            worker_id(&request.path, "/v1/agent/workers/", "/desktop-output")
                .and_then(|job_id| worker.desktop_output(auth, job_id, now))
                .and_then(|descriptor| {
                    serde_json::to_vec(&descriptor)
                        .map_err(|_| p::Error("desktop descriptor encoding failed".into()))
                }),
        );
    }
    if request.method == "GET" && request.path.ends_with("/result") {
        return Some(
            worker_id(&request.path, "/v1/agent/workers/", "/result")
                .and_then(|job_id| manifest(worker, auth, job_id, now)),
        );
    }
    if request.method == "POST" && request.path.ends_with("/result/read") {
        return Some(
            worker_id(&request.path, "/v1/agent/workers/", "/result/read")
                .and_then(|job_id| read(worker, auth, job_id, &request.body, now)),
        );
    }
    None
}

pub(super) fn manifest(
    worker: &dyn IsolatedExecutorHost,
    auth: &HostAuth,
    job_id: &str,
    now: p::Timestamp,
) -> p::Result<Vec<u8>> {
    serde_json::to_vec(&worker.result_manifest(auth, job_id, now)?)
        .map_err(|_| p::Error("worker result manifest response failed".into()))
}

fn worker_id<'a>(path: &'a str, prefix: &str, suffix: &str) -> p::Result<&'a str> {
    let value = path
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(suffix))
        .filter(|value| !value.is_empty() && !value.contains('/'))
        .ok_or_else(|| p::Error("worker result path is invalid".into()))?;
    p::validate_product_id(value)?;
    Ok(value)
}

pub(super) fn read(
    worker: &dyn IsolatedExecutorHost,
    auth: &HostAuth,
    job_id: &str,
    body: &[u8],
    now: p::Timestamp,
) -> p::Result<Vec<u8>> {
    let request: p::WorkerResultRead = serde_json::from_slice(body)
        .map_err(|_| p::Error("worker result read body is invalid".into()))?;
    if request.binding.job_id != job_id {
        return Err(p::Error("worker result read job binding is invalid".into()));
    }
    serde_json::to_vec(&worker.read_result(auth, request, now)?)
        .map_err(|_| p::Error("worker result chunk response failed".into()))
}
