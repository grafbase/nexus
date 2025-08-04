# Configuration Crate Guidelines

This crate handles all configuration management for the Nexus AI router system.

## Purpose

The config crate provides:
- Type-safe configuration loading from TOML files
- Environment-specific settings management
- Configuration validation and defaults
- Serde-based serialization/deserialization
- Rate limiting configuration for global, per-IP, and per-MCP-server limits
- Redis and in-memory storage backends for rate limiting

## Key Principles

### Configuration Structure

All configuration structs should:
- Derive `serde::Deserialize` (not `Serialize` - configs are read-only)
- Derive `Debug` for testing and debugging
- **Always implement the `Default` trait** instead of using serde default functions when possible
- Use `#[serde(default)]` on the struct level to allow partial configuration
- Only use field-level `#[serde(default = "...")]` when the default value differs from the `Default` implementation
- Use descriptive field names that match TOML conventions (snake_case)

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub timeout_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            timeout_secs: 30,
        }
    }
}

// Example where field-level default is needed (rare case)
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AdvancedConfig {
    pub standard_field: String,
    
    // Only use field-level default when it differs from Default impl
    #[serde(default = "special_default")]
    pub special_field: String,
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            standard_field: "normal".to_string(),
            special_field: "default".to_string(),
        }
    }
}

fn special_default() -> String {
    "special-override".to_string()
}
```

### Default Implementation Best Practices

Why prefer `Default` trait over serde default functions:
- **Type safety**: The `Default` trait ensures all fields have defaults
- **Testability**: Easy to create test configurations with `Config::default()`
- **Consistency**: One source of truth for default values
- **Clarity**: Default values are visible in one place

```rust
// Good: Clean Default implementation
impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            auth: AuthConfig::default(),
            search: SearchConfig::default(),
        }
    }
}

// Usage in tests becomes simple
let mut config = Config::default();
config.server.port = 9000;
```

### Validation

Configuration should be validated at load time:

```rust
impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.server.port > 0, "Port must be positive");
        anyhow::ensure!(!self.server.host.is_empty(), "Host cannot be empty");
        Ok(())
    }
}
```

### Nested Configuration

Use logical groupings for related settings:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    pub server: ServerConfig,
    pub mcp: McpConfig,
    pub auth: AuthConfig,
    pub search: SearchConfig,
}
```

## Testing

Always include tests for:
- Default configuration values
- Configuration loading from TOML
- Validation logic
- Environment variable overrides

Use insta inline snapshots for testing configuration structures:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn default_config_is_valid() {
        let config = Config::default();
        assert!(config.validate().is_ok());

        // Verify default configuration structure
        assert_debug_snapshot!(config, @r###"
        Config {
            server: ServerConfig {
                host: "127.0.0.1",
                port: 8080,
                timeout_secs: 30,
            },
            mcp: McpConfig {
                max_connections: 10,
                timeout_secs: 60,
            },
            auth: AuthConfig {
                enabled: false,
            },
            search: SearchConfig {
                index_path: "./data/search",
            },
        }
        "###);
    }

    #[test]
    fn loads_from_toml() {
        let toml = r#"
            [server]
            host = "0.0.0.0"
            port = 3000

            [mcp]
            max_connections = 20
        "#;

        let config: Config = toml::from_str(toml).unwrap();

        // Use snapshot to verify the entire loaded configuration
        assert_debug_snapshot!(config, @r###"
        Config {
            server: ServerConfig {
                host: "0.0.0.0",
                port: 3000,
                timeout_secs: 30,
            },
            mcp: McpConfig {
                max_connections: 20,
                timeout_secs: 60,
            },
            auth: AuthConfig {
                enabled: false,
            },
            search: SearchConfig {
                index_path: "./data/search",
            },
        }
        "###);
    }

    #[test]
    fn partial_config_with_defaults() {
        let toml = r#"
            [server]
            port = 9000
        "#;

        let config: Config = toml::from_str(toml).unwrap();

        // Snapshot testing shows how defaults are applied
        assert_debug_snapshot!(config.server, @r###"
        ServerConfig {
            host: "127.0.0.1",
            port: 9000,
            timeout_secs: 30,
        }
        "###);
    }
}
```

When using insta snapshots for configuration testing:
- Use `assert_debug_snapshot!` for configuration structs (which only implement `Debug` and `Deserialize`)
- Include inline snapshots (`@r###"..."###`) for clarity
- Test both complete and partial configurations to verify defaults
- Approve snapshot changes with `cargo insta approve`

## Documentation

Each configuration struct should have:
- Doc comments explaining its purpose
- Examples in doc comments for complex configurations
- Clear descriptions of validation rules

```rust
/// Server configuration for the Nexus HTTP API
///
/// # Example
/// ```toml
/// [server]
/// host = "0.0.0.0"
/// port = 8080
/// timeout_secs = 60
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// The host address to bind to
    pub host: String,

    /// The port to listen on (must be > 0)
    pub port: u16,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}
```

## Security

- Use the `secrecy` crate for sensitive configuration values (tokens, passwords, keys)
- The `Secret` type prevents accidental logging or exposure of sensitive data
- Access secret values only when needed using `expose_secret()`
- Validate URLs and paths to prevent injection attacks

```rust
use secrecy::{Secret, SecretString};

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    pub client_id: String,

    /// Client secret wrapped in Secret to prevent accidental exposure
    pub client_secret: SecretString,

    pub redirect_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Database password protected from logs
    pub password: SecretString,

    /// Connection string may contain credentials
    pub connection_string: SecretString,
}

// Usage example
impl AuthConfig {
    pub fn make_request(&self, client: &reqwest::Client) -> anyhow::Result<()> {
        // Only expose secret when actually needed
        let request = client
            .post("https://api.example.com/token")
            .form(&[
                ("client_id", &self.client_id),
                ("client_secret", self.client_secret.expose_secret()),
            ]);

        // ... rest of implementation
        Ok(())
    }
}
```

### Testing with Secrets

When testing configurations with secrets, the `Secret` type will display as `Secret([REDACTED])` in debug output:

```rust
#[test]
fn config_with_secrets() {
    let toml = r#"
        [auth]
        client_id = "public-id"
        client_secret = "super-secret-value"
    "#;

    let config: AuthConfig = toml::from_str(toml).unwrap();

    // Secret values are redacted in snapshots
    assert_debug_snapshot!(config, @r###"
    AuthConfig {
        client_id: "public-id",
        client_secret: Secret([REDACTED]),
        redirect_uri: "http://localhost:8080/callback",
    }
    "###);
}
```

Remember: Configuration should be predictable, well-documented, and fail fast with clear error messages when invalid.

## Keeping This Document Updated

**IMPORTANT**: Update this CLAUDE.md when configuration patterns change:

1. **New Config Structs**: Document any new configuration sections
2. **Changed Defaults**: Update examples if default values change
3. **New Patterns**: Add guidance for new configuration patterns (e.g., new secret types)
4. **Validation Rules**: Document new validation requirements
5. **Breaking Changes**: Clearly mark deprecated patterns

Update triggers:
- Adding new configuration fields or sections
- Changing how secrets are handled
- Modifying validation logic
- Introducing new environment variable patterns
- Changing TOML structure or naming conventions
