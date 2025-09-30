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
- `llm.rs` - LLM providers (model discovery configuration and filters)
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

### Model Discovery and Filters

The `llm.rs` module exposes automatic model discovery with optional regex filtering. Providers may either:
- Configure at least one explicit model under `[llm.providers.<name>.models.<model-id>]`, or
- Supply a `model_filter` regex to restrict which discovered models are exposed

`ModelFilter` wraps a case-insensitive regex and enforces strict validation:

```rust
pub struct ModelFilter {
    pattern: Regex,  // validated: non-empty, no '/'
}
```

**Validation Rules:**
- Filter cannot be empty
- Filter cannot contain `/` characters
- Regex must compile successfully (case-insensitive flag is applied automatically)

**Configuration:**
```toml
[llm.providers.openai]
type = "openai"
api_key = "sk-..."
model_filter = "^gpt-4.*"  # Optional: restrict discovery to GPT-4 variants

# Explicit models always remain available, even if the filter would exclude them
[llm.providers.openai.models.gpt-3-5-turbo]
```

**Provider Requirements:**
- Startup validation ensures each provider has at least one explicit model or a `model_filter`
- Both explicit models and filters may coexist (filters apply only to discovered models)

## Update This Doc When:
- Adding modules or config sections
- Changing validation rules
- Modifying test patterns
- Changing defaults
- Adding new LLM configuration features
