//! Opt-in, disposable real Web daemon for manual browser component checks.
//! The preclaimed fixture does not certify installation, claim, or a real brain.
#![forbid(unsafe_code)]

use ed25519_dalek::{Signer, SigningKey};
use forme_gateway::web::identity::{SensitivePassword, WebIdentityStore};
use forme_protocol as p;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// Public repository-owned fixture credentials. Never used for a real owner.
const PASSWORD: &str = "browser fixture password 20261026";

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "opt-in local browser preview; creates only a temporary fixture account"]
fn claimed_web_daemon_for_browser_component_checks() {
    assert_eq!(std::env::var("FORME_BROWSER_PREVIEW").as_deref(), Ok("1"));
    let root = tempfile::Builder::new()
        .prefix("forme-browser-preview-")
        .tempdir()
        .unwrap();
    let identity_path = root.path().join("identity");
    std::fs::create_dir(&identity_path).unwrap();
    let key = SigningKey::from_bytes(&[113; 32]);
    let identity = WebIdentityStore::open(
        &identity_path,
        "installation:browser-component-fixture",
        key.verifying_key(),
    )
    .unwrap();
    let commitment = identity
        .prepare_claim(
            "claim:browser-component-fixture",
            "browser-fixture",
            SensitivePassword::new(PASSWORD.into()),
        )
        .unwrap();
    let receipt = p::WebClaimCommit {
        schema_version: p::SchemaVersion(1),
        install_ref: "installation:browser-component-fixture".into(),
        operation_id: "claim:browser-component-fixture".into(),
        candidate_ref: "data:browser-component-fixture".into(),
        owner: p::OwnerPrincipalRef("owner:browser-fixture".into()),
        account_commitment: commitment,
        expires_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
            + 60_000,
    };
    let digest = p::canonical_digest(&("forme-web-claim-commit-v1", &receipt)).unwrap();
    identity
        .commit_claim(&p::SignedWebClaimCommit {
            schema_version: p::SchemaVersion(1),
            receipt,
            signature: key.sign(digest.0.as_bytes()).to_bytes().to_vec(),
        })
        .unwrap();
    drop(identity);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let config = root.path().join("web.json");
    std::fs::write(
        &config,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "install_ref": "installation:browser-component-fixture",
            "identity_directory": identity_path,
            "claim_verifying_key": key.verifying_key().to_bytes(),
            "bind": address
        }))
        .unwrap(),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_forme-webd"));
    command
        .args(["--config", config.to_str().unwrap()])
        .current_dir(root.path())
        .env_clear()
        .env("TEMP", root.path())
        .env("TMP", root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    let mut daemon = Daemon(command.spawn().unwrap());
    let stdout = daemon.0.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line);
        let _ = sender.send(result.map(|_| line));
    });
    let line = receiver
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap();
    reader.join().unwrap();
    assert!(
        line.starts_with("FORME_WEB_URL=http://127.0.0.1:"),
        "preview startup did not report a listener; status={:?}, stdout={line:?}",
        daemon.0.try_wait().unwrap()
    );
    let stop = root.path().join("stop-preview");
    println!("{}", line.trim());
    println!("PREVIEW_STOP={}", stop.display());
    println!("PREVIEW_SCOPE=preclaimed real webd; core offline; fixture data only");
    std::io::stdout().flush().unwrap();
    let started = Instant::now();
    while !stop.exists() && started.elapsed() < Duration::from_secs(900) {
        assert!(
            daemon.0.try_wait().unwrap().is_none(),
            "preview daemon exited"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        stop.is_file(),
        "browser preview expired without an explicit stop"
    );
    drop(daemon);
    root.close().unwrap();
}
