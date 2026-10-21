#![forbid(unsafe_code)]

use forme_gateway::web::{http, WebServerConfig};
use std::io::Write;

#[cfg(windows)]
#[path = "../service/web.rs"]
mod service;

#[tokio::main]
async fn main() {
    #[cfg(windows)]
    {
        match service::administrative_command() {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                eprintln!("forme-webd: {error}");
                std::process::exit(1);
            }
        }
        match service::dispatch() {
            Ok(()) => return,
            Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1063) => {}
            Err(error) => {
                eprintln!("forme-webd: Windows service dispatcher failed: {error}");
                std::process::exit(1);
            }
        }
    }
    if let Err(error) = run().await {
        eprintln!("forme-webd: {error}");
        std::process::exit(1);
    }
}

async fn run() -> forme_protocol::Result<()> {
    let path = config_path(false)?;
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

fn config_path(required: bool) -> forme_protocol::Result<std::path::PathBuf> {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref() {
        Some(value) if value == std::ffi::OsStr::new("--config") => {
            let path = args.next().map(std::path::PathBuf::from).ok_or_else(|| {
                forme_protocol::Error("Web service configuration path is required".into())
            })?;
            if args.next().is_some() {
                return Err(forme_protocol::Error(
                    "unknown Web service arguments".into(),
                ));
            }
            Ok(path)
        }
        None if !required => std::env::var_os("FORME_WEB_CONFIG_PATH")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| {
                forme_protocol::Error(
                    "usage: forme-webd --config <absolute-managed-web-config>".into(),
                )
            }),
        _ => Err(forme_protocol::Error(
            "unknown Web service arguments".into(),
        )),
    }
}
