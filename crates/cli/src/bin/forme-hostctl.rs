//! Local operator entry point, not an Agent or a second brain.
#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;

use forme_gateway::web::bootstrap::LocalBootstrapClient;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme-hostctl: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
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
