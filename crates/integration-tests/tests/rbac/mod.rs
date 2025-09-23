//! Group-based access control tests for MCP servers
//!
//! This module contains tests for group-based access control functionality,
//! covering both static and dynamic MCP servers, token forwarding, and search capabilities.

use crate::oauth2::HydraClient;
use indoc::indoc;
use integration_tests::TestServer;

mod execute;
mod search_access;
mod token_forwarding;

/// Setup helper for OAuth2 tests with custom config
/// Combines the provided config with OAuth2 setup and returns (server, access_token)
pub async fn setup_oauth2_with_config(custom_config: &str) -> Result<(TestServer, String), Box<dyn std::error::Error>> {
    // Set up Hydra and get a real OAuth2 token
    let hydra = HydraClient::new(4444, 4445);
    hydra.wait_for_hydra().await?;

    let client_id = "shared-test-client-universal";
    let client_secret = format!("{client_id}-secret");
    let token_response = hydra.get_token(client_id, &client_secret).await?;

    // OAuth2 config that can be combined with any custom config
    let oauth_config = indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"
        expected_issuer = "http://127.0.0.1:4444"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]
    "#};

    // Combine OAuth2 config with custom config
    let combined_config = format!("{}\n\n{}", oauth_config, custom_config);

    let server = TestServer::builder().build(&combined_config).await;

    Ok((server, token_response.access_token))
}
