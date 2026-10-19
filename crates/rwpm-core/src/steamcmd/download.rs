//! Fetching Valve's bootstrap archive into memory, with a size cap.

use std::time::Duration;

use super::{Error, MAX_ARCHIVE_BYTES, SOURCE};

const TIMEOUT: Duration = Duration::from_secs(120);

/// Downloads Valve's steamcmd archive for this system. HTTPS only, also across redirects.
pub fn bootstrap() -> Result<Vec<u8>, Error> {
    fetch(SOURCE.url, true, MAX_ARCHIVE_BYTES, TIMEOUT)
}

/// Reads at most `cap` bytes from `url`. `https_only` is off only in tests, which talk to a
/// local server.
pub(super) fn fetch(
    url: &str,
    https_only: bool,
    cap: u64,
    timeout: Duration,
) -> Result<Vec<u8>, Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(https_only)
        .timeout_global(Some(timeout))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| Error::Download(e.to_string()))?;
    response
        .body_mut()
        .with_config()
        .limit(cap)
        .read_to_vec()
        .map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => Error::TooBig,
            other => Error::Download(other.to_string()),
        })
}
