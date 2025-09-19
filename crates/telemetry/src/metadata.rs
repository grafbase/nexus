//! Common utilities for OTLP exporter configuration

use config::OtlpExporterConfig;
use opentelemetry_otlp::tonic_types::metadata::MetadataMap;
use opentelemetry_otlp::tonic_types::transport::{Certificate, ClientTlsConfig, Identity};
use std::collections::HashMap;

/// Build a MetadataMap from configured headers for gRPC exporters
///
/// The headers are validated during config loading and stored as HeaderName/HeaderValue.
/// We convert them to a HeaderMap first, then use MetadataMap::from_headers() for the conversion.
pub fn build_metadata(config: &OtlpExporterConfig) -> anyhow::Result<MetadataMap> {
    let Some(grpc_config) = &config.grpc else {
        return Ok(MetadataMap::new());
    };

    if grpc_config.headers.is_empty() {
        return Ok(MetadataMap::new());
    }

    // Build a HeaderMap from our headers
    let mut header_map = http::HeaderMap::new();

    for (name, value) in grpc_config.headers.iter() {
        // Convert our wrapper types to the inner http types
        header_map.insert(name.clone().into_inner(), value.clone().into_inner());
    }

    // Convert HeaderMap to MetadataMap using the from_headers method
    Ok(MetadataMap::from_headers(header_map))
}

/// Build a HashMap of headers for HTTP exporters
///
/// The headers are already parsed and validated as HeaderName/HeaderValue during config loading.
pub fn build_http_headers(config: &OtlpExporterConfig) -> anyhow::Result<HashMap<String, String>> {
    let Some(http_config) = &config.http else {
        return Ok(HashMap::new());
    };

    if http_config.headers.is_empty() {
        return Ok(HashMap::new());
    }

    let mut headers = HashMap::with_capacity(http_config.headers.iter().count());

    for (name, value) in http_config.headers.iter() {
        let header_name = name.as_str().to_string();
        let header_value = value
            .to_str()
            .map_err(|_| anyhow::anyhow!("Header value contains non-UTF8 characters for key: {}", header_name))?
            .to_string();

        headers.insert(header_name, header_value);
    }

    Ok(headers)
}

/// Build TLS configuration for gRPC exporters
pub fn build_tls_config(config: &OtlpExporterConfig) -> anyhow::Result<Option<ClientTlsConfig>> {
    let Some(grpc_config) = &config.grpc else {
        return Ok(None);
    };

    let Some(tls_config) = &grpc_config.tls else {
        return Ok(None);
    };

    let mut client_tls = ClientTlsConfig::new();

    // Set domain name for SNI if provided
    if let Some(domain_name) = &tls_config.domain_name {
        client_tls = client_tls.domain_name(domain_name.clone());
    }

    // Add CA certificate if provided
    if let Some(ca_path) = &tls_config.ca {
        let ca_cert = std::fs::read_to_string(ca_path)
            .map_err(|e| anyhow::anyhow!("Failed to read CA certificate from {}: {}", ca_path, e))?;
        client_tls = client_tls.ca_certificate(Certificate::from_pem(ca_cert));
    }

    // Add client identity (key + cert) if both are provided
    if let Some(key_path) = &tls_config.key
        && let Some(cert_path) = &tls_config.cert
    {
        let key = std::fs::read_to_string(key_path)
            .map_err(|e| anyhow::anyhow!("Failed to read client key from {}: {}", key_path, e))?;
        let cert = std::fs::read_to_string(cert_path)
            .map_err(|e| anyhow::anyhow!("Failed to read client certificate from {}: {}", cert_path, e))?;
        client_tls = client_tls.identity(Identity::from_pem(cert, key));
    }

    Ok(Some(client_tls))
}
