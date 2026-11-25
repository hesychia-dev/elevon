use axum::http::HeaderValue;
use pingora::proxy::Session;

pub fn extract_domain(host: &HeaderValue) -> Option<&str> {
    let host_str = host.to_str().ok()?;

    let mut parts = host_str.rsplitn(2, ':');
    let first_part = parts.next()?;

    if let Some(domain_part) = parts.next() {
        // If there was a colon, the second part yielded by rsplitn is the domain
        Some(domain_part)
    } else {
        // If there was no colon, the whole string is the domain
        Some(first_part)
    }
}

pub fn extract_host(session: &Session) -> Option<&HeaderValue> {
    session.req_header().headers.get("Host")
}
