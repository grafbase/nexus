use std::time::Duration;

use axum::http;
use reqwest::Client;

pub(super) fn default_http_client_builder(mut headers: http::HeaderMap) -> reqwest::ClientBuilder {
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
