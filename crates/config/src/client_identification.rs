//! Client identification configuration.

use std::collections::BTreeSet;

use serde::Deserialize;

/// Identification source - either JWT claim or HTTP header.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum IdentificationSource {
    /// Extract from JWT claim.
    JwtClaim {
        /// JWT claim path (e.g., "sub", "plan").
        jwt_claim: String,
    },
    /// Extract from HTTP header.
    HttpHeader {
        /// HTTP header name (e.g., "X-Client-Id", "X-Group-Id").
        http_header: String,
    },
}

impl Default for IdentificationSource {
    fn default() -> Self {
        Self::JwtClaim {
            jwt_claim: "sub".to_string(),
        }
    }
}

/// Client identification extraction configuration.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ClientIdentificationConfig {
    /// Whether client identification is enabled.
    pub enabled: bool,

    /// Validation settings for client identification.
    pub validation: ClientIdentificationValidation,

    /// Client ID extraction source.
    pub client_id: IdentificationSource,

    /// Group ID extraction source.
    pub group_id: Option<IdentificationSource>,
}

/// Validation settings for client identification.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ClientIdentificationValidation {
    /// List of valid group values. All group names in rate limits must be from this list.
    #[serde(default)]
    pub group_values: BTreeSet<String>,
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use insta::assert_debug_snapshot;

    use crate::Config;

    #[test]
    fn client_identification_config() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.jwt_claim = "sub"
            group_id.jwt_claim = "plan"

            [server.client_identification.validation]
            group_values = ["free", "pro", "enterprise"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config.server.client_identification, @r#"
        ClientIdentificationConfig {
            enabled: true,
            validation: ClientIdentificationValidation {
                group_values: {
                    "enterprise",
                    "free",
                    "pro",
                },
            },
            client_id: JwtClaim {
                jwt_claim: "sub",
            },
            group_id: Some(
                JwtClaim {
                    jwt_claim: "plan",
                },
            ),
        }
        "#);
    }

    #[test]
    fn client_identification_http_headers() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.http_header = "X-Client-Id"
            group_id.http_header = "X-Plan"

            [server.client_identification.validation]
            group_values = ["basic", "premium"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config.server.client_identification, @r#"
        ClientIdentificationConfig {
            enabled: true,
            validation: ClientIdentificationValidation {
                group_values: {
                    "basic",
                    "premium",
                },
            },
            client_id: HttpHeader {
                http_header: "X-Client-Id",
            },
            group_id: Some(
                HttpHeader {
                    http_header: "X-Plan",
                },
            ),
        }
        "#);
    }
}
