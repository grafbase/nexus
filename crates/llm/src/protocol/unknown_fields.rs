use std::collections::HashMap;

/// Represents arbitrary additional fields in a message.
#[derive(Default, Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnknownFields(HashMap<String, serde_json::Value>); // TODO: use a more efficient memory representation.
