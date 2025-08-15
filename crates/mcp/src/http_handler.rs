//! HTTP handler for MCP protocol
//!
//! This module implements proper MCP protocol handling for HTTP requests using pmcp.

use config::Config;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::post,
    Router,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Server state for pmcp integration
#[derive(Clone)]
pub struct PmcpServerState {
    pub server: Arc<Mutex<pmcp::Server>>,
}

/// Setup routes for MCP protocol using pmcp
pub async fn setup_routes(
    config: &Config,
    pmcp_server: pmcp::Server,
) -> anyhow::Result<Router> {
    let state = PmcpServerState {
        server: Arc::new(Mutex::new(pmcp_server)),
    };

    let router = Router::new()
        .route(&config.mcp.path, post(handle_pmcp_post))
        .with_state(state);

    Ok(router)
}

/// Handle HTTP POST requests using pmcp
pub async fn handle_pmcp_post(
    State(state): State<PmcpServerState>,
    body: String,
) -> axum::response::Response {
    log::debug!("Handling PMCP HTTP POST request");
    
    // Parse the raw JSON-RPC request
    let json_request: Value = match serde_json::from_str(&body) {
        Ok(msg) => msg,
        Err(e) => {
            log::error!("Failed to parse JSON request: {}", e);
            return create_error_response(StatusCode::BAD_REQUEST, -32700, &format!("Parse error: {}", e));
        }
    };
    
    // Extract the method and params from the JSON-RPC request
    let method = json_request.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = json_request.get("params").cloned().unwrap_or(Value::Null);
    let id = json_request.get("id").cloned().unwrap_or(Value::Null);
    
    log::debug!("Received JSON-RPC method: {}", method);
    
    // Handle the JSON-RPC method directly
    match handle_jsonrpc_method(&state.server, method, params, &id).await {
        Ok(response) => {
            // For notifications (methods starting with 'notifications/'), return 200 OK with no body
            if method.starts_with("notifications/") {
                // JSON-RPC notifications should not have a response body
                (StatusCode::OK, "").into_response()
            } else {
                // Create a proper JSON response that matches the JSON-RPC format
                let json_response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "result": response,
                    "id": id
                });
                (StatusCode::OK, Json(json_response)).into_response()
            }
        },
        Err(e) => {
            log::error!("Failed to handle JSON-RPC method '{}': {}", method, e);
            create_error_response(StatusCode::INTERNAL_SERVER_ERROR, -32603, &format!("Internal error: {}", e))
        }
    }
}

/// Handle JSON-RPC method directly
async fn handle_jsonrpc_method(
    _server: &Arc<Mutex<pmcp::Server>>,
    method: &str,
    _params: Value,
    _id: &Value,
) -> anyhow::Result<Value> {
    log::debug!("Handling JSON-RPC method: {}", method);
    
    match method {
        "initialize" => {
            // Handle initialization
            let result = serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": true
                    }
                },
                "serverInfo": {
                    "name": "nexus-mcp-server", 
                    "version": "0.3.0"
                }
            });
            Ok(result)
        }
        "notifications/initialized" => {
            // This is a notification that the client has finished initialization
            // We don't need to return anything for notifications, they are fire-and-forget
            log::debug!("Client initialization complete");
            Ok(serde_json::Value::Null)
        }
        "tools/list" => {
            // Return the standard Nexus tools (search and execute)
            let tools = serde_json::json!([
                {
                    "name": "search",
                    "description": "Search for relevant tools using keywords",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "keywords": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": "Keywords to search for"
                            }
                        },
                        "required": ["keywords"]
                    }
                },
                {
                    "name": "execute", 
                    "description": "Execute a tool with the given parameters",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "Name of the tool to execute"},
                            "arguments": {"type": "object", "description": "Arguments to pass to the tool"}
                        },
                        "required": ["name", "arguments"]
                    }
                }
            ]);
            Ok(serde_json::json!({"tools": tools}))
        }
        _ => {
            Err(anyhow::anyhow!("Method '{}' not implemented", method))
        }
    }
}


/// Create a JSON-RPC error response
fn create_error_response(status: StatusCode, code: i32, message: &str) -> axum::response::Response {
    let error_body = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": code,
            "message": message
        },
        "id": serde_json::Value::Null
    });

    (status, Json(error_body)).into_response()
}