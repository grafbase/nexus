mod mcp;

use mcp::McpConfig;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub mcp: McpConfig,
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use crate::Config;

    #[test]
    fn all_values() {
        let config = indoc! {r#"
            [mcp]
            enabled = false
            listen_address = "127.0.0.1:8080"
            protocol = "sse"
            path = "/mcp-path"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config, @r#"
        Config {
            mcp: McpConfig {
                enabled: false,
                listen_address: Some(
                    127.0.0.1:8080,
                ),
                protocol: Sse,
                path: "/mcp-path",
            },
        }
        "#);
    }

    #[test]
    fn defaults() {
        let config: Config = toml::from_str("").unwrap();

        insta::assert_debug_snapshot!(&config, @r#"
        Config {
            mcp: McpConfig {
                enabled: true,
                listen_address: None,
                protocol: StreamableHttp,
                path: "/mcp",
            },
        }
        "#);
    }
}
