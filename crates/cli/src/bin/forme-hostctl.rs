//! Local operator entry point, not an Agent or a second brain.
#![forbid(unsafe_code)]

use std::io::{self, Read};
use std::net::SocketAddr;
use std::path::PathBuf;

use forme_gateway::web::bootstrap::LocalBootstrapClient;
use forme_gateway::web::identity::{SensitivePassword, WebIdentityStore};
use forme_gateway::web::WebServerConfig;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme-hostctl: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|value| {
        matches!(
            value.as_str(),
            "inspect-data" | "backup-data" | "verify-data" | "plan-restore-data" | "restore-data"
        )
    }) {
        return forme_cli::hostctl_data::run(&args);
    }
    if args
        .first()
        .is_some_and(|value| value == "prepare-installation")
    {
        return forme_cli::hostctl_install::run(&args[1..]);
    }
    if args.first().is_some_and(|value| value == "recover-login") {
        return recover_login(&args[1..]);
    }
    let usage = || {
        "usage: forme-hostctl bootstrap-proof --address <loopback-ip:port> --credential-file <absolute-path> --operation <stable-ref>".to_owned()
    };
    let [command, address_flag, address, credential_flag, credential, operation_flag, operation] =
        args.as_slice()
    else {
        return Err(usage());
    };
    if command != "bootstrap-proof"
        || address_flag != "--address"
        || credential_flag != "--credential-file"
        || operation_flag != "--operation"
    {
        return Err(usage());
    }
    let address: SocketAddr = address.parse().map_err(|_| usage())?;
    let credential = PathBuf::from(credential);
    let client = LocalBootstrapClient::connect(address, &credential).map_err(|e| e.to_string())?;
    let proof = client.issue_proof(operation).map_err(|e| e.to_string())?;
    // This is the intentionally local proof-delivery channel. Do not redirect
    // its output into diagnostics, a public page or a URL.
    println!(
        "{}",
        serde_json::to_string(&proof).map_err(|_| "proof response unavailable".to_owned())?
    );
    Ok(())
}

fn recover_login(args: &[String]) -> Result<(), String> {
    require_local_administrator()?;
    let [config_flag, config, operation_flag, operation, stdin_flag] = args else {
        return Err(
            "usage: forme-hostctl recover-login --web-config <absolute-path> --operation <stable-ref> --password-stdin"
                .into(),
        );
    };
    if config_flag != "--web-config"
        || operation_flag != "--operation"
        || stdin_flag != "--password-stdin"
    {
        return Err(
            "usage: forme-hostctl recover-login --web-config <absolute-path> --operation <stable-ref> --password-stdin"
                .into(),
        );
    }
    let config_path = PathBuf::from(config);
    if !config_path.is_absolute() {
        return Err("--web-config must be an absolute path".into());
    }
    let config = WebServerConfig::load(&config_path).map_err(|error| error.to_string())?;
    let mut password = Vec::new();
    io::stdin()
        .take(1025)
        .read_to_end(&mut password)
        .map_err(|_| "could not read password from stdin".to_owned())?;
    if password.len() > 1024 {
        return Err("password input is too large".into());
    }
    if password.ends_with(b"\r\n") {
        password.truncate(password.len() - 2);
    } else if password.ends_with(b"\n") {
        password.pop();
    }
    let password =
        String::from_utf8(password).map_err(|_| "password input must be valid UTF-8".to_owned())?;
    let store = WebIdentityStore::open(
        &config.identity_directory,
        &config.install_ref,
        ed25519_dalek::VerifyingKey::from_bytes(&config.claim_verifying_key)
            .map_err(|_| "web claim key is invalid".to_owned())?,
    )
    .map_err(|error| error.to_string())?;
    let revision = store
        .recover_login_with_local_authority(operation, SensitivePassword::new(password))
        .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::json!({
            "schema_version": 1,
            "operation": operation,
            "credential_revision": revision.credential_revision,
            "auth_epoch": revision.auth_epoch,
            "state": "local_recovery_committed_core_fence_pending"
        })
    );
    Ok(())
}

fn require_local_administrator() -> Result<(), String> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("whoami.exe")
            .args(["/groups"])
            .output()
            .map_err(|_| "cannot verify Windows administrator authority".to_owned())?;
        let text = String::from_utf8_lossy(&output.stdout);
        if !output.status.success()
            || !text.contains("S-1-5-32-544")
            || !(text.contains("Enabled") || text.contains("已启用"))
        {
            return Err("local administrator authority is required".into());
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        let output = std::process::Command::new("id")
            .arg("-u")
            .output()
            .map_err(|_| "cannot verify local administrator authority".to_owned())?;
        if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != "0" {
            return Err("local administrator authority is required".into());
        }
        Ok(())
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err("this platform cannot verify local administrator authority".into())
    }
}
