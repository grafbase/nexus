#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to open configuration file: {0}")]
    ConfigOpenError(#[from] std::io::Error),
    #[error("Failed to parse configuration file: {0}")]
    ConfigParseError(#[from] toml::de::Error),
    #[error("At {path} failed substituing environment variable: {reason}")]
    EnvVarSubstitutionError { path: String, reason: String },
}
