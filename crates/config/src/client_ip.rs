/// Configuration for extracting client IP from headers.
#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ClientIpConfig {
    /// Whether X-Real-IP header should be used.
    pub x_real_ip: bool,
    /// How many trusted hops to skip when using X-Forwarded-For header.
    /// If None, X-Forwarded-For is not used.
    pub x_forwarded_for_trusted_hops: Option<usize>,
}
