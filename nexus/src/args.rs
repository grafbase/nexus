use std::{fmt, io::IsTerminal, net::SocketAddr, path::PathBuf};

use clap::{Parser, ValueEnum};
use config::Config;

#[derive(Debug, Parser)]
#[command(name = "Grafbase Nexus", version, long_about = concat!("Grafbase Nexus v", env!("CARGO_PKG_VERSION")))]
pub struct Args {
    /// IP address on which the server will listen for incomming connections.
    /// Default: 127.0.0.1:6000
    #[arg(short, long, env = "NEXUS_LISTEN_ADDRESS")]
    pub listen_address: Option<SocketAddr>,
    /// Path to the TOML configuration file
    #[arg(long, short, env = "NEXUS_CONFIG_PATH", default_value = "./nexus.toml")]
    pub config: PathBuf,
    /// Set the logging level, this applies to all spans, logs and trace events.
    #[arg(long = "log", env = "NEXUS_LOG", default_value_t = LogLevel::default())]
    pub log_level: LogLevel,
    /// Set the style of log output
    #[arg(long, env = "NEXUS_LOG_STYLE", default_value_t = LogStyle::default())]
    pub log_style: LogStyle,
}

impl Args {
    pub fn config(&self) -> anyhow::Result<Config> {
        let config = if self.config.exists() {
            Config::load(&self.config)?
        } else {
            let config = Config::default();
            // Validate default config to ensure it has downstream servers
            config.validate()?;
            config
        };

        Ok(config)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum LogStyle {
    /// Colorized text, used as the default with TTY output
    Color,
    /// Standard text, used as the default with non-TTY output
    Text,
    /// JSON objects
    Json,
}

impl Default for LogStyle {
    fn default() -> Self {
        if std::io::stdout().is_terminal() {
            LogStyle::Color
        } else {
            LogStyle::Text
        }
    }
}

impl AsRef<str> for LogStyle {
    fn as_ref(&self) -> &str {
        match self {
            LogStyle::Color => "color",
            LogStyle::Text => "text",
            LogStyle::Json => "json",
        }
    }
}

impl fmt::Display for LogStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum LogLevel {
    /// Disable logging
    Off,
    /// Only log errors
    Error,
    /// Log errors, and warnings
    Warn,
    /// Log errors, warnings, and info messages
    #[default]
    Info,
    /// Log errors, warnings, info, and debug messages
    Debug,
    /// Log errors, warnings, info, debug, and trace messages
    Trace,
}

impl LogLevel {
    pub fn env_filter(self) -> String {
        match self {
            LogLevel::Off => "off".to_string(),
            // For other levels, set the default to 'warn' for all crates,
            // but use the selected level for workspace crates
            level => format!(
                "warn,nexus={level},server={level},mcp={level},config={level},llm={level},telemetry={level},rate_limit={level}"
            ),
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

impl AsRef<str> for LogLevel {
    fn as_ref(&self) -> &str {
        match self {
            LogLevel::Off => "off",
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}
