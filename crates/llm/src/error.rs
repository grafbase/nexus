use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

use crate::messages::anthropic;

pub type LlmResult<T> = std::result::Result<T, LlmError>;

/// LLM service errors with appropriate HTTP status codes.
#[derive(Debug, Error)]
pub enum LlmError {
    /// Invalid model format or missing provider/model in request.
    #[error("Invalid model format: expected 'provider/model', got '{0}'")]
    InvalidModelFormat(String),

    /// Provider not found in configuration.
    #[error("Provider '{0}' not found")]
    ProviderNotFound(String),

    /// Model not found at the provider.
    #[error("{0}")]
    ModelNotFound(String),

    /// Authentication failed (missing or invalid API key).
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Invalid request parameters.
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Rate limit exceeded.
    #[error("Rate limit exceeded: {message}")]
    RateLimitExceeded { message: String },

    /// Insufficient quota or credits.
    #[error("Insufficient quota: {0}")]
    InsufficientQuota(String),

    /// Streaming not supported.
    #[error("Streaming is not yet supported. Please set stream=false or omit the parameter.")]
    StreamingNotSupported,

    /// Provider API returned an error.
    #[error("Provider API error ({status}): {message}")]
    ProviderApiError { status: u16, message: String },

    /// Network or connection error.
    #[error("Connection error: {0}")]
    ConnectionError(String),

    /// Internal server error.
    /// If Some(message), it came from a provider and can be shown.
    /// If None, it's an internal Nexus error and should not leak details.
    #[error("Internal server error")]
    InternalError(Option<String>),
}

impl LlmError {
    /// Get the appropriate HTTP status code for this error.
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidModelFormat(_) | Self::InvalidRequest(_) | Self::StreamingNotSupported => {
                StatusCode::BAD_REQUEST
            }
            Self::AuthenticationFailed(_) => StatusCode::UNAUTHORIZED,
            Self::InsufficientQuota(_) => StatusCode::FORBIDDEN,
            Self::ProviderNotFound(_) | Self::ModelNotFound(_) => StatusCode::NOT_FOUND,
            Self::RateLimitExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::ConnectionError(_) => StatusCode::BAD_GATEWAY,
            Self::ProviderApiError { status, .. } => {
                // Map provider status codes to our status codes
                match *status {
                    400 => StatusCode::BAD_REQUEST,
                    401 => StatusCode::UNAUTHORIZED,
                    403 => StatusCode::FORBIDDEN,
                    404 => StatusCode::NOT_FOUND,
                    429 => StatusCode::TOO_MANY_REQUESTS,
                    500..=599 => StatusCode::BAD_GATEWAY,
                    _ => StatusCode::BAD_GATEWAY,
                }
            }
            Self::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Get the error type string for the response.
    pub fn error_type(&self) -> &str {
        match self {
            Self::InvalidModelFormat(_) | Self::InvalidRequest(_) | Self::StreamingNotSupported => {
                "invalid_request_error"
            }
            Self::AuthenticationFailed(_) => "authentication_error",
            Self::InsufficientQuota(_) => "insufficient_quota",
            Self::ProviderNotFound(_) | Self::ModelNotFound(_) => "not_found_error",
            Self::RateLimitExceeded { .. } => "rate_limit_error",
            Self::ConnectionError(_) | Self::ProviderApiError { .. } => "api_error",
            Self::InternalError(_) => "internal_error",
        }
    }

    /// Message that is safe to expose to API consumers.
    pub fn client_message(&self) -> String {
        match self {
            Self::InternalError(Some(provider_msg)) => provider_msg.clone(),
            Self::InternalError(None) => "Internal server error".to_string(),
            _ => self.to_string(),
        }
    }
}

/// Error response format compatible with OpenAI API.
#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: ErrorDetails,
}

#[derive(Debug, Serialize)]
struct ErrorDetails {
    message: String,
    r#type: String,
    code: u16,
}

impl IntoResponse for LlmError {
    fn into_response(self) -> Response {
        let status = self.status_code();

        // No Retry-After headers to maintain consistency with downstream LLM providers

        // For internal errors, only show provider messages, not Nexus internals
        let message = self.client_message();

        let error_response = ErrorResponse {
            error: ErrorDetails {
                message,
                r#type: self.error_type().to_string(),
                code: status.as_u16(),
            },
        };

        // Build response without Retry-After headers for consistency with downstream providers
        (status, Json(error_response)).into_response()
    }
}

pub struct AnthropicErrorResponse {
    status: StatusCode,
    body: anthropic::ErrorResponse,
}

impl From<LlmError> for AnthropicErrorResponse {
    fn from(error: LlmError) -> Self {
        let status = error.status_code();
        let body = anthropic::ErrorResponse::from(error);

        Self { status, body }
    }
}

impl IntoResponse for AnthropicErrorResponse {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub type AnthropicResult<T> = std::result::Result<T, AnthropicErrorResponse>;
