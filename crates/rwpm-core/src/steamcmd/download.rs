//! Fetching Valve's bootstrap archive into memory, with a size cap.

use std::time::Duration;

use super::{Error, MAX_ARCHIVE_BYTES, SOURCE};
use crate::http::HttpError;

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
    crate::http::get(url, https_only, cap, timeout, &[]).map_err(|e| match e {
        HttpError::TooBig => Error::TooBig,
        HttpError::Failed(details) => Error::Download(details),
    })
}
