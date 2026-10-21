#![forbid(unsafe_code)]

use forme_gateway::web::{http, WebServerConfig};
use std::io::Write;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("forme-webd: {error}");
        std::process::exit(1);
    }
}

async fn run() -> forme_protocol::Result<()> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--config")) {
        return Err(forme_protocol::Error(
            "usage: forme-webd --config <absolute-managed-web-config>".into(),
        ));
    }
    let path = args.next().map(std::path::PathBuf::from).ok_or_else(|| {
        forme_protocol::Error("Web service configuration path is required".into())
    })?;
    if args.next().is_some() {
        return Err(forme_protocol::Error(
            "unknown Web service arguments".into(),
        ));
    }
    let config = WebServerConfig::load(&path)?;
    http::serve_until(
        config,
        |address| {
            println!("FORME_WEB_URL=http://{address}");
            std::io::stdout()
                .flush()
                .map_err(|_| forme_protocol::Error("Web startup notification unavailable".into()))
        },
        http::shutdown_signal(),
    )
    .await
}
