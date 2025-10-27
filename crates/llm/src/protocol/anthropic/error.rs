use serde::{Deserialize, Serialize};

use crate::protocol::unknown_fields::UnknownFields;

pub const ERROR_TYPE_INVALID_REQUEST: &str = "invalid_request_error";
pub const ERROR_TYPE_AUTHENTICATION: &str = "authentication_error";
pub const ERROR_TYPE_BILLING: &str = "billing_error";
pub const ERROR_TYPE_PERMISSION: &str = "permission_error";
pub const ERROR_TYPE_NOT_FOUND: &str = "not_found_error";
pub const ERROR_TYPE_RATE_LIMIT: &str = "rate_limit_error";
pub const ERROR_TYPE_TIMEOUT: &str = "timeout_error";
pub const ERROR_TYPE_API: &str = "api_error";
pub const ERROR_TYPE_OVERLOADED: &str = "overloaded_error";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    #[serde(default)]
    pub request_id: Option<String>,

    /// Error details
    pub error: Error,
}

/// Anthropic error payload surfaced for 4XX responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    /// The type of error that occurred.
    #[serde(rename = "type")]
    pub r#type: String,

    /// Human-readable error explanation.
    pub message: String,

    /// Additional metadata returned by Anthropic that Nexus does not model yet.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

impl crate::telemetry::Error for Error {
    fn error_type(&self) -> &str {
        self.r#type.as_str()
    }
}

impl crate::telemetry::Error for (http::StatusCode, Error) {
    fn error_type(&self) -> &str {
        self.1.r#type.as_str()
    }
}

#[allow(dead_code)]
impl Error {
    fn new(r#type: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            r#type: r#type.into(),
            message: message.into(),
            unknown_fields: UnknownFields::default(),
        }
    }

    pub fn invalid_request_error(message: impl Into<String>) -> Self {
        Self::new(ERROR_TYPE_INVALID_REQUEST, message)
    }

    pub fn authentication_error(message: impl Into<String>) -> Self {
        Self::new(ERROR_TYPE_AUTHENTICATION, message)
    }

    pub fn billing_error(message: impl Into<String>) -> Self {
        Self::new(ERROR_TYPE_BILLING, message)
    }

    pub fn permission_error(message: impl Into<String>) -> Self {
        Self::new(ERROR_TYPE_PERMISSION, message)
    }

    pub fn not_found_error(message: impl Into<String>) -> Self {
        Self::new(ERROR_TYPE_NOT_FOUND, message)
    }

    pub fn rate_limit_error(message: impl Into<String>) -> Self {
        Self::new(ERROR_TYPE_RATE_LIMIT, message)
    }

    pub fn timeout_error(message: impl Into<String>) -> Self {
        Self::new(ERROR_TYPE_TIMEOUT, message)
    }

    pub fn api_error(message: impl Into<String>) -> Self {
        Self::new(ERROR_TYPE_API, message)
    }

    pub fn overloaded_error(message: impl Into<String>) -> Self {
        Self::new(ERROR_TYPE_OVERLOADED, message)
    }
}
