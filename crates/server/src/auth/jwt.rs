use std::str::FromStr;

use super::AuthResult;
use super::claims::CustomClaims;
use super::error::AuthError;
use super::jwks::{Alg, Jwks, JwksCache};
use config::OauthConfig;
use http::{header::AUTHORIZATION, request::Parts};
use jwt_compact::{Algorithm, AlgorithmExt, TimeOptions, UntrustedToken, jwk::JsonWebKey};
use secrecy::SecretString;
use url::Url;

const BEARER_TOKEN_LENGTH: usize = 6;

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

    pub async fn authenticate(&self, parts: &Parts) -> AuthResult<SecretString> {
        let token_header = parts
            .headers
            .get(AUTHORIZATION)
            .ok_or(AuthError::InvalidToken("missing token"))?;

        let token_str = token_header
            .to_str()
            .map_err(|_| AuthError::InvalidToken("invalid token"))?;

        // RFC 7235: authentication scheme is case-insensitive
        // Check if it starts with "bearer" (case-insensitive) followed by space
        if token_str.len() > BEARER_TOKEN_LENGTH
            && token_str[..BEARER_TOKEN_LENGTH].eq_ignore_ascii_case("bearer")
            && token_str.chars().nth(BEARER_TOKEN_LENGTH) == Some(' ')
        {
            let token_str = &token_str[BEARER_TOKEN_LENGTH + 1..]; // Skip "Bearer " (case-insensitive)

            if token_str.is_empty() {
                return Err(AuthError::InvalidToken("missing token"));
            }

            // Continue with token validation
            let token = UntrustedToken::new(token_str).map_err(|_| AuthError::InvalidToken("invalid token"))?;
            let jwks = self.jwks_cache.get().await?;

            self.validate_token(&jwks, token).ok_or(AuthError::Unauthorized)?;

            Ok(SecretString::from(token_str.to_string()))
        } else if token_str.eq_ignore_ascii_case("bearer") {
            // Handle case where header is exactly "Bearer" with no space/token
            Err(AuthError::InvalidToken("missing token"))
        } else {
            // Not a valid Bearer format
            Err(AuthError::InvalidToken("token must be prefixed with Bearer"))
        }
    }

    fn validate_token(
        &self,
        jwks: &Jwks<'_>,
        untrusted_token: UntrustedToken<'_>,
    ) -> Option<jwt_compact::Token<CustomClaims>> {
        use jwt_compact::alg::*;

        let time_options = TimeOptions::default();
        let mut validation_results = Vec::new();

        // Collect all potential validation results to prevent timing attacks
        for jwk in &jwks.keys {
            // Always check key ID match regardless of whether we'll use this key
            let kid_matches = match (&untrusted_token.header().key_id, &jwk.key_id) {
                (Some(expected), Some(kid)) => expected == kid,
                (Some(_), None) => false,
                (None, _) => true,
            };

            if let Ok(alg) = Alg::from_str(untrusted_token.algorithm()) {
                let decode_result = match alg {
                    Alg::HS256 => decode(Hs256, &jwk.key, &untrusted_token),
                    Alg::HS384 => decode(Hs384, &jwk.key, &untrusted_token),
                    Alg::HS512 => decode(Hs512, &jwk.key, &untrusted_token),
                    Alg::ES256 => decode(Es256, &jwk.key, &untrusted_token),
                    Alg::RS256 => decode(Rsa::rs256(), &jwk.key, &untrusted_token),
                    Alg::RS384 => decode(Rsa::rs384(), &jwk.key, &untrusted_token),
                    Alg::RS512 => decode(Rsa::rs512(), &jwk.key, &untrusted_token),
                    Alg::PS256 => decode(Rsa::ps256(), &jwk.key, &untrusted_token),
                    Alg::PS384 => decode(Rsa::ps384(), &jwk.key, &untrusted_token),
                    Alg::PS512 => decode(Rsa::ps512(), &jwk.key, &untrusted_token),
                    Alg::EdDSA => decode(Ed25519, &jwk.key, &untrusted_token),
                };

                if let Some(token) = decode_result {
                    let claims = token.claims();

                    let time_valid = claims.validate_expiration(&time_options).is_ok()
                        && (claims.not_before.is_none() || claims.validate_maturity(&time_options).is_ok());

                    let issuer_valid = self.validate_issuer(&claims.custom);
                    let audience_valid = self.validate_audience(&claims.custom);

                    validation_results.push((kid_matches, time_valid, issuer_valid, audience_valid, token));
                }
            }
        }

        // Find the first valid token that matches all criteria
        validation_results
            .into_iter()
            .find(|(kid_matches, time_valid, issuer_valid, audience_valid, _)| {
                *kid_matches && *time_valid && *issuer_valid && *audience_valid
            })
            .map(|(_, _, _, _, token)| token)
    }

    fn validate_issuer(&self, claims: &CustomClaims) -> bool {
        let Some(expected_issuer) = &self.config.expected_issuer else {
            // If no expected issuer is configured, skip validation
            return true;
        };

        match claims.get_issuer() {
            Some(issuer) if issuer == expected_issuer => {
                log::debug!("Token issuer validation passed");
                true
            }
            Some(_) => {
                log::debug!("Token rejected: invalid issuer");
                false
            }
            None => {
                log::debug!("Token rejected: missing issuer claim");
                false
            }
        }
    }

    fn validate_audience(&self, claims: &CustomClaims) -> bool {
        let Some(expected_audience) = &self.config.expected_audience else {
            // If no expected audience is configured, skip validation
            return true;
        };

        if claims.has_audience(expected_audience) {
            log::debug!("Token audience validation passed");
            true
        } else {
            log::debug!("Token rejected: audience validation failed");
            false
        }
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
