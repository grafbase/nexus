use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Custom JWT claims that include OAuth 2.0 scopes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomClaims {
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
}
