//! One small GET helper shared by every download: a size cap, a timeout, and HTTPS only.

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The body is larger than the cap.
    TooBig,
    /// The request failed: connection, status, TLS, or a body read error.
    Failed(String),
}

/// Reads at most `cap` bytes from `url`, sending `headers`. `https_only` is off only in
/// tests, which talk to a local server.
pub fn get(
    url: &str,
    https_only: bool,
    cap: u64,
    timeout: Duration,
    headers: &[(&str, &str)],
) -> Result<Vec<u8>, HttpError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(https_only)
        .timeout_global(Some(timeout))
        .build()
        .into();
    let mut response = call(&agent, url, headers)?;
    response
        .body_mut()
        .with_config()
        .limit(cap)
        .read_to_vec()
        .map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => HttpError::TooBig,
            other => HttpError::Failed(other.to_string()),
        })
}

fn call(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<ureq::http::Response<ureq::Body>, HttpError> {
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.call().map_err(|e| HttpError::Failed(e.to_string()))
}

/// Streams `url` into `dest` without holding it in memory, and returns the lowercase hex
/// SHA-256 of what was written. Fails with [`HttpError::TooBig`] past `cap` bytes.
pub fn download_to(
    url: &str,
    https_only: bool,
    dest: &mut impl std::io::Write,
    cap: u64,
    timeout: Duration,
    headers: &[(&str, &str)],
) -> Result<String, HttpError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(https_only)
        .timeout_global(Some(timeout))
        .build()
        .into();
    let mut response = call(&agent, url, headers)?;
    // One byte past the cap, so the count below is what reports an oversize body.
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(cap.saturating_add(1))
        .reader();
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| HttpError::Failed(e.to_string()))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > cap {
            return Err(HttpError::TooBig);
        }
        hasher.update(&buf[..n]);
        dest.write_all(&buf[..n])
            .map_err(|e| HttpError::Failed(e.to_string()))?;
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::*;

    fn serve_once(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://127.0.0.1:{port}/file")
    }

    const WAIT: Duration = Duration::from_secs(10);

    #[test]
    fn a_download_is_written_out_and_hashed() {
        let url = serve_once(b"abc".to_vec());
        let mut out = Vec::new();
        let hash = download_to(&url, false, &mut out, 16, WAIT, &[]).expect("download");
        assert_eq!(out, b"abc");
        // SHA-256 of "abc"
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_download_exactly_at_the_cap_is_fine_and_one_byte_over_is_not() {
        let url = serve_once(vec![7; 16]);
        assert!(download_to(&url, false, &mut Vec::new(), 16, WAIT, &[]).is_ok());
        let url = serve_once(vec![7; 17]);
        assert_eq!(
            download_to(&url, false, &mut Vec::new(), 16, WAIT, &[]),
            Err(HttpError::TooBig)
        );
    }

    #[test]
    fn a_download_over_plain_http_is_refused_when_https_only() {
        let url = serve_once(b"abc".to_vec());
        assert!(matches!(
            download_to(&url, true, &mut Vec::new(), 16, WAIT, &[]),
            Err(HttpError::Failed(_))
        ));
    }
}
