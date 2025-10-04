use std::sync::OnceLock;

use http::header;
use reqwest::RequestBuilder;

pub(crate) fn insert_proxied_headers_into(mut request: RequestBuilder, headers: &http::HeaderMap) -> RequestBuilder {
    static HOP_BY_HOP_HEADER_NAMES: OnceLock<[&'static str; 21]> = OnceLock::new();
    let hop_by_hop_header_names = HOP_BY_HOP_HEADER_NAMES.get_or_init(|| {
        let mut names = [
            header::ACCEPT.as_str(),
            header::ACCEPT_CHARSET.as_str(),
            header::ACCEPT_ENCODING.as_str(),
            header::ACCEPT_RANGES.as_str(),
            header::CONTENT_LENGTH.as_str(),
            header::CONTENT_TYPE.as_str(),
            // hop-by-hop headers
            header::CONNECTION.as_str(),
            "keep-alive",
            header::PROXY_AUTHENTICATE.as_str(),
            header::PROXY_AUTHORIZATION.as_str(),
            header::TE.as_str(),
            header::TRAILER.as_str(),
            header::TRANSFER_ENCODING.as_str(),
            header::UPGRADE.as_str(),
            header::ORIGIN.as_str(),
            header::HOST.as_str(),
            header::SEC_WEBSOCKET_VERSION.as_str(),
            header::SEC_WEBSOCKET_KEY.as_str(),
            header::SEC_WEBSOCKET_ACCEPT.as_str(),
            header::SEC_WEBSOCKET_PROTOCOL.as_str(),
            header::SEC_WEBSOCKET_EXTENSIONS.as_str(),
        ];
        names.sort_unstable();
        names
    });

    for (name, value) in headers.iter() {
        if hop_by_hop_header_names.contains(&name.as_str()) {
            continue;
        }
        request = request.header(name, value);
    }

    request
}

/// Similar to `strict_insert_proxied_headers_into` but allows `Content-*` and `Accept-*` headers
/// to be forwarded as well in cases where we act as a transparent proxy.
pub(crate) fn insert_proxied_headers_and_content_accept_into(
    mut request: RequestBuilder,
    headers: &http::HeaderMap,
) -> RequestBuilder {
    static HOP_BY_HOP_HEADER_NAMES: OnceLock<[&'static str; 15]> = OnceLock::new();
    let hop_by_hop_header_names = HOP_BY_HOP_HEADER_NAMES.get_or_init(|| {
        let mut names = [
            // hop-by-hop headers
            header::CONNECTION.as_str(),
            "keep-alive",
            header::PROXY_AUTHENTICATE.as_str(),
            header::PROXY_AUTHORIZATION.as_str(),
            header::TE.as_str(),
            header::TRAILER.as_str(),
            header::TRANSFER_ENCODING.as_str(),
            header::UPGRADE.as_str(),
            header::ORIGIN.as_str(),
            header::HOST.as_str(),
            header::SEC_WEBSOCKET_VERSION.as_str(),
            header::SEC_WEBSOCKET_KEY.as_str(),
            header::SEC_WEBSOCKET_ACCEPT.as_str(),
            header::SEC_WEBSOCKET_PROTOCOL.as_str(),
            header::SEC_WEBSOCKET_EXTENSIONS.as_str(),
        ];
        names.sort_unstable();
        names
    });

    for (name, value) in headers.iter() {
        if hop_by_hop_header_names.contains(&name.as_str()) {
            continue;
        }
        request = request.header(name, value);
    }

    request
}
