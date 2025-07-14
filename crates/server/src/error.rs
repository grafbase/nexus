#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to bind to address: {0}")]
    Bind(#[source] std::io::Error),

    #[error("Server error: {0}")]
    Server(#[source] std::io::Error),

    #[error("TLS error: {0}")]
    Tls(String),
}
