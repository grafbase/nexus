#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Internal server error")]
    Internal,
    #[error("Invalid token: {0}")]
    InvalidToken(&'static str),
}
