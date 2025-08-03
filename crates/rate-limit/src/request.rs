//! Request information for rate limiting.

use std::net::IpAddr;

/// Information about a request that needs to be rate limited.
#[derive(Debug, Clone)]
pub struct RateLimitRequest {
    /// IP address of the request origin.
    pub ip: Option<IpAddr>,
    /// Name of the MCP server handling the request.
    pub server_name: Option<String>,
    /// Name of the tool being invoked.
    pub tool_name: Option<String>,
}

impl RateLimitRequest {
    /// Create a new builder for a rate limit request.
    pub fn builder() -> RateLimitRequestBuilder {
        RateLimitRequestBuilder::default()
    }
}

/// Builder for creating rate limit requests.
#[derive(Debug, Default)]
pub struct RateLimitRequestBuilder {
    ip: Option<IpAddr>,
    server_name: Option<String>,
    tool_name: Option<String>,
}

impl RateLimitRequestBuilder {
    /// Set the IP address.
    pub fn ip(mut self, ip: IpAddr) -> Self {
        self.ip = Some(ip);
        self
    }
    
    /// Set the IP address from a string.
    pub fn ip_str(mut self, ip: &str) -> Result<Self, std::net::AddrParseError> {
        self.ip = Some(ip.parse()?);
        Ok(self)
    }
    
    /// Set the server name.
    pub fn server(mut self, name: impl Into<String>) -> Self {
        self.server_name = Some(name.into());
        self
    }
    
    /// Set the tool name.
    pub fn tool(mut self, name: impl Into<String>) -> Self {
        self.tool_name = Some(name.into());
        self
    }
    
    /// Set both server and tool name at once.
    pub fn server_tool(mut self, server: impl Into<String>, tool: impl Into<String>) -> Self {
        self.server_name = Some(server.into());
        self.tool_name = Some(tool.into());
        self
    }
    
    /// Build the rate limit request.
    pub fn build(self) -> RateLimitRequest {
        RateLimitRequest {
            ip: self.ip,
            server_name: self.server_name,
            tool_name: self.tool_name,
        }
    }
}

