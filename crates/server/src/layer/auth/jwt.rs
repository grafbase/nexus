use std::str::FromStr;

use super::claims::CustomClaims;
use super::jwks::{Alg, Jwks, JwksCache};
use config::OauthConfig;
use http::{header::AUTHORIZATION, request::Parts};
use jwt_compact::{Algorithm, AlgorithmExt, TimeOptions, UntrustedToken, jwk::JsonWebKey};
use url::Url;

pub struct JwtAuth {
    config: OauthConfig,
    jwks_cache: JwksCache,
}

impl JwtAuth {
    pub fn new(config: OauthConfig) -> Self {
        let jwks_cache = JwksCache::new(config.url.clone(), config.poll_interval);

        JwtAuth { config, jwks_cache }
    }

    pub fn metadata_endpoint(&self) -> Url {
        self.config.protected_resource.resource_documentation()
    }

    pub async fn authenticate(&self, parts: &Parts) -> anyhow::Result<()> {
        let token_str = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(|| anyhow::Error::msg("Failed to parse authorization header"))?;

        let token = UntrustedToken::new(token_str)?;
        let jwks = self.jwks_cache.get().await?;

        self.validate_token(&jwks, token)
            .ok_or_else(|| anyhow::Error::msg("Failed to decode token"))?;

        Ok(())
    }

    fn validate_token(
        &self,
        jwks: &Jwks<'_>,
        untrusted_token: UntrustedToken<'_>,
    ) -> Option<jwt_compact::Token<CustomClaims>> {
        use jwt_compact::alg::*;

        let time_options = TimeOptions::default();

        jwks.keys
            .iter()
            // If 'kid' was provided, we only use the jwk with the correct id.
            .filter(|jwk| match (&untrusted_token.header().key_id, &jwk.key_id) {
                (Some(expected), Some(kid)) => expected == kid,
                (Some(_), None) => false,
                (None, _) => true,
            })
            .map(|jwk| &jwk.key)
            .filter_map(|jwk| match Alg::from_str(untrusted_token.algorithm()).ok()? {
                Alg::HS256 => decode(Hs256, jwk, &untrusted_token),
                Alg::HS384 => decode(Hs384, jwk, &untrusted_token),
                Alg::HS512 => decode(Hs512, jwk, &untrusted_token),
                Alg::ES256 => decode(Es256, jwk, &untrusted_token),
                Alg::RS256 => decode(Rsa::rs256(), jwk, &untrusted_token),
                Alg::RS384 => decode(Rsa::rs384(), jwk, &untrusted_token),
                Alg::RS512 => decode(Rsa::rs512(), jwk, &untrusted_token),
                Alg::PS256 => decode(Rsa::ps256(), jwk, &untrusted_token),
                Alg::PS384 => decode(Rsa::ps384(), jwk, &untrusted_token),
                Alg::PS512 => decode(Rsa::ps512(), jwk, &untrusted_token),
                Alg::EdDSA => decode(Ed25519, jwk, &untrusted_token),
            })
            .find(|token| {
                let claims = token.claims();

                if claims.validate_expiration(&time_options).is_err() {
                    return false;
                }

                if claims.not_before.is_some() && claims.validate_maturity(&time_options).is_err() {
                    return false;
                }

                self.validate_scopes(claims.custom.get_scopes())
            })
    }

    fn validate_scopes(&self, scopes: Vec<String>) -> bool {
        let Some(supported_scopes) = &self.config.protected_resource.scopes_supported else {
            return true;
        };

        if scopes.is_empty() {
            log::debug!("Token rejected: no scopes present but scopes are required");
            return false;
        }

        scopes.iter().all(|scope| supported_scopes.contains(scope))
    }
}

fn decode<A: Algorithm>(
    alg: A,
    jwk: &JsonWebKey<'_>,
    untrusted_token: &UntrustedToken<'_>,
) -> Option<jwt_compact::Token<CustomClaims>>
where
    A::VerifyingKey: std::fmt::Debug + for<'a> TryFrom<&'a JsonWebKey<'a>>,
{
    let key = A::VerifyingKey::try_from(jwk).ok()?;
    alg.validator(&key).validate(untrusted_token).ok()
}
