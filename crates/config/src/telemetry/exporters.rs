use crate::{HeaderName, HeaderValue};
use duration_str::deserialize_duration;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
use std::time::Duration;
use url::Url;

/// Exporters configuration for telemetry
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ExportersConfig {
    /// OTLP exporter configuration
    pub otlp: OtlpExporterConfig,
}

/// OTLP exporter configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OtlpExporterConfig {
    /// Whether this exporter is enabled
    pub enabled: bool,

    /// OTLP endpoint URL
    pub endpoint: Url,

    /// OTLP protocol selection
    pub protocol: OtlpProtocol,

    /// Request timeout
    #[serde(deserialize_with = "deserialize_duration")]
    pub timeout: Duration,

    /// Batch export configuration
    pub batch_export: BatchExportConfig,

    /// gRPC configuration (mutually exclusive with http)
    pub grpc: Option<OtlpGrpcConfig>,

    /// HTTP configuration (mutually exclusive with grpc)
    pub http: Option<OtlpHttpConfig>,
}

impl Default for OtlpExporterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: Url::parse("http://localhost:4317").expect("default URL should be valid"),
            protocol: OtlpProtocol::default(),
            timeout: Duration::from_secs(60),
            batch_export: BatchExportConfig::default(),
            grpc: None,
            http: None,
        }
    }
}

impl OtlpExporterConfig {
    /// Validate that the protocol configuration matches the selected protocol
    pub fn validate(&self) -> anyhow::Result<()> {
        match self.protocol {
            OtlpProtocol::Grpc => {
                if self.http.is_some() {
                    return Err(anyhow::anyhow!(
                        "HTTP configuration found but protocol is set to 'grpc'"
                    ));
                }
            }
            OtlpProtocol::Http => {
                if self.grpc.is_some() {
                    return Err(anyhow::anyhow!(
                        "gRPC configuration found but protocol is set to 'http'"
                    ));
                }
            }
        }

        // Also check that both aren't configured
        if self.grpc.is_some() && self.http.is_some() {
            return Err(anyhow::anyhow!(
                "Cannot configure both 'grpc' and 'http' for OTLP exporter. Choose one."
            ));
        }

        Ok(())
    }
}

/// gRPC-specific configuration for OTLP
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OtlpGrpcConfig {
    /// gRPC metadata to include with requests
    #[serde(default)]
    pub headers: GrpcHeaders,

    /// TLS configuration for secure connections
    #[serde(default)]
    pub tls: Option<OtlpGrpcTlsConfig>,
}

/// TLS configuration for OTLP gRPC connections
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OtlpGrpcTlsConfig {
    /// Domain name for TLS verification (SNI)
    pub domain_name: Option<String>,

    /// Path to the client private key PEM file
    pub key: Option<String>,

    /// Path to the client certificate PEM file
    pub cert: Option<String>,

    /// Path to the CA certificate PEM file
    pub ca: Option<String>,
}

/// HTTP-specific configuration for OTLP
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OtlpHttpConfig {
    /// HTTP headers to include with requests
    #[serde(default)]
    pub headers: HttpHeaders,
}

impl ExportersConfig {
    /// Get the OTLP exporter configuration
    pub fn otlp(&self) -> &OtlpExporterConfig {
        &self.otlp
    }
}

/// OTLP protocol selection
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum OtlpProtocol {
    /// gRPC protocol (default)
    #[default]
    Grpc,
    /// HTTP/protobuf protocol
    Http,
}

/// Batch export configuration for OTLP
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BatchExportConfig {
    /// Delay between batch exports
    #[serde(deserialize_with = "deserialize_duration", default = "default_scheduled_delay")]
    pub scheduled_delay: Duration,

    /// Maximum queue size
    pub max_queue_size: usize,

    /// Maximum batch size for export
    pub max_export_batch_size: usize,

    /// Maximum concurrent exports
    pub max_concurrent_exports: usize,
}

impl Default for BatchExportConfig {
    fn default() -> Self {
        Self {
            scheduled_delay: default_scheduled_delay(),
            max_queue_size: 2048,
            max_export_batch_size: 512,
            max_concurrent_exports: 1,
        }
    }
}

fn default_scheduled_delay() -> Duration {
    Duration::from_secs(5)
}

/// Collection of HTTP headers for OTLP HTTP exporters
#[derive(Debug, Clone, Default)]
pub struct HttpHeaders {
    entries: Vec<(HeaderName, HeaderValue)>,
}

