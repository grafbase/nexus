use serde::Deserialize;

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProxyConfig {
    pub anthropic: AnthropicProxyConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnthropicProxyConfig {
    pub enabled: bool,
    pub path: String,
}

impl Default for AnthropicProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "/proxy/anthropic".to_string(),
        }
    }
}
