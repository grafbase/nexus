//! Common utilities for OTLP exporter configuration

use config::OtlpExporterConfig;
use opentelemetry_otlp::tonic_types::metadata::MetadataMap;
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
