//! PMCP server adapter for bridging McpServer with pmcp's Server implementation

use crate::server::McpServer;
use crate::types::{
    CallToolRequest, CallToolResult,
    ListToolsResult, 
    ListPromptsResult, GetPromptRequest, GetPromptResult,
    ListResourcesResult, ReadResourceRequest, ReadResourceResult,
};
use pmcp::types::{InitializeResult, Implementation};
use pmcp::{ProtocolVersion, ServerCapabilities};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Adapter that implements pmcp's Handler trait for our McpServer
pub struct PmcpServerAdapter {
    mcp_server: Arc<Mutex<McpServer>>,
}

impl PmcpServerAdapter {
    pub fn new(mcp_server: McpServer) -> Self {
        Self {
            mcp_server: Arc::new(Mutex::new(mcp_server)),
        }
    }

    /// Build a pmcp Server using this adapter
    pub fn build_server(self) -> anyhow::Result<()> {
        // Since pmcp doesn't expose Server/ServerBuilder in a way we can use,
        // this adapter will be used directly for handling requests
        Ok(())
    }
}

// Manual implementation of handler methods since pmcp doesn't expose Handler trait
impl PmcpServerAdapter {
    pub async fn initialize(&self) -> Result<InitializeResult, pmcp::Error> {
        let server = self.mcp_server.lock().await;
        let info = server.get_info().await;
        
        Ok(InitializeResult {
            protocol_version: ProtocolVersion(info.protocol_version.clone()),
            capabilities: ServerCapabilities {
                tools: if info.capabilities.tools.is_some() {
                    Some(Default::default())
                } else {
                    None
                },
                prompts: if info.capabilities.prompts.is_some() {
                    Some(Default::default())
                } else {
                    None
                },
                resources: if info.capabilities.resources.is_some() {
                    Some(Default::default())
                } else {
                    None
                },
                ..Default::default()
            },
            server_info: Implementation {
                name: info.name.clone(),
                version: info.version.clone(),
            },
            instructions: info.instructions.clone(),
        })
    }

    pub async fn list_tools(&self) -> Result<ListToolsResult, pmcp::Error> {
        let server = self.mcp_server.lock().await;
        
        match server.list_tools().await {
            Ok(result) => Ok(result),
            Err(e) => Err(pmcp::Error::internal(e.to_string())),
        }
    }

    pub async fn call_tool(&self, request: CallToolRequest) -> Result<CallToolResult, pmcp::Error> {
        let server = self.mcp_server.lock().await;
        
        // Convert CallToolRequest to our internal format
        let internal_request = crate::types::CallToolRequest {
            name: request.name,
            arguments: request.arguments,
        };
        
        // TODO: Get auth token from somewhere (context?)
        match server.call_tool(internal_request, None).await {
            Ok(result) => Ok(result),
            Err(e) => Err(pmcp::Error::internal(e.to_string())),
        }
    }

    pub async fn list_prompts(&self) -> Result<ListPromptsResult, pmcp::Error> {
        let server = self.mcp_server.lock().await;
        
        // TODO: Get auth token from somewhere (context?)
        match server.list_prompts(None).await {
            Ok(result) => Ok(result),
            Err(e) => Err(pmcp::Error::internal(e.to_string())),
        }
    }

    pub async fn get_prompt(&self, request: GetPromptRequest) -> Result<GetPromptResult, pmcp::Error> {
        let server = self.mcp_server.lock().await;
        
        // TODO: Get auth token from somewhere (context?)
        match server.get_prompt(request, None).await {
            Ok(result) => Ok(result),
            Err(e) => Err(pmcp::Error::internal(e.to_string())),
        }
    }

    pub async fn list_resources(&self) -> Result<ListResourcesResult, pmcp::Error> {
        let server = self.mcp_server.lock().await;
        
        // TODO: Get auth token from somewhere (context?)
        match server.list_resources(None).await {
            Ok(result) => Ok(result),
            Err(e) => Err(pmcp::Error::internal(e.to_string())),
        }
    }

    pub async fn read_resource(&self, request: ReadResourceRequest) -> Result<ReadResourceResult, pmcp::Error> {
        let server = self.mcp_server.lock().await;
        
        // TODO: Get auth token from somewhere (context?)
        match server.read_resource(request, None).await {
            Ok(result) => Ok(result),
            Err(e) => Err(pmcp::Error::internal(e.to_string())),
        }
    }
}