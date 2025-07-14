#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to open configuration file: {0}")]
    ConfigOpen(#[from] std::io::Error),
    #[error("Failed to parse configuration file: {0}")]
    ConfigParse(#[from] toml::de::Error),
    #[error("At {path} failed substituing environment variable: {reason}")]
    EnvVarSubstitution { path: String, reason: String },
}
