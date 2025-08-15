//! MCP (Multi-Client Protocol) library for routing HTTP requests to multiple MCP backends.

#![deny(missing_docs)]

mod cache;
mod config;
mod downstream;
mod http_handler;
mod index;
mod pmcp_server;
mod server;
mod server_builder;
mod types;

use axum::Router;

pub use config::{RouterConfig, RouterConfigBuilder};

/// Creates an axum router for MCP.
pub async fn router(
    RouterConfig {
        config,
        rate_limit_manager,
    }: RouterConfig,
) -> anyhow::Result<Router> {
    let mut builder = server::McpServer::builder(config.clone());

    if let Some(manager) = rate_limit_manager {
        builder = builder.rate_limit_manager(manager);
    }

    let mcp_server = builder.build().await?;
    
    // Create router with the MCP server state
    let router = Router::new()
        .route(&config.mcp.path, axum::routing::post(handle_mcp_request))
        .with_state(mcp_server);
    
    Ok(router)
}

/// Helper function to convert JSON Value to RequestId
fn convert_to_request_id(id: Option<serde_json::Value>) -> pmcp::types::RequestId {
    match id {
        Some(serde_json::Value::String(s)) => pmcp::types::RequestId::String(s),
        Some(serde_json::Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                pmcp::types::RequestId::Number(i)
            } else {
                pmcp::types::RequestId::String(n.to_string())
            }
        }
        _ => pmcp::types::RequestId::String("unknown".to_string()),
    }
}

