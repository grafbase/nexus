//! Redis connection pool implementation based on Grafbase's approach.

use std::sync::atomic::{AtomicUsize, Ordering};

use deadpool::managed::{self, Metrics};
use redis::{Client, RedisError, RedisResult, aio::MultiplexedConnection};

use config::{RedisConfig, RedisTlsConfig};

/// Redis connection pool.
pub type Pool = deadpool::managed::Pool<Manager>;

/// Manager for Redis connections.
#[derive(Debug)]
pub struct Manager {
    client: Client,
    ping_number: AtomicUsize,
}

impl Manager {
    /// Create a new Redis pool manager.
    pub fn new(config: &RedisConfig) -> RedisResult<Self> {
        let client = if let Some(tls_config) = &config.tls {
            // For Redis with TLS, build certificates or use insecure mode
            let tls_certs = build_tls_certificates(tls_config)?;

            Client::build_with_tls(config.url.clone(), tls_certs)?
        } else {
            Client::open(config.url.as_str())?
        };

        Ok(Self {
            client,
            ping_number: AtomicUsize::new(0),
        })
    }
}

impl managed::Manager for Manager {
    type Type = MultiplexedConnection;
    type Error = RedisError;

    async fn create(&self) -> Result<MultiplexedConnection, Self::Error> {
        let conn = self.client.get_multiplexed_async_connection().await?;
        Ok(conn)
    }

    async fn recycle(&self, conn: &mut MultiplexedConnection, _: &Metrics) -> managed::RecycleResult<Self::Error> {
        let ping_number = self.ping_number.fetch_add(1, Ordering::Relaxed).to_string();

        let (n,) = redis::Pipeline::with_capacity(2)
            .cmd("UNWATCH")
            .ignore()
            .cmd("PING")
            .arg(&ping_number)
            .query_async::<(String,)>(conn)
            .await?;

        if n == ping_number {
            Ok(())
        } else {
            Err(managed::RecycleError::message("Invalid PING response"))
        }
    }
}

/// Build TLS certificates from configuration.
fn build_tls_certificates(config: &RedisTlsConfig) -> RedisResult<redis::TlsCertificates> {
    use redis::ClientTlsConfig;

    // For insecure mode, we'll use the CA cert from the container
    if config.insecure.unwrap_or(false) {
        // Try to load the CA cert for self-signed certificates
        let ca_path = config
            .ca_cert_path
            .as_deref()
            .unwrap_or("./crates/integration-tests/docker/redis/tls/ca.crt");

        let root_cert = std::fs::read(ca_path).ok();

        return Ok(redis::TlsCertificates {
            client_tls: None,
            root_cert,
        });
    }

    let mut client_tls = None;
    let mut root_cert = None;

    // Load client certificate and key if provided
    if let (Some(cert_path), Some(key_path)) = (&config.client_cert_path, &config.client_key_path) {
        let cert = std::fs::read(cert_path).map_err(|e| {
            RedisError::from((
                redis::ErrorKind::IoError,
                "Failed to read client certificate",
                e.to_string(),
            ))
        })?;
        let key = std::fs::read(key_path)
            .map_err(|e| RedisError::from((redis::ErrorKind::IoError, "Failed to read client key", e.to_string())))?;

        client_tls = Some(ClientTlsConfig {
            client_cert: cert,
            client_key: key,
        });
    }

    // Load CA certificate if provided
    if let Some(ca_path) = &config.ca_cert_path {
        root_cert = Some(std::fs::read(ca_path).map_err(|e| {
            RedisError::from((
                redis::ErrorKind::IoError,
                "Failed to read CA certificate",
                e.to_string(),
            ))
        })?);
    }

    Ok(redis::TlsCertificates { client_tls, root_cert })
}

/// Create a Redis connection pool from configuration.
pub fn create_pool(config: &RedisConfig) -> RedisResult<Pool> {
    let manager = Manager::new(config)?;

    let mut pool_config = deadpool::managed::PoolConfig::default();

    if let Some(max_size) = config.pool.max_size {
        pool_config.max_size = max_size;
    }

    if let Some(timeout_create) = config.pool.timeout_create {
        pool_config.timeouts.create = Some(timeout_create);
    }

    if let Some(timeout_wait) = config.pool.timeout_wait {
        pool_config.timeouts.wait = Some(timeout_wait);
    }

    if let Some(timeout_recycle) = config.pool.timeout_recycle {
        pool_config.timeouts.recycle = Some(timeout_recycle);
    }

    let pool = Pool::builder(manager)
        .config(pool_config)
        .runtime(deadpool::Runtime::Tokio1)
        .build()
        .map_err(|e| RedisError::from((redis::ErrorKind::IoError, "Failed to create pool", e.to_string())))?;

    Ok(pool)
}
