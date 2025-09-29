# Configuration Crate

Type-safe TOML configuration with validation for Nexus.

## Module Structure
- `lib.rs` - Main `Config` struct, basic tests
- `loader.rs` - Loading, validation, validation tests
- `server.rs` - HTTP server config
- `oauth.rs` - OAuth2 authentication
- `cors.rs` - CORS with comprehensive tests
- `client_identification.rs` - Rate limiting identity
- `mcp.rs` - Model Context Protocol
- `llm.rs` - LLM providers (includes model pattern routing)
- `rate_limit.rs` - Rate limiting
- `telemetry.rs` - Observability

## Key Principles

### Always Use Default Trait
```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]  // Struct level
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
        }
    }
}
```

### Validation
```rust
impl Config {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.port > 0, "Port must be positive");
        Ok(())
    }
}
```

### Secrets
```rust
use secrecy::SecretString;

pub struct AuthConfig {
    pub client_secret: SecretString,  // Never logged
}
```

## Test Pattern (REQUIRED)

1. **Start with `indoc!` TOML**
2. **Parse with `toml::from_str()`**
3. **End with INLINE `assert_debug_snapshot!`**

**CRITICAL RULES**:
- NEVER use `assert_eq!` for structs, vecs, or complex types
- ALWAYS use inline snapshots (`@r###"..."###`)
- NEVER create external snapshot files
- Use `assert!` or `assert_eq!` ONLY for booleans and primitives

```rust
#[test]
fn config_test() {
    let config_str = indoc! {r#"
        [server]
        port = 3000
    "#};
    
    let config: Config = toml::from_str(config_str).unwrap();
    
    assert_debug_snapshot!(config, @r###"
    Config {
        server: ServerConfig {
            host: "127.0.0.1",
            port: 3000,
        },
    }
    "###);
}
```

## Test Organization
- Module tests co-located with code
- `lib.rs`: Basic Config tests
- `loader.rs`: Validation tests
- Each module: Own specific tests

## LLM Provider Configuration

### Model Pattern Routing

The `llm.rs` module supports dynamic model routing via regex patterns:

```rust
pub struct ModelPattern {
    regex: Regex,  // Case-insensitive regex
}
```

**Validation Rules:**
- Pattern cannot be empty
- Pattern cannot contain `/` characters
- Must be valid regex syntax
- Case-insensitive by default

**Configuration:**
```toml
[llm.providers.openai]
type = "openai"
api_key = "sk-..."
model_pattern = "^gpt-.*"  # Optional: matches all GPT models

# Can mix pattern with explicit models
[llm.providers.openai.models.gpt-4]
```

**Provider Requirements:**
- Each provider must have either:
  - At least one explicit model configured, OR
  - A `model_pattern` field
- Both can coexist for mixed routing

## Update This Doc When:
- Adding modules or config sections
- Changing validation rules
- Modifying test patterns
- Changing defaults
- Adding new LLM configuration features