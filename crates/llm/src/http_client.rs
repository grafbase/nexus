use std::{sync::OnceLock, time::Duration};

use axum::http;
use reqwest::Client;

pub(crate) fn default_http_client_builder() -> reqwest::ClientBuilder {
    let mut headers = http::HeaderMap::new();
    headers.insert(http::header::CONNECTION, http::HeaderValue::from_static("keep-alive"));

    Client::builder()
        .timeout(Duration::from_secs(60))
        // Hyper connection pool only exposes two parameters max idle connections per host
        // and idle connection timeout. There is not TTL on the connections themselves to
        // force a refresh, necessary if the DNS changes its records. Somehow, even within
        // a benchmark ramping *up* traffic, we do pick up DNS changes by setting a pool
        // idle timeout of 5 seconds even though in theory no connection should be idle?
        // A bit confusing, and I suspect I don't fully understand how Hyper is managing
        // connections underneath. But seems like best choice we have right now, Grafbase
        // Gateway/Apollo Router use this same default value.
        .pool_idle_timeout(Some(Duration::from_secs(5)))
        .tcp_nodelay(true)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .default_headers(headers)
}

/// Common HTTP client to re-use as much as possible the same connections.
pub(crate) fn http_client() -> reqwest::Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();

    CLIENT
        .get_or_init(|| {
            default_http_client_builder()
                .build()
                .expect("Failed to build default HTTP client")
        })
        .clone()
}
