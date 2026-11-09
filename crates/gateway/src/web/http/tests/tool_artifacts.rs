//! Real Web authentication and typed HTTP projection; core bytes are controlled.
use super::*;
use axum::http::HeaderMap;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Core {
    client: Arc<crate::web::core::CoreBridgeClient>,
    mode: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    release: Arc<tokio::sync::Notify>,
    root: PathBuf,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Core {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        let credential_path = root.join("credential");
        let signing_key_path = root.join("signing-key");
        std::fs::write(
            &credential_path,
            "fixture-tool-artifact-core-credential-2026",
        )
        .unwrap();
        std::fs::write(&signing_key_path, [103; 32]).unwrap();
        let mode = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let release_read = release.clone();
        let (mode_read, calls_read) = (mode.clone(), calls.clone());
        let (send, receive) = mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(async move {
                let app=Router::new().route("/v1/artifacts/tools/query",post(|headers:HeaderMap,axum::Json(query):axum::Json<p::ToolArtifactCatalogQuery>| async move {
                    assert!(headers.contains_key("x-forme-core-assertion"));query.validate().unwrap();
                    axum::Json(p::ToolArtifactCatalogView {schema_version:p::TOOL_ARTIFACT_SCHEMA_VERSION,session:query.session,run:query.run,digest:p::sha256_content_digest(b"empty"),total:0,next_offset:None,artifacts:vec![]})
                })).route("/v1/artifacts/tools/read",post(move |headers:HeaderMap,axum::Json(request):axum::Json<p::ToolArtifactRead>| {
                    let (mode,calls,release)=(mode_read.clone(),calls_read.clone(),release_read.clone());async move {
                        assert!(headers.contains_key("x-forme-core-assertion"));request.validate().unwrap();calls.fetch_add(1,Ordering::SeqCst);
                        let mode=mode.load(Ordering::SeqCst);
                        if mode==5 {tokio::time::timeout(Duration::from_secs(5),release.notified()).await.unwrap();}
                        let mut chunk=p::ToolArtifactChunk {schema_version:p::TOOL_ARTIFACT_SCHEMA_VERSION,session:request.session,run:request.run,content_ref:request.content_ref,content_digest:request.content_digest,offset:request.offset,next_offset:request.offset+3,total_bytes:3,finished:true,bytes:vec![137,80,255]};
                        match mode {1=>chunk.session=p::SessionId("session:other".into()),2=>chunk.offset=1,3=>chunk.content_digest=p::sha256_content_digest(b"wrong"),4=>chunk.bytes=vec![0;262145],_=>()};
                        axum::Json(chunk)
                    }
                })).route("/v1/web/session-fence",post(||async {axum::http::StatusCode::NO_CONTENT}));
                let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();send.send(listener.local_addr().unwrap()).unwrap();
                axum::serve(listener,app).with_graceful_shutdown(async move{let _=stopped.await;}).await.unwrap();
            });
        });
        let config = crate::web::config::WebCoreBridgeConfig {
            schema_version: p::SchemaVersion(1),
            address: receive.recv_timeout(Duration::from_secs(5)).unwrap(),
            credential_path,
            signing_key_path,
        };
        let client = Arc::new(
            crate::web::core::CoreBridgeClient::connect(&config, "installation:http-fixture")
                .unwrap(),
        );
        Self {
            client,
            mode,
            calls,
            release,
            root,
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
fn path() -> String {
    format!("/api/v2/sessions/session:images/runs/run:images/artifacts/content:{}?content_digest={}&offset=0&max_bytes=3",p::sha256_content_digest(b"object").0,p::sha256_content_digest(&[137,80,255]).0)
}
fn binary(f: &Fixture, path: &str, cookie: Option<&str>) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(f.address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let cookie = cookie
        .map(|c| format!("Cookie: {c}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n{cookie}\r\n",
        f.address
    )
    .unwrap();
    let mut result = Vec::new();
    stream.read_to_end(&mut result).unwrap();
    let split = result.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8(result[..split].to_vec()).unwrap();
    let status = head
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (status, head, result[split + 4..].to_vec())
}
#[test]
fn tool_artifact_http_requires_session_and_returns_only_verified_raw_range() {
    let core = Core::new();
    let f = Fixture::with_core(true, Some(core.client.clone()));
    assert_eq!(binary(&f, &path(), None).0, 401);
    assert_eq!(core.calls.load(Ordering::SeqCst), 0);
    let (cookie, _) = f.login();
    let (status, head, bytes) = binary(&f, &path(), Some(&cookie));
    assert_eq!(status, 206);
    assert_eq!(bytes, [137, 80, 255]);
    let head = head.to_ascii_lowercase();
    for required in [
        "content-type: image/png",
        "cache-control: no-store",
        "x-content-type-options: nosniff",
        "content-range: bytes 0-2/3",
        "img-src 'self' data: blob:",
        "x-forme-next-offset: 3",
    ] {
        assert!(head.contains(required), "{head}");
    }
    let catalog = f.request(
        "GET",
        "/api/v2/sessions/session:images/runs/run:images/artifacts?offset=0&limit=100",
        &[("Cookie", cookie.clone())],
        "",
    );
    assert_eq!(catalog.status, 200);
    assert_eq!(catalog.json()["artifacts"], json!([]));
    for mode in 1..=4 {
        core.mode.store(mode, Ordering::SeqCst);
        let (status, _, bytes) = binary(&f, &path(), Some(&cookie));
        assert_eq!(status, 502);
        assert!(!bytes.windows(3).any(|w| w == [137, 80, 255]));
    }
    let before = core.calls.load(Ordering::SeqCst);
    for wrong in [
        format!("{}&owner=forged", path()),
        path().replace("max_bytes=3", "max_bytes=262145"),
        path().replace("offset=0", "offset=67108864"),
    ] {
        assert_eq!(binary(&f, &wrong, Some(&cookie)).0, 400);
    }
    assert_eq!(core.calls.load(Ordering::SeqCst), before);
    // The new bridge is finite; existing wildcard proxy does not expose it.
    assert_eq!(
        f.request(
            "GET",
            "/api/v1/brain/artifacts/tools/read",
            &[("Cookie", cookie)],
            ""
        )
        .status,
        404
    );
}

#[test]
fn logout_serializes_with_admitted_image_reads_and_fences_subsequent_reads() {
    let core = Core::new();
    let f = Fixture::with_core(true, Some(core.client.clone()));
    let (cookie, csrf) = f.login();
    core.mode.store(5, Ordering::SeqCst);
    let (sent, received) = mpsc::channel();
    std::thread::scope(|threads| {
        let read = threads.spawn(|| binary(&f, &path(), Some(&cookie)));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while core.calls.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(core.calls.load(Ordering::SeqCst), 1);
        let logout = threads.spawn(|| {
            let result = f.request(
                "POST",
                "/api/v1/auth/logout",
                &[
                    ("Origin", f.origin()),
                    ("Cookie", cookie.clone()),
                    ("X-Forme-Csrf", csrf),
                    ("Idempotency-Key", "logout:tool-image".into()),
                ],
                "{}",
            );
            sent.send(result.status).unwrap();
        });
        // The existing core fence serializes logout behind the admitted read.
        // A queued logout has not yet revoked the session.
        assert!(matches!(
            received.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        core.release.notify_one();
        let (status, _, body) = read.join().unwrap();
        assert_eq!(status, 206);
        assert_eq!(body, [137, 80, 255]);
        assert!(matches!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            200 | 204
        ));
        logout.join().unwrap();
    });
    let (status, _, body) = binary(&f, &path(), Some(&cookie));
    assert_eq!(status, 401);
    assert!(!body.windows(3).any(|w| w == [137, 80, 255]));
    assert_eq!(core.calls.load(Ordering::SeqCst), 1);
}
