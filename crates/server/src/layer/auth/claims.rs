use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Custom JWT claims that include OAuth 2.0 scopes and standard JWT claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomClaims {
    /// Issuer claim - identifies the principal that issued the JWT
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// Audience claim - identifies the recipients that the JWT is intended for
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aud: Option<Value>,

    /// OAuth 2.0 scopes - can be either a string or array
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Value>,

    /// Alternative field name for scopes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Value>,
}

impl CustomClaims {
    /// Extract scopes from the claims
    pub fn get_scopes(&self) -> Vec<String> {
        let scope_value = self.scope.as_ref().or(self.scopes.as_ref());

        match scope_value {
            Some(Value::String(s)) => s.split_whitespace().map(String::from).collect(),
            Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            _ => Vec::new(),
        }
    }

    /// Get the issuer claim
    pub fn get_issuer(&self) -> Option<&str> {
        self.iss.as_deref()
    }

    /// Get the audience claim as a list of strings
    pub fn get_audiences(&self) -> Vec<String> {
        match &self.aud {
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            _ => Vec::new(),
        }
    }

    /// Check if the given audience is present in the audience claim
    pub fn has_audience(&self, expected_audience: &str) -> bool {
        self.get_audiences().iter().any(|aud| aud == expected_audience)
    }
}
