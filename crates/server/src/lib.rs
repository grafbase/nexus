mod error;

use std::net::SocketAddr;

use axum::{Router, response::Html, routing::get};
use axum_server::tls_rustls::RustlsConfig;
use config::Config;
use tokio::net::TcpListener;

pub(crate) type Result<T> = std::result::Result<T, error::Error>;

pub struct ServeConfig {
    pub listen_address: SocketAddr,
    pub config: Config,
}

pub async fn serve(ServeConfig { listen_address, config }: ServeConfig) -> crate::Result<()> {
    // Create the router with the MCP endpoint
    let mut app = Router::new();

    // Add the MCP endpoint if enabled
    if config.mcp.enabled {
        app = app.route(&config.mcp.path, get(hello_world));
    }

    // Create TCP listener
    let listener = TcpListener::bind(listen_address).await.map_err(error::Error::Bind)?;

    match &config.server.tls {
        Some(tls_config) => {
            // Setup TLS
            let rustls_config = RustlsConfig::from_pem_file(&tls_config.certificate, &tls_config.key)
                .await
                .map_err(|e| error::Error::Tls(e.to_string()))?;

            if config.mcp.enabled {
                log::info!("MCP endpoint available at: https://{listen_address}{}", config.mcp.path);
            }

            // Convert tokio listener to std listener for axum-server
            let std_listener = listener.into_std().map_err(error::Error::Bind)?;

            // Start the HTTPS server
            axum_server::from_tcp_rustls(std_listener, rustls_config)
                .serve(app.into_make_service())
                .await
                .map_err(|e| error::Error::Server(std::io::Error::other(e)))?;
        }
        None => {
            if config.mcp.enabled {
                log::info!("MCP endpoint available at: http://{listen_address}{}", config.mcp.path);
            }

            // Start the HTTP server
            axum::serve(listener, app).await.map_err(error::Error::Server)?;
        }
    }

    Ok(())
}

async fn hello_world() -> Html<&'static str> {
    Html("<h1>Hello, World!</h1>")
}
