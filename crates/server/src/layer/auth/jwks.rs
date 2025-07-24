use std::{
    borrow::Cow,
    str::FromStr,
    time::{Duration, Instant},
};

use jwt_compact::jwk::JsonWebKey;
use tokio::sync::{Mutex, RwLock};
use url::Url;

pub struct JwksCache {
    url: Url,
    jwks: RwLock<Option<(Jwks<'static>, Instant)>>,
    refresh_lock: Mutex<()>,
    client: reqwest::Client,
    ttl: Option<Duration>,
}

impl JwksCache {
    pub fn new(url: Url, ttl: Option<Duration>) -> Self {
        Self {
            url,
            jwks: RwLock::new(None),
            refresh_lock: Mutex::new(()),
            client: reqwest::Client::new(),
            ttl,
        }
    }

    pub async fn get(&self) -> anyhow::Result<Jwks<'static>> {
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

        let jwks: Jwks<'static> = self.client.get(self.url.clone()).send().await?.json().await?;

        {
            let mut cache = self.jwks.write().await;
            *cache = Some((jwks.clone(), Instant::now()));
        }

        Ok(jwks)
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Jwks<'a> {
    pub keys: Vec<Jwk<'a>>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Jwk<'a> {
    #[serde(flatten)]
    pub key: JsonWebKey<'a>,
    #[serde(rename = "kid")]
    pub key_id: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, Copy)]
pub enum Alg {
    HS256,
    HS384,
    HS512,
    ES256,
    RS256,
    RS384,
    RS512,
    PS256,
    PS384,
    PS512,
    EdDSA,
}

impl FromStr for Alg {
    type Err = anyhow::Error;

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
            _ => Err(anyhow::Error::msg(format!("Unknown algorithm: {s}"))),
        }
    }
}
