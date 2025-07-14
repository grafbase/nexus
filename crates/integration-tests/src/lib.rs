use std::net::SocketAddr;
use std::sync::Once;
use std::time::Duration;

use config::Config;
use server::ServeConfig;
use tokio::net::TcpListener;
use tokio::time::timeout;

static INIT: Once = Once::new();

fn init_crypto_provider() {
    INIT.call_once(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Failed to install default crypto provider");
    });
}

/// Test client for making HTTP requests to the test server
pub struct TestClient {
    base_url: String,
    client: reqwest::Client,
}

impl TestClient {
    /// Create a new test client for the given base URL
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
        }
    }

    /// Create a new test client that accepts invalid TLS certificates
    pub fn new_with_tls(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("Failed to create client with invalid cert acceptance");

        Self { base_url, client }
    }

    /// Send a POST request to the given path with JSON body
    pub async fn post<T: serde::Serialize>(&self, path: &str, body: &T) -> reqwest::Result<reqwest::Response> {
        self.client
            .post(format!("{}{}", self.base_url, path))
            .json(body)
            .send()
            .await
    }

    /// Send a GET request to the given path
    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
            .unwrap()
    }
}

/// Test server that manages the lifecycle of a server instance
pub struct TestServer {
    pub client: TestClient,
    pub address: SocketAddr,
    _handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    /// Start a new test server with the given TOML configuration
    pub async fn start(config_toml: &str) -> Self {
        // Initialize crypto provider for rustls
        init_crypto_provider();

        // Parse the configuration from TOML
        let config: Config = toml::from_str(config_toml).unwrap();

        // Find an available port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        // Check if TLS is configured before moving config into spawn task
        let has_tls = config.server.tls.is_some();

        // Create the server configuration
        let serve_config = ServeConfig {
            listen_address: address,
            config,
        };

        // Start the server in a background task
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            // Drop the listener so the server can bind to the address
            drop(listener);

            match server::serve(serve_config).await {
                Ok(()) => {
                    let _ = tx.send(Ok(()));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });

        // Wait for the server to start up or fail
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check if the server failed to start (non-blocking check)
        if let Ok(Err(e)) = rx.try_recv() {
            eprintln!("Server failed to start: {e}");
            std::process::exit(1);
        }

        // Create the test client - use HTTPS if TLS is configured
        let protocol = if has_tls { "https" } else { "http" };
        let base_url = format!("{protocol}://{address}");

        let client = if has_tls {
            TestClient::new_with_tls(base_url)
        } else {
            TestClient::new(base_url)
        };

        // Verify the server is actually running by making a simple request
        let mut retries = 10;
        while retries > 0 {
            if timeout(Duration::from_millis(100), client.get("/")).await.is_ok() {
                break;
            }
            retries -= 1;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        TestServer {
            client,
            address,
            _handle: handle,
        }
    }
}
