use std::sync::{Arc, Mutex};

/// Standard HTTP headers that are always present and not interesting for header rule tests
const STANDARD_HEADERS: &[&str] = &[
    // Standard HTTP headers
    "accept",
    "accept-charset",
    "accept-encoding",
    "accept-language",
    "accept-ranges",
    "cache-control",
    "connection",
    "content-encoding",
    "content-length",
    "content-type",
    "cookie",
    "date",
    "expect",
    "host",
    "if-match",
    "if-modified-since",
    "if-none-match",
    "if-range",
    "if-unmodified-since",
    "origin",
    "pragma",
    "range",
    "referer",
    "transfer-encoding",
    "user-agent",
    "upgrade",
    "via",
    "warning",
    // Authentication headers (provider-specific)
    "authorization",
    "www-authenticate",
    "proxy-authenticate",
    "proxy-authorization",
    // Provider-specific API headers
    "x-api-key",         // Common API key header
    "anthropic-version", // Anthropic-specific
    "openai-beta",       // OpenAI-specific
    "google-api-client", // Google-specific
];

/// A test helper for capturing and inspecting headers sent to mock servers
pub struct HeaderRecorder {
    headers: Arc<Mutex<Vec<(String, String)>>>,
}

impl HeaderRecorder {
    /// Create a new header recorder from an Arc<Mutex<Vec>>
    pub(crate) fn new(headers: Arc<Mutex<Vec<(String, String)>>>) -> Self {
        Self { headers }
    }

    /// Get the headers that were affected by header rules (filters out standard HTTP headers)
    pub fn captured_headers(&self) -> Vec<(String, String)> {
        self.headers
            .lock()
            .unwrap()
            .clone()
            .into_iter()
            .filter(|(name, _)| {
                let name_lower = name.to_lowercase();
                !STANDARD_HEADERS.iter().any(|&std_header| name_lower == std_header)
            })
            .collect()
    }

    /// Get all captured headers without filtering (for debugging)
    pub fn all_headers(&self) -> Vec<(String, String)> {
        self.headers.lock().unwrap().clone()
    }
}
