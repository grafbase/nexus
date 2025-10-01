#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Internal server error")]
    Internal,
}