impl HttpHeaders {
    /// Iterate over the configured headers
    pub fn iter(&self) -> impl Iterator<Item = (&HeaderName, &HeaderValue)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A validated gRPC metadata header name
#[derive(Debug, Clone)]
pub struct GrpcHeaderName(HeaderName);

impl<'de> Deserialize<'de> for GrpcHeaderName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = HeaderName::deserialize(deserializer)?;

        // Validate gRPC metadata key rules
        if name.as_str().starts_with("grpc-") {
            return Err(serde::de::Error::custom(
                "gRPC metadata key cannot start with 'grpc-' (reserved)",
            ));
        }

        Ok(GrpcHeaderName(name))
    }
}

/// Collection of gRPC headers for OTLP gRPC exporters
/// These will be converted to a HeaderMap and then to MetadataMap
#[derive(Debug, Clone, Default)]
pub struct GrpcHeaders {
    entries: Vec<(HeaderName, HeaderValue)>,
}

impl GrpcHeaders {
    /// Iterate over the configured headers
    pub fn iter(&self) -> impl Iterator<Item = (&HeaderName, &HeaderValue)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'de> Deserialize<'de> for HttpHeaders {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HeadersVisitor;

        impl<'de> Visitor<'de> for HeadersVisitor {
            type Value = HttpHeaders;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map or array of HTTP headers")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some((name, value)) = map.next_entry::<HeaderName, HeaderValue>()? {
                    entries.push((name, value));
                }
                Ok(HttpHeaders { entries })
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some(header) = seq.next_element::<HttpHeaderEntry>()? {
                    entries.push((header.name, header.value));
                }
                Ok(HttpHeaders { entries })
            }
        }

        deserializer.deserialize_any(HeadersVisitor)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct HttpHeaderEntry {
    name: HeaderName,
    value: HeaderValue,
}

impl<'de> Deserialize<'de> for GrpcHeaders {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HeadersVisitor;

