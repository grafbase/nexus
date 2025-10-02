mod middleware;

pub use config::ClientIdentity;
use config::{ClientIdentificationConfig, IdentificationSource};
use context::{Authentication, NexusToken};
use http::Request;
pub use middleware::ClientIdentificationLayer;

/// Errors that can occur during client identification extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientIdentificationError {
    /// The client is in a group that's not in the allowed list
    UnauthorizedGroup {
        /// The group the client belongs to
        group: String,
        /// The list of allowed groups
        allowed_groups: Vec<String>,
    },
    /// Client identification is required but not found
    MissingIdentification,
}

impl std::fmt::Display for ClientIdentificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnauthorizedGroup { group, allowed_groups } => {
                write!(
                    f,
                    "Client is in group '{}' which is not in allowed groups: {:?}",
                    group, allowed_groups
                )
            }
            Self::MissingIdentification => {
                write!(f, "Client identification is required but not provided")
            }
        }
    }
}

impl std::error::Error for ClientIdentificationError {}

/// Extract client identity from a request based on configuration.
///
/// This function attempts to extract client identification from either:
/// 1. JWT claims (if a valid token is present)
/// 2. HTTP headers
///
/// The extraction method and field names are determined by the configuration.
///
/// Returns:
/// - `Ok(Some(identity))` if identity was successfully extracted
/// - `Ok(None)` if client identification is not enabled
/// - `Err(...)` if identification is required but invalid or missing
pub fn extract_client_identity<B>(
    req: &Request<B>,
    config: &ClientIdentificationConfig,
) -> Result<Option<ClientIdentity>, ClientIdentificationError> {
    if !config.enabled {
        return Ok(None);
    }

    // Extract JWT token once if any source uses it
    let jwt_token = req
        .extensions()
        .get::<Authentication>()
        .and_then(|auth| auth.nexus.as_ref());

    // Extract client ID (always configured when enabled)
    let client_id = extract_from_source_with_token(req.headers(), jwt_token, &config.client_id)
        .ok_or(ClientIdentificationError::MissingIdentification)?;

    // Extract group if configured
    let group = config
        .group_id
        .as_ref()
        .and_then(|source| extract_from_source_with_token(req.headers(), jwt_token, source));

    // Validate group against allowed list
    validate_group(&group, config)?;

    Ok(Some(ClientIdentity { client_id, group }))
}

/// Validate that a group is in the allowed list if restrictions are configured.
fn validate_group(
    group: &Option<String>,
    config: &ClientIdentificationConfig,
) -> Result<(), ClientIdentificationError> {
    // No validation needed if no group_values are configured
    if config.validation.group_values.is_empty() {
        return Ok(());
    }

    // If no group was provided, that's OK - they'll use default rate limits
    // Only validate if a group was actually provided
    if let Some(g) = group {
        // The provided group must be in the allowed list
        if !config.validation.group_values.contains(g) {
            return Err(ClientIdentificationError::UnauthorizedGroup {
                group: g.clone(),
                allowed_groups: config.validation.group_values.iter().cloned().collect(),
            });
        }
    }

    Ok(())
}

/// Extract a value from either JWT claims or HTTP headers based on the source configuration.
/// Takes the JWT token as a parameter to avoid repeated lookups.
fn extract_from_source_with_token(
    headers: &http::HeaderMap,
    jwt_token: Option<&NexusToken>,
    source: &IdentificationSource,
) -> Option<String> {
    match source {
        // Extract a value from JWT claims.
        IdentificationSource::JwtClaim { jwt_claim } => {
            let token = jwt_token?;
            let claims = token.claims();

            // Use the get_claim method to extract the value
            claims.custom.get_claim(jwt_claim)
        }
        IdentificationSource::HttpHeader { http_header } => headers
            .get(http_header)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::{ClientIdentificationConfig, ClientIdentificationValidation, IdentificationSource};
    use std::collections::BTreeSet;

    fn create_config(enabled: bool) -> ClientIdentificationConfig {
        ClientIdentificationConfig {
            enabled,
            validation: ClientIdentificationValidation {
                group_values: BTreeSet::new(),
            },
            client_id: IdentificationSource::default(),
            group_id: None,
        }
    }

    #[test]
    fn disabled_config_returns_none() {
        let config = create_config(false);
        let req = Request::builder().body(()).unwrap();

        assert_eq!(extract_client_identity(&req, &config), Ok(None));
    }

    #[test]
    fn missing_client_id_returns_error() {
        let config = create_config(true);
        let req = Request::builder().body(()).unwrap();

        // When identification is enabled but client_id is not provided, returns error
        assert_eq!(
            extract_client_identity(&req, &config),
            Err(ClientIdentificationError::MissingIdentification)
        );
    }

    #[test]
    fn extract_from_http_headers() {
        let mut config = create_config(true);

        config.client_id = IdentificationSource::HttpHeader {
            http_header: "X-Client-Id".to_string(),
        };

        config.group_id = Some(IdentificationSource::HttpHeader {
            http_header: "X-Group".to_string(),
        });

        let req = Request::builder()
            .header("X-Client-Id", "user123")
            .header("X-Group", "pro")
            .body(())
            .unwrap();

        let identity = extract_client_identity(&req, &config).unwrap().unwrap();

        assert_eq!(identity.client_id, "user123");
        assert_eq!(identity.group, Some("pro".to_string()));
    }

    #[test]
    fn validates_group_against_allowed_list() {
        let mut config = create_config(true);

        config.client_id = IdentificationSource::HttpHeader {
            http_header: "X-Client-Id".to_string(),
        };

        config.group_id = Some(IdentificationSource::HttpHeader {
            http_header: "X-Group".to_string(),
        });

        config.validation.group_values = ["free", "pro"].iter().map(|s| s.to_string()).collect();

        // Valid group
        let req = Request::builder()
            .header("X-Client-Id", "user123")
            .header("X-Group", "pro")
            .body(())
            .unwrap();

        let identity = extract_client_identity(&req, &config).unwrap().unwrap();
        assert_eq!(identity.group, Some("pro".to_string()));

        // Invalid group - should return error
        let req = Request::builder()
            .header("X-Client-Id", "user123")
            .header("X-Group", "enterprise")
            .body(())
            .unwrap();

        let result = extract_client_identity(&req, &config);

        assert_eq!(
            result,
            Err(ClientIdentificationError::UnauthorizedGroup {
                group: "enterprise".to_string(),
                allowed_groups: vec!["free".to_string(), "pro".to_string()],
            })
        );
    }

    #[test]
    fn missing_header_returns_error() {
        let mut config = create_config(true);

        config.client_id = IdentificationSource::HttpHeader {
            http_header: "X-Client-Id".to_string(),
        };

        let req = Request::builder().body(()).unwrap();

        assert_eq!(
            extract_client_identity(&req, &config),
            Err(ClientIdentificationError::MissingIdentification)
        );
    }
}
