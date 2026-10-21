#![forbid(unsafe_code)]

use std::io::Write;

use forme_gateway::{
    serve_environment, serve_managed_until, CoreServiceConfig, GatewayServerConfig,
};

#[cfg(windows)]
mod service;

#[tokio::main]
async fn main() {
    #[cfg(windows)]
    {
        match service::administrative_command() {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                eprintln!("forme-gatewayd: {error}");
                std::process::exit(1);
            }
        }
        match service::dispatch() {
            Ok(()) => return,
            Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1063) => {}
            Err(error) => {
                eprintln!("forme-gatewayd: Windows service dispatcher failed: {error}");
                std::process::exit(1);
            }
        }
    }
    if let Err(error) = run().await {
        eprintln!("forme-gatewayd: {error}");
        std::process::exit(1);
    }
}

async fn run() -> forme_protocol::Result<()> {
    if let Some(path) = configuration_path(false)? {
        return serve_managed_until(
            CoreServiceConfig::load(&path)?,
            |address| {
                println!("FORME_GATEWAY_URL=http://{address}");
                std::io::stdout().flush().map_err(|_| {
                    forme_protocol::Error("core startup notification unavailable".into())
                })
            },
            shutdown_signal(),
        )
        .await;
    }
    let config = GatewayServerConfig::from_environment()?;
    let token_path = config.token_path().map(|path| path.display().to_string());
    serve_environment(config, move |address| {
        println!("FORME_GATEWAY_URL=http://{address}");
        if let Some(path) = &token_path {
            println!("FORME_GATEWAY_TOKEN_PATH={path}");
        }
        let _ = std::io::stdout().flush();
    })
    .await
}

fn configuration_path(required: bool) -> forme_protocol::Result<Option<std::path::PathBuf>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [] if !required => Ok(None),
        [flag, path] if flag == "--config" => Ok(Some(path.into())),
        _ => Err(forme_protocol::Error(
            "usage: forme-gatewayd --config <absolute-managed-core-config>".into(),
        )),
    }
}

async fn shutdown_signal() -> forme_protocol::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .map_err(|_| forme_protocol::Error("core termination signal unavailable".into()))?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.map_err(|_| forme_protocol::Error("core interrupt signal unavailable".into())),
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .map_err(|_| forme_protocol::Error("core interrupt signal unavailable".into()))
}