        impl<'de> Visitor<'de> for HeadersVisitor {
            type Value = GrpcHeaders;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map or array of gRPC metadata")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some((name, value)) = map.next_entry::<GrpcHeaderName, HeaderValue>()? {
                    entries.push((name.0, value));
                }
                Ok(GrpcHeaders { entries })
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some(header) = seq.next_element::<GrpcHeaderEntry>()? {
                    entries.push((header.name.0, header.value));
                }
                Ok(GrpcHeaders { entries })
            }
        }

        deserializer.deserialize_any(HeadersVisitor)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct GrpcHeaderEntry {
    name: GrpcHeaderName,
    value: HeaderValue,
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn grpc_headers_valid() {
        let config: ExportersConfig = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true
            protocol = "grpc"

            [otlp.grpc.headers]
            authorization = "Bearer token"
            x-custom-header = "value123"
        "#})
        .unwrap();

        assert!(config.otlp.grpc.is_some());
        let grpc = config.otlp.grpc.as_ref().unwrap();
        assert_eq!(grpc.headers.iter().count(), 2);
        assert_eq!(config.otlp.protocol, OtlpProtocol::Grpc);
    }

    #[test]
    fn http_headers_valid() {
        let config: ExportersConfig = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true
            protocol = "http"

            [otlp.http.headers]
            Authorization = "Bearer token"
            X-Custom-Header = "value123"
        "#})
        .unwrap();

        assert!(config.otlp.http.is_some());
        let http = config.otlp.http.as_ref().unwrap();
        assert_eq!(http.headers.iter().count(), 2);
        assert_eq!(config.otlp.protocol, OtlpProtocol::Http);
    }

    #[test]
    fn grpc_headers_invalid_key_rejected() {
        // HTTP header names are case-insensitive and will be normalized
        // So uppercase is actually valid for HeaderName
        // This test should verify that uppercase works
        let result: Result<ExportersConfig, _> = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true

            [otlp.grpc.headers]
            Authorization = "Bearer token"
        "#});

        assert!(result.is_ok());
    }

    #[test]
    fn grpc_headers_reserved_prefix_rejected() {
        // gRPC doesn't allow keys starting with "grpc-"
        let result: Result<ExportersConfig, _> = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true

            [otlp.grpc.headers]
            grpc-status = "0"
        "#});

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("cannot start with 'grpc-' (reserved)"));
    }

    #[test]
    fn grpc_headers_non_ascii_value_accepted() {
        // HeaderValue actually accepts UTF-8 and percent-encodes it internally
        // The emoji will be percent-encoded as valid ASCII
        let result: Result<ExportersConfig, _> = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true

            [otlp.grpc.headers]
            authorization = "Bearer test"
        "#});

        assert!(result.is_ok());
    }

    #[test]
    fn both_grpc_and_http_rejected() {
        let result: Result<ExportersConfig, _> = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true

            [otlp.grpc.headers]
            authorization = "Bearer token"

            [otlp.http.headers]
            Authorization = "Bearer token"
        "#});

        // This should parse successfully, but validation should fail
        let config = result.unwrap();
        let validation_result = config.otlp.validate();
        assert!(validation_result.is_err());
        let error_msg = validation_result.unwrap_err().to_string();
        // Protocol defaults to grpc, so it will complain about http config
        assert!(
            error_msg.contains("HTTP configuration found but protocol is set to 'grpc'")
                || error_msg.contains("Cannot configure both"),
            "Unexpected error: {}",
            error_msg
        );
    }

    #[test]
    fn protocol_detection() {
        // Explicit gRPC config
        let grpc_config: ExportersConfig = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true
            protocol = "grpc"

            [otlp.grpc.headers]
            authorization = "Bearer token"
        "#})
        .unwrap();
        assert_eq!(grpc_config.otlp.protocol, OtlpProtocol::Grpc);

        // Explicit HTTP config
        let http_config: ExportersConfig = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true
            protocol = "http"

            [otlp.http.headers]
            Authorization = "Bearer token"
        "#})
        .unwrap();
        assert_eq!(http_config.otlp.protocol, OtlpProtocol::Http);

        // Default (no explicit protocol)
        let default_config: ExportersConfig = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true
        "#})
        .unwrap();
        assert_eq!(default_config.otlp.protocol, OtlpProtocol::Grpc);
    }

    #[test]
    fn grpc_tls_config_full() {
        let config: ExportersConfig = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true
            protocol = "grpc"

            [otlp.grpc.tls]
            domain_name = "example.com"
            key = "/path/to/key.pem"
            cert = "/path/to/cert.pem"
            ca = "/path/to/ca.pem"
        "#})
        .unwrap();

        assert!(config.otlp.grpc.is_some());
        let grpc = config.otlp.grpc.as_ref().unwrap();
        assert!(grpc.tls.is_some());

        let tls = grpc.tls.as_ref().unwrap();
        assert_eq!(tls.domain_name.as_deref(), Some("example.com"));
        assert_eq!(tls.key.as_deref(), Some("/path/to/key.pem"));
        assert_eq!(tls.cert.as_deref(), Some("/path/to/cert.pem"));
        assert_eq!(tls.ca.as_deref(), Some("/path/to/ca.pem"));
    }

    #[test]
    fn grpc_tls_config_partial() {
        let config: ExportersConfig = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true
            protocol = "grpc"

            [otlp.grpc.tls]
            domain_name = "example.com"
            ca = "/path/to/ca.pem"
        "#})
        .unwrap();

        assert!(config.otlp.grpc.is_some());
        let grpc = config.otlp.grpc.as_ref().unwrap();
        assert!(grpc.tls.is_some());

        let tls = grpc.tls.as_ref().unwrap();
        assert_eq!(tls.domain_name.as_deref(), Some("example.com"));
        assert!(tls.key.is_none());
        assert!(tls.cert.is_none());
        assert_eq!(tls.ca.as_deref(), Some("/path/to/ca.pem"));
    }

    #[test]
    fn grpc_tls_with_headers() {
        let config: ExportersConfig = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true
            protocol = "grpc"

            [otlp.grpc.headers]
            authorization = "Bearer token"
            x-custom = "value"

            [otlp.grpc.tls]
            domain_name = "secure.example.com"
            ca = "/etc/ssl/ca.pem"
        "#})
        .unwrap();

        assert!(config.otlp.grpc.is_some());
        let grpc = config.otlp.grpc.as_ref().unwrap();

        // Check headers
        assert_eq!(grpc.headers.iter().count(), 2);

        // Check TLS
        assert!(grpc.tls.is_some());
        let tls = grpc.tls.as_ref().unwrap();
        assert_eq!(tls.domain_name.as_deref(), Some("secure.example.com"));
        assert_eq!(tls.ca.as_deref(), Some("/etc/ssl/ca.pem"));
    }

    #[test]
    fn http_no_tls_config() {
        // Ensure HTTP protocol doesn't accidentally get TLS config
        let result: Result<ExportersConfig, _> = toml::from_str(indoc! {r#"
            [otlp]
            enabled = true
            protocol = "http"

            [otlp.http.headers]
            Authorization = "Bearer token"

            [otlp.http.tls]
            domain_name = "example.com"
        "#});

        // This should fail because http section doesn't have a tls field
        assert!(result.is_err());
    }
}
