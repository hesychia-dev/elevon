use axum::http::HeaderValue;
use pingora::{Error, ErrorType, http::ResponseHeader, proxy::Session};
use tokio::fs::File;
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;

pub fn error_response(message: impl Into<String>, status_code: u16) -> Box<Error> {
    Error::explain(ErrorType::HTTPStatus(status_code), message.into())
}

fn extract_domain(host: &HeaderValue) -> Option<&str> {
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

pub fn extract_domain_and_path(session: &Session) -> pingora::Result<(String, String)> {
    let host = session
        .req_header()
        .headers
        .get("Host")
        .ok_or_else(|| error_response("no Host header", 502))?;

    let domain = extract_domain(host)
        .ok_or_else(|| error_response("could not get domain out of host", 502))?;

    let path = session.req_header().uri.path();

    Ok((domain.to_string(), path.to_string()))
}

pub async fn file_response(
    session: &mut Session,
    file_path: std::path::PathBuf,
) -> pingora::Result<()> {
    let file = File::open(&file_path)
        .await
        .map_err(|_| error_response("could not open requested file", 500))?;

    let mut header = ResponseHeader::build(200, None)?;
    let mime_guess = mime_guess::from_path(&file_path)
        .first_raw()
        .unwrap_or("application/octet-stream");

    header.insert_header("Content-Type", mime_guess)?;

    session
        .write_response_header(Box::new(header), false)
        .await?;

    let mut file_stream = ReaderStream::new(file);
    while let Some(chunk) = file_stream.next().await {
        let chunk = chunk.map_err(|_| error_response("failed to stream a file chunk", 500))?;
        session.write_response_body(Some(chunk), false).await?;
    }

    session.write_response_body(None, true).await?;

    Ok(())
}
