#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to bind to address: {0}")]
    BindError(#[source] std::io::Error),

    #[error("Server error: {0}")]
    ServerError(#[source] std::io::Error),

    #[error("TLS error: {0}")]
    TlsError(String),
}
