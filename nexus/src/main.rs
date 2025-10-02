use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use args::Args;
use clap::Parser;
use config::Config;
use server::ServeConfig;
use tokio_util::sync::CancellationToken;

mod args;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = args.config()?;

    // The server crate will handle telemetry and logger initialization

    // Create a cancellation token for graceful shutdown
    let shutdown_signal = CancellationToken::new();
    let shutdown_signal_clone = shutdown_signal.clone();

    // Spawn a task to listen for shutdown signals
    tokio::spawn(async move {
        shutdown_signal_handler().await;
        log::info!("Shutdown signal received");
        shutdown_signal_clone.cancel();
    });

    if let Err(e) = server::serve(serve_config(&args, config, shutdown_signal)).await {
        log::error!("Server failed to start: {e}");
        std::process::exit(1);
    }

    log::info!("Server shut down gracefully");
    Ok(())
}

async fn shutdown_signal_handler() {
    // Wait for CTRL+C
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    // Also listen for SIGTERM on Unix
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

fn serve_config(args: &Args, config: Config, shutdown_signal: CancellationToken) -> ServeConfig {
    let listen_address = args
        .listen_address
        .or(config.server.listen_address)
        .unwrap_or(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8000)));

    // Convert the log level to an env filter string
    let log_filter = args.log_level.env_filter();

    ServeConfig {
        listen_address,
        config,
        shutdown_signal,
        log_filter,
        version: env!("CARGO_PKG_VERSION").to_string(),
        bound_addr_sender: None,
    }
}