/// Handle POST requests to the MCP endpoint
async fn handle_mcp_request(
    axum::extract::State(server): axum::extract::State<server::McpServer>,
    headers: axum::http::HeaderMap,
    body: String,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use secrecy::SecretString;
    
    log::debug!("Received MCP request: {}", body);
    log::debug!("Request headers: {:?}", headers);
    
    // Extract authentication token from headers
    let auth_token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(|token| SecretString::new(token.to_string().into()));
    
    // Parse the request (could be either JSON-RPC format or TransportMessage format)
    let request_value: serde_json::Value = match serde_json::from_str(&body) {
        Ok(val) => val,
        Err(e) => {
            let error_response = serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32700,
                    "message": format!("Parse error: {}", e)
                },
                "id": null
            });
            return (axum::http::StatusCode::BAD_REQUEST, axum::Json(error_response)).into_response();
        }
    };
    
    // Check if this is a TransportMessage format (has "request" field) or direct JSON-RPC
    let (jsonrpc_value, message_id) = if let Some(request_obj) = request_value.get("request") {
        // This is TransportMessage format from StreamableHttpTransport
        let id = request_value.get("id").cloned();
        (request_obj.clone(), id)
    } else {
        // This is direct JSON-RPC format from HttpTransport
        let id = request_value.get("id").cloned();
        (request_value, id)
    };
    
    // Check if this is a request (has id) or notification (no id)
    let is_notification = message_id.is_none();
    
    // Extract method and params from the JSON-RPC request
    if let Some(method) = jsonrpc_value.get("method").and_then(|m| m.as_str()) {
        if is_notification {
            // Handle notification (no response expected)
            log::debug!("Received notification: {}", method);
            if method == "notifications/initialized" {
                // Client has finished initialization
                // Notifications should return 202 Accepted with empty body per MCP spec
                return axum::http::StatusCode::ACCEPTED.into_response();
            }
            // For other notifications, return 202 Accepted with empty body
            return axum::http::StatusCode::ACCEPTED.into_response();
        } else {
            // Handle request (response expected)
            if method == "initialize" {
                // Handle initialize request
                let server_info = server.get_info().await;
                
                // Build initialize result response using pmcp types
                use pmcp::types::{JSONRPCResponse, jsonrpc::ResponsePayload};
                
                // Convert request_id from Value to RequestId
                let id = convert_to_request_id(message_id);
                
                let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    payload: ResponsePayload::Result(serde_json::json!({
                        "protocolVersion": server_info.protocol_version,
                        "capabilities": server_info.capabilities,
                        "serverInfo": {
                            "name": server_info.name,
                            "version": server_info.version
                        },
                        "instructions": server_info.instructions
                    })),
                };
                
                log::debug!("Sending initialize response");
                return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
            } else if method == "tools/list" {
                // Handle tools/list request
                use pmcp::types::{JSONRPCResponse, jsonrpc::ResponsePayload};
                
                let id = convert_to_request_id(message_id);
                
                match server.list_tools().await {
                    Ok(result) => {
                        let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            payload: ResponsePayload::Result(serde_json::to_value(result).unwrap_or_default()),
                        };
                        return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                    }
                    Err(e) => {
                        let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            payload: ResponsePayload::Error(pmcp::types::jsonrpc::JSONRPCError {
                                code: -32603,
                                message: e.to_string(),
                                data: None,
                            }),
                        };
                        return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                    }
                }
            } else if method == "tools/call" {
                // Handle tools/call request
                use pmcp::types::{JSONRPCResponse, jsonrpc::ResponsePayload};
                
                let id = convert_to_request_id(message_id);
                let params = jsonrpc_value.get("params").cloned().unwrap_or_default();
                
                // Parse the call tool request
                match serde_json::from_value::<crate::types::CallToolRequest>(params) {
                    Ok(request) => {
                        match server.call_tool(request, auth_token.as_ref()).await {
                            Ok(result) => {
                                let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id,
                                    payload: ResponsePayload::Result(serde_json::to_value(result).unwrap_or_default()),
                                };
                                return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                            }
                            Err(e) => {
                                let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id,
                                    payload: ResponsePayload::Error(pmcp::types::jsonrpc::JSONRPCError {
                                        code: -32603,
                                        message: e.to_string(),
                                        data: None,
                                    }),
                                };
                                return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                            }
                        }
                    }
                    Err(e) => {
                        let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            payload: ResponsePayload::Error(pmcp::types::jsonrpc::JSONRPCError {
                                code: -32602,
                                message: format!("Invalid parameters: {}", e),
                                data: None,
                            }),
                        };
                        return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                    }
                }
            } else if method == "prompts/list" {
                // Handle prompts/list request
                use pmcp::types::{JSONRPCResponse, jsonrpc::ResponsePayload};
                
                let id = convert_to_request_id(message_id);
                
                match server.list_prompts(auth_token.as_ref()).await {
                    Ok(result) => {
                        let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            payload: ResponsePayload::Result(serde_json::to_value(result).unwrap_or_default()),
                        };
                        return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                    }
                    Err(e) => {
                        let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            payload: ResponsePayload::Error(pmcp::types::jsonrpc::JSONRPCError {
                                code: -32603,
                                message: e.to_string(),
                                data: None,
                            }),
                        };
                        return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                    }
                }
            } else if method == "prompts/get" {
                // Handle prompts/get request
                use pmcp::types::{JSONRPCResponse, jsonrpc::ResponsePayload};
                
                let id = convert_to_request_id(message_id);
                let params = jsonrpc_value.get("params").cloned().unwrap_or_default();
                
                match serde_json::from_value::<crate::types::GetPromptRequest>(params) {
                    Ok(request) => {
                        match server.get_prompt(request, auth_token.as_ref()).await {
                            Ok(result) => {
                                let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id,
                                    payload: ResponsePayload::Result(serde_json::to_value(result).unwrap_or_default()),
                                };
                                return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                            }
                            Err(e) => {
                                let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id,
                                    payload: ResponsePayload::Error(pmcp::types::jsonrpc::JSONRPCError {
                                        code: -32603,
                                        message: e.to_string(),
                                        data: None,
                                    }),
                                };
                                return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                            }
                        }
                    }
                    Err(e) => {
                        let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            payload: ResponsePayload::Error(pmcp::types::jsonrpc::JSONRPCError {
                                code: -32602,
                                message: format!("Invalid parameters: {}", e),
                                data: None,
                            }),
                        };
                        return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                    }
                }
            } else if method == "resources/list" {
                // Handle resources/list request
                use pmcp::types::{JSONRPCResponse, jsonrpc::ResponsePayload};
                
                let id = convert_to_request_id(message_id);
                
                match server.list_resources(auth_token.as_ref()).await {
                    Ok(result) => {
                        let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            payload: ResponsePayload::Result(serde_json::to_value(result).unwrap_or_default()),
                        };
                        return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                    }
                    Err(e) => {
                        let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            payload: ResponsePayload::Error(pmcp::types::jsonrpc::JSONRPCError {
                                code: -32603,
                                message: e.to_string(),
                                data: None,
                            }),
                        };
                        return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                    }
                }
            } else if method == "resources/read" {
                // Handle resources/read request
                use pmcp::types::{JSONRPCResponse, jsonrpc::ResponsePayload};
                
                let id = convert_to_request_id(message_id);
                let params = jsonrpc_value.get("params").cloned().unwrap_or_default();
                
                match serde_json::from_value::<crate::types::ReadResourceRequest>(params) {
                    Ok(request) => {
                        match server.read_resource(request, auth_token.as_ref()).await {
                            Ok(result) => {
                                let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id,
                                    payload: ResponsePayload::Result(serde_json::to_value(result).unwrap_or_default()),
                                };
                                return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                            }
                            Err(e) => {
                                let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id,
                                    payload: ResponsePayload::Error(pmcp::types::jsonrpc::JSONRPCError {
                                        code: -32603,
                                        message: e.to_string(),
                                        data: None,
                                    }),
                                };
                                return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                            }
                        }
                    }
                    Err(e) => {
                        let json_rpc_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            payload: ResponsePayload::Error(pmcp::types::jsonrpc::JSONRPCError {
                                code: -32602,
                                message: format!("Invalid parameters: {}", e),
                                data: None,
                            }),
                        };
                        return (axum::http::StatusCode::OK, axum::Json(json_rpc_response)).into_response();
                    }
                }
            }
        }
    }
    
    // For now, return not implemented for other methods
    use pmcp::types::{JSONRPCResponse, jsonrpc::{ResponsePayload, JSONRPCError}};
    
    // Convert ID from Value to RequestId
    let error_id = convert_to_request_id(jsonrpc_value.get("id").cloned());
    
    let error_response: JSONRPCResponse<serde_json::Value, pmcp::types::jsonrpc::JSONRPCError> = JSONRPCResponse {
        jsonrpc: "2.0".to_string(),
        id: error_id,
        payload: ResponsePayload::Error(JSONRPCError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    };
    
    (axum::http::StatusCode::OK, axum::Json(error_response)).into_response()
}
