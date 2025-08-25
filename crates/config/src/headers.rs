//! Header transformation rules for HTTP requests to downstream services.

use crate::http_types::{HeaderName, HeaderValue};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Deserializer};
use std::borrow::Cow;

/// A header name can be provided either as a regex pattern or as a static name.
#[derive(Debug, Clone, Deserialize)]
pub enum NameOrPattern {
    /// A regex pattern matching multiple headers.
    #[serde(rename = "pattern")]
    Pattern(NamePattern),
    /// A static single name.
    #[serde(rename = "name")]
    Name(HeaderName),
}

/// A case-insensitive regex pattern for matching header names.
#[derive(Debug, Clone)]
pub struct NamePattern(pub Regex);

impl<'de> Deserialize<'de> for NamePattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let pattern = Cow::<'de, str>::deserialize(deserializer)?;
        Ok(NamePattern(
            RegexBuilder::new(&pattern)
                // Header names are case insensitive per HTTP spec
                .case_insensitive(true)
                .build()
                .map_err(serde::de::Error::custom)?,
        ))
    }
}

/// A header transformation rule for LLM providers (supports all operations).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case")]
pub enum HeaderRule {
    /// Forward a header from the incoming request.
    Forward(HeaderForward),
    /// Insert a new header with a static or templated value.
    Insert(HeaderInsert),
    /// Remove headers matching a name or pattern.
    Remove(HeaderRemove),
    /// Forward the header together with a renamed copy.
    RenameDuplicate(HeaderRenameDuplicate),
}

/// A header transformation rule for MCP providers (currently only supports insert).
/// Uses enum format for future extensibility as MCP protocol evolves.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case")]
pub enum McpHeaderRule {
    /// Insert a new header with a static value.
    Insert(HeaderInsert),
}

/// Header forwarding rules.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderForward {
    /// Name or pattern of the header to be forwarded.
    #[serde(flatten)]
    pub name: NameOrPattern,
    /// If header is not present, insert this value.
    #[serde(default)]
    pub default: Option<HeaderValue>,
    /// Use this name instead of the original when forwarding.
    #[serde(default)]
    pub rename: Option<HeaderName>,
}

/// Header insertion rules.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderInsert {
    /// The name of the header.
    pub name: HeaderName,
    /// The value of the header (supports {{ env.VAR }} templating).
    pub value: HeaderValue,
}

/// Header removal rules.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderRemove {
    /// Removes the header with a static name or matching a regex pattern.
    #[serde(flatten)]
    pub name: NameOrPattern,
}

/// Header rename with duplication rules.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderRenameDuplicate {
    /// Name of the header to be duplicated.
    pub name: HeaderName,
    /// If header is not present, insert this value.
    #[serde(default)]
    pub default: Option<HeaderValue>,
    /// The new name for the duplicated header.
    pub rename: HeaderName,
}
