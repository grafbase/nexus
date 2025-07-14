mod error;
mod loader;
mod mcp;

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use mcp::McpConfig;
use serde::Deserialize;

pub(crate) type Result<T> = std::result::Result<T, error::Error>;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub mcp: McpConfig,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> crate::Result<Config> {
        loader::load(path)
    }
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub listen_address: Option<SocketAddr>,
    pub tls: Option<TlsConfig>,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    pub certificate: PathBuf,
    pub key: PathBuf,
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use crate::Config;

    #[test]
    fn all_values() {
        let config = indoc! {r#"
            [server]
            listen_address = "127.0.0.1:8080"

            [mcp]
            enabled = false
            protocol = "sse"
            path = "/mcp-path"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config, @r#"
        Config {
            server: ServerConfig {
                listen_address: Some(
                    127.0.0.1:8080,
                ),
            },
            mcp: McpConfig {
                enabled: false,
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
            server: ServerConfig {
                listen_address: None,
            },
            mcp: McpConfig {
                enabled: true,
                protocol: StreamableHttp,
                path: "/mcp",
            },
        }
        "#);
    }
}
