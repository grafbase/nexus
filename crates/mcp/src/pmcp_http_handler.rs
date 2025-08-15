//! PMCP HTTP handler for MCP protocol requests
//!
//! This module provides HTTP handlers that use pmcp's server implementation
//! instead of manual JSON-RPC handling.

use crate::server::McpServer;
use crate::pmcp_server::PmcpServerAdapter;
use axum::{extract::State, http::StatusCode, response::Json};
use pmcp::Server;
use std::sync::Arc;

/// HTTP handler state containing the pmcp server
pub struct PmcpHttpState {
    pub pmcp_server: Arc<Server>,
}

impl PmcpHttpState {
    pub fn new(mcp_server: McpServer) -> anyhow::Result<Self> {
        let adapter = PmcpServerAdapter::new(mcp_server);
        let pmcp_server = adapter.build_server()?;
        
        Ok(Self {
            pmcp_server: Arc::new(pmcp_server),
        })
    }
}

/// Handle HTTP POST requests using pmcp
pub async fn handle_pmcp_post(
    State(state): State<PmcpHttpState>,
    body: String,
) -> Result<Json<serde_json::Value>, StatusCode> {
    log::debug!("Handling PMCP HTTP request");
    
    // Parse the JSON-RPC request
    let request: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    // For now, create a simple response format
    // This needs to be adapted to use pmcp's actual API
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "message": "pmcp integration in progress"
        },
        "id": request.get("id")
    });
    
    Ok(Json(response))
}