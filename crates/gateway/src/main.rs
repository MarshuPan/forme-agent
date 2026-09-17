#![forbid(unsafe_code)]

use std::io::Write;

use forme_gateway::{serve_environment, GatewayServerConfig};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("forme-gatewayd: {error}");
        std::process::exit(1);
    }
}

async fn run() -> forme_protocol::Result<()> {
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
