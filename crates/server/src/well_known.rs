use axum::Json;
use config::OauthConfig;
use http::StatusCode;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct OAuthProtectedResourceMetadata {
    /// The resource server's issuer identifier
    resource: String,

    /// Array of OAuth 2.0 authorization server issuer identifiers
    authorization_servers: Vec<String>,

    /// Array of supported OAuth 2.0 scopes
    #[serde(skip_serializing_if = "Option::is_none")]
    scopes_supported: Option<Vec<String>>,
}

/// Handles the OAuth 2.0 Protected Resource Metadata endpoint
pub async fn oauth_metadata(config: OauthConfig) -> (StatusCode, Json<OAuthProtectedResourceMetadata>) {
    let authorization_servers = config
        .protected_resource
        .authorization_servers
        .iter()
        .map(|url| url.to_string())
        .collect();

    let metadata = OAuthProtectedResourceMetadata {
        resource: config.protected_resource.resource.to_string(),
        authorization_servers,
        scopes_supported: config.protected_resource.scopes_supported.clone(),
    };

    (StatusCode::OK, Json(metadata))
}
