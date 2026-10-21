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
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request
        .call()
        .map_err(|e| HttpError::Failed(e.to_string()))?;
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
