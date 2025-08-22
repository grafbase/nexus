//! HTTP header types with serde support and validation.

use http::header::{HeaderName as HttpHeaderName, HeaderValue as HttpHeaderValue};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

/// A validated HTTP header name that can be serialized/deserialized.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeaderName(HttpHeaderName);

impl HeaderName {
    /// Create a new HeaderName from a static string.
    pub fn from_static(s: &'static str) -> Self {
        Self(HttpHeaderName::from_static(s))
    }

    /// Get the inner http::HeaderName.
    pub fn into_inner(self) -> HttpHeaderName {
        self.0
    }
}

impl AsRef<HttpHeaderName> for HeaderName {
    fn as_ref(&self) -> &HttpHeaderName {
        &self.0
    }
}

impl Deref for HeaderName {
    type Target = HttpHeaderName;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromStr for HeaderName {
    type Err = http::header::InvalidHeaderName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        HttpHeaderName::from_str(s).map(Self)
    }
}

impl fmt::Display for HeaderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for HeaderName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        HttpHeaderName::from_str(&s)
            .map(Self)
            .map_err(|e| serde::de::Error::custom(format!("invalid header name '{}': {}", s, e)))
    }
}

impl Serialize for HeaderName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

/// A validated HTTP header value that can be serialized/deserialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderValue(HttpHeaderValue);

impl HeaderValue {
    /// Create a new HeaderValue from a static string.
    pub fn from_static(s: &'static str) -> Self {
        Self(HttpHeaderValue::from_static(s))
    }

    /// Get the inner http::HeaderValue.
    pub fn into_inner(self) -> HttpHeaderValue {
        self.0
    }

    /// Get the header value as a string if it contains valid UTF-8.
    pub fn to_str(&self) -> Result<&str, http::header::ToStrError> {
        self.0.to_str()
    }
}

impl AsRef<HttpHeaderValue> for HeaderValue {
    fn as_ref(&self) -> &HttpHeaderValue {
        &self.0
    }
}

impl Deref for HeaderValue {
    type Target = HttpHeaderValue;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromStr for HeaderValue {
    type Err = http::header::InvalidHeaderValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        HttpHeaderValue::from_str(s).map(Self)
    }
}

impl fmt::Display for HeaderValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.to_str() {
            Ok(s) => s.fmt(f),
            Err(_) => write!(f, "<non-utf8 header value>"),
        }
    }
}

impl<'de> Deserialize<'de> for HeaderValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        HttpHeaderValue::from_str(&s)
            .map(Self)
            .map_err(|e| serde::de::Error::custom(format!("invalid header value '{}': {}", s, e)))
    }
}

impl Serialize for HeaderValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0.to_str() {
            Ok(s) => serializer.serialize_str(s),
            Err(_) => Err(serde::ser::Error::custom("header value contains non-UTF8 characters")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)] // Fields are used by Debug and Deserialize
    struct TestConfig {
        name: HeaderName,
        value: HeaderValue,
    }

    #[test]
    fn header_types_deserialize() {
        let config = indoc! {r#"
            name = "content-type"
            value = "application/json"
        "#};

        let config: TestConfig = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config, @r#"
        TestConfig {
            name: HeaderName(
                "content-type",
            ),
            value: HeaderValue(
                "application/json",
            ),
        }
        "#);
    }

    #[test]
    fn custom_headers() {
        let config = indoc! {r#"
            name = "x-custom-header"
            value = "custom-value"
        "#};

        let config: TestConfig = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config, @r#"
        TestConfig {
            name: HeaderName(
                "x-custom-header",
            ),
            value: HeaderValue(
                "custom-value",
            ),
        }
        "#);
    }

    #[test]
    fn invalid_header_name() {
        let config = indoc! {r#"
            name = "invalid header name"
            value = "test"
        "#};

        let result: Result<TestConfig, _> = toml::from_str(config);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_header_value() {
        let config = indoc! {r#"
            name = "test"
            value = "\n\r"
        "#};

        let result: Result<TestConfig, _> = toml::from_str(config);
        assert!(result.is_err());
    }
}
