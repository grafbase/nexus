use std::collections::HashMap;

use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Default, Clone, Debug)]
pub struct Authentication {
    pub nexus: Option<NexusToken>,
    pub has_anthropic_authorization: bool,
}

#[derive(Clone, Debug)]
pub struct NexusToken {
    pub raw: SecretString,
    pub token: jwt_compact::Token<Claims>,
}

impl std::ops::Deref for NexusToken {
    type Target = jwt_compact::Token<Claims>;
    fn deref(&self) -> &Self::Target {
        &self.token
    }
}

/// Custom JWT claims that include OAuth 2.0 scopes and standard JWT claims
#[serde_with::serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Issuer claim - identifies the principal that issued the JWT
    #[serde(default, rename = "iss")]
    pub issuer: Option<String>,

    /// Audience claim - identifies the recipients that the JWT is intended for
    #[serde_as(deserialize_as = "Option<serde_with::OneOrMany<_>>")]
    #[serde(default, rename = "aud")]
    pub audience: Option<Vec<String>>,

    /// Subject claim - identifies the principal that is the subject of the JWT
    #[serde(default, rename = "sub")]
    pub subject: Option<String>,

    /// Additional claims for flexible access to custom fields
    #[serde(flatten)]
    pub additional: HashMap<String, Value>,
}

impl Claims {
    /// Extract a claim value by path, supporting nested claims.
    ///
    /// Paths can be simple (e.g., "sub") or nested (e.g., "user.plan").
    pub fn get_claim(&self, path: &str) -> Option<String> {
        // Handle standard claims
        match path {
            "iss" => return self.issuer.clone(),
            "sub" => return self.subject.clone(),
            "aud" => return self.audience.as_ref().and_then(|audiences| audiences.first().cloned()),
            _ => {}
        }

        // Handle nested paths in additional claims
        let mut parts = path.split('.');
        let first = parts.next()?;
        let current = parts.fold(self.additional.get(first).unwrap_or(&Value::Null), |current, part| {
            current.get(part).unwrap_or(&Value::Null)
        });

        // Convert the final value to string
        match current {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
}
