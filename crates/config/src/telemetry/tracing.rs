use super::exporters::ExportersConfig;
use serde::{Deserialize, Deserializer};

/// Tracing configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TracingConfig {
    /// Sampling rate (0.0 to 1.0)
    #[serde(deserialize_with = "validate_sampling_rate")]
    pub sampling: f64,

    /// Use parent-based sampler
    pub parent_based_sampler: bool,

    /// Collection limits configuration
    #[serde(default)]
    pub collect: CollectConfig,

    /// Trace context propagation configuration
    #[serde(default)]
    pub propagation: PropagationConfig,

    /// Override global exporters for traces (optional)
    #[serde(default)]
    exporters: Option<ExportersConfig>,
}

impl TracingConfig {
    /// Get the exporters if configured
    pub fn exporters(&self) -> Option<&ExportersConfig> {
        self.exporters.as_ref()
    }
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            sampling: 0.15,
            parent_based_sampler: false,
            collect: CollectConfig::default(),
            propagation: PropagationConfig::default(),
            exporters: None,
        }
    }
}

/// Validate that sampling rate is between 0.0 and 1.0
fn validate_sampling_rate<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = f64::deserialize(deserializer)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(serde::de::Error::custom(format!(
            "sampling rate must be between 0.0 and 1.0, got {}",
            value
        )));
    }
    Ok(value)
}

/// Collection limits for tracing
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CollectConfig {
    /// Maximum events per span
    pub max_events_per_span: usize,

    /// Maximum attributes per span
    pub max_attributes_per_span: usize,

    /// Maximum links per span
    pub max_links_per_span: usize,

    /// Maximum attributes per event
    pub max_attributes_per_event: usize,

    /// Maximum attributes per link
    pub max_attributes_per_link: usize,
}

impl Default for CollectConfig {
    fn default() -> Self {
        Self {
            max_events_per_span: 128,
            max_attributes_per_span: 128,
            max_links_per_span: 128,
            max_attributes_per_event: 128,
            max_attributes_per_link: 128,
        }
    }
}

/// Trace context propagation configuration
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PropagationConfig {
    /// W3C Trace Context propagation
    pub trace_context: bool,

    /// AWS X-Ray propagation
    pub aws_xray: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn tracing_config_defaults() {
        let config: TracingConfig = toml::from_str("").unwrap();
        assert_debug_snapshot!(config, @r###"
        TracingConfig {
            sampling: 0.15,
            parent_based_sampler: false,
            collect: CollectConfig {
                max_events_per_span: 128,
                max_attributes_per_span: 128,
                max_links_per_span: 128,
                max_attributes_per_event: 128,
                max_attributes_per_link: 128,
            },
            propagation: PropagationConfig {
                trace_context: false,
                aws_xray: false,
            },
            exporters: None,
        }
        "###);
    }

    #[test]
    fn tracing_config_minimal() {
        let config: TracingConfig = toml::from_str(
            r#"
            sampling = 0.5
        "#,
        )
        .unwrap();

        assert_debug_snapshot!(config, @r###"
        TracingConfig {
            sampling: 0.5,
            parent_based_sampler: false,
            collect: CollectConfig {
                max_events_per_span: 128,
                max_attributes_per_span: 128,
                max_links_per_span: 128,
                max_attributes_per_event: 128,
                max_attributes_per_link: 128,
            },
            propagation: PropagationConfig {
                trace_context: false,
                aws_xray: false,
            },
            exporters: None,
        }
        "###);
    }

    #[test]
    fn tracing_config_full() {
        let config: TracingConfig = toml::from_str(
            r#"
            sampling = 1.0
            parent_based_sampler = true

            [collect]
            max_events_per_span = 256
            max_attributes_per_span = 256
            max_links_per_span = 256
            max_attributes_per_event = 256
            max_attributes_per_link = 256

            [propagation]
            trace_context = true
            aws_xray = true

            [exporters.otlp]
            enabled = true
            endpoint = "http://localhost:4317"
        "#,
        )
        .unwrap();

        assert_debug_snapshot!(config, @r#"
        TracingConfig {
            sampling: 1.0,
            parent_based_sampler: true,
            collect: CollectConfig {
                max_events_per_span: 256,
                max_attributes_per_span: 256,
                max_links_per_span: 256,
                max_attributes_per_event: 256,
                max_attributes_per_link: 256,
            },
            propagation: PropagationConfig {
                trace_context: true,
                aws_xray: true,
            },
            exporters: Some(
                ExportersConfig {
                    otlp: OtlpExporterConfig {
                        enabled: true,
                        endpoint: Url {
                            scheme: "http",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "localhost",
                                ),
                            ),
                            port: Some(
                                4317,
                            ),
                            path: "/",
                            query: None,
                            fragment: None,
                        },
                        protocol: Grpc,
                        timeout: 60s,
                        batch_export: BatchExportConfig {
                            scheduled_delay: 5s,
                            max_queue_size: 2048,
                            max_export_batch_size: 512,
                            max_concurrent_exports: 1,
                        },
                        grpc: None,
                        http: None,
                    },
                },
            ),
        }
        "#);
    }

    #[test]
    fn invalid_sampling_rate_too_high() {
        let result: Result<TracingConfig, _> = toml::from_str(
            r#"
            sampling = 1.5
        "#,
        );

        let err = result.unwrap_err();
        insta::assert_snapshot!(err.to_string(), @"TOML parse error at line 2, column 24
  |
2 |             sampling = 1.5
  |                        ^^^
sampling rate must be between 0.0 and 1.0, got 1.5
");
    }

    #[test]
    fn invalid_sampling_rate_negative() {
        let result: Result<TracingConfig, _> = toml::from_str(
            r#"
            sampling = -0.1
        "#,
        );

        let err = result.unwrap_err();
        insta::assert_snapshot!(err.to_string(), @"TOML parse error at line 2, column 24
  |
2 |             sampling = -0.1
  |                        ^^^^
sampling rate must be between 0.0 and 1.0, got -0.1
");
    }

    #[test]
    fn propagation_partial_config() {
        let config: TracingConfig = toml::from_str(
            r#"
            [propagation]
            trace_context = true
            aws_xray = true
        "#,
        )
        .unwrap();

        assert_debug_snapshot!(config.propagation, @r###"
        PropagationConfig {
            trace_context: true,
            aws_xray: true,
        }
        "###);
    }

    #[test]
    fn trace_exporters_override() {
        let config: TracingConfig = toml::from_str(
            r#"
            [exporters.otlp]
            enabled = true
            endpoint = "http://trace-collector:4317"
            protocol = "grpc"
        "#,
        )
        .unwrap();

        let exporters = config.exporters().unwrap();
        assert_debug_snapshot!(exporters, @r#"
        ExportersConfig {
            otlp: OtlpExporterConfig {
                enabled: true,
                endpoint: Url {
                    scheme: "http",
                    cannot_be_a_base: false,
                    username: "",
                    password: None,
                    host: Some(
                        Domain(
                            "trace-collector",
                        ),
                    ),
                    port: Some(
                        4317,
                    ),
                    path: "/",
                    query: None,
                    fragment: None,
                },
                protocol: Grpc,
                timeout: 60s,
                batch_export: BatchExportConfig {
                    scheduled_delay: 5s,
                    max_queue_size: 2048,
                    max_export_batch_size: 512,
                    max_concurrent_exports: 1,
                },
                grpc: None,
                http: None,
            },
        }
        "#);
    }
}
