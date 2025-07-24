use std::{
    borrow::Cow,
    str::FromStr,
    time::{Duration, Instant},
};

use jwt_compact::jwk::JsonWebKey;
use tokio::sync::{Mutex, RwLock};
use url::Url;

use super::{AuthResult, error::AuthError};

/// A thread-safe cache for JSON Web Key Sets (JWKS) with TTL support.
///
/// This cache fetches JWKS from a remote URL and caches them in memory with an optional
/// time-to-live (TTL). It uses a double-checked locking pattern to ensure that only
/// one request is made to refresh the cache when it expires, even in concurrent scenarios.
pub struct JwksCache {
    /// The URL to fetch JWKS from
    url: Url,
    /// Cached JWKS with timestamp of when it was cached
    jwks: RwLock<Option<(Jwks<'static>, Instant)>>,
    /// Mutex to ensure only one refresh operation happens at a time
    refresh_lock: Mutex<()>,
    /// HTTP client for making requests
    client: reqwest::Client,
    /// Time-to-live for cached JWKS. If None, cache never expires
    ttl: Option<Duration>,
}

impl JwksCache {
    /// Creates a new JWKS cache.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to fetch JWKS from
    /// * `ttl` - Optional time-to-live for cached JWKS. If None, cache never expires
    pub fn new(url: Url, ttl: Option<Duration>) -> Self {
        Self {
            url,
            jwks: RwLock::new(None),
            refresh_lock: Mutex::new(()),
            client: reqwest::Client::new(),
            ttl,
        }
    }

    /// Retrieves JWKS from cache or fetches from remote URL if cache is expired or empty.
    ///
    /// This method uses a double-checked locking pattern to ensure thread safety and
    /// prevent multiple concurrent requests to the same URL.
    pub async fn get(&self) -> AuthResult<Jwks<'static>> {
        if let Some((jwks, cached_at)) = self.jwks.read().await.as_ref() {
            match self.ttl {
                Some(ttl) if cached_at.elapsed() > ttl => {}
                _ => return Ok(jwks.clone()),
            }
        }

        let _refresh_guard = self.refresh_lock.lock().await;

        // Double-check: another task might have refreshed while we were waiting
        if let Some((jwks, cached_at)) = self.jwks.read().await.as_ref() {
            match self.ttl {
                Some(ttl) if cached_at.elapsed() > ttl => {}
                _ => return Ok(jwks.clone()),
            }
        }

        let jwks: Jwks<'static> = self
            .client
            .get(self.url.clone())
            .send()
            .await
            .map_err(|_| AuthError::Internal)?
            .json()
            .await
            .map_err(|_| AuthError::Internal)?;

        {
            let mut cache = self.jwks.write().await;
            *cache = Some((jwks.clone(), Instant::now()));
        }

        Ok(jwks)
    }
}

/// A JSON Web Key Set (JWKS) containing a collection of cryptographic keys.
///
/// This structure represents the standard JWKS format as defined in RFC 7517.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Jwks<'a> {
    /// Array of JSON Web Keys
    pub keys: Vec<Jwk<'a>>,
}

/// A JSON Web Key (JWK) representing a cryptographic key.
///
/// This structure represents a single key within a JWKS, containing the key material
/// and optional metadata like the key ID.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Jwk<'a> {
    /// The cryptographic key material
    #[serde(flatten)]
    pub key: JsonWebKey<'a>,
    /// Optional key identifier used to match keys with JWT headers
    #[serde(rename = "kid")]
    pub key_id: Option<Cow<'a, str>>,
}

/// Supported cryptographic algorithms for JWT signatures.
///
/// This enum covers the most commonly used algorithms in JWT implementations,
/// including HMAC, RSA, ECDSA, and EdDSA variants.
#[derive(Debug, Clone, Copy)]
pub enum Alg {
    /// HMAC using SHA-256
    HS256,
    /// HMAC using SHA-384
    HS384,
    /// HMAC using SHA-512
    HS512,
    /// ECDSA using P-256 and SHA-256
    ES256,
    /// RSASSA-PKCS1-v1_5 using SHA-256
    RS256,
    /// RSASSA-PKCS1-v1_5 using SHA-384
    RS384,
    /// RSASSA-PKCS1-v1_5 using SHA-512
    RS512,
    /// RSASSA-PSS using SHA-256 and MGF1 with SHA-256
    PS256,
    /// RSASSA-PSS using SHA-384 and MGF1 with SHA-384
    PS384,
    /// RSASSA-PSS using SHA-512 and MGF1 with SHA-512
    PS512,
    /// EdDSA signature algorithms
    EdDSA,
}

impl FromStr for Alg {
    type Err = AuthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "HS256" => Ok(Alg::HS256),
            "HS384" => Ok(Alg::HS384),
            "HS512" => Ok(Alg::HS512),
            "ES256" => Ok(Alg::ES256),
            "RS256" => Ok(Alg::RS256),
            "RS384" => Ok(Alg::RS384),
            "RS512" => Ok(Alg::RS512),
            "PS256" => Ok(Alg::PS256),
            "PS384" => Ok(Alg::PS384),
            "PS512" => Ok(Alg::PS512),
            "EdDSA" => Ok(Alg::EdDSA),
            _ => Err(AuthError::InvalidToken("unsupported algorithm")),
        }
    }
}
