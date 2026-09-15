//! Remote image fetching over HTTP(S) with a size cap.

use std::io::Read;

use super::image::{ensure_within_limit, MAX_IMAGE_BYTES};

/// Fetch bytes from an http(s) URL with a 30s timeout and a UA header.
pub(crate) fn fetch_url(url: &str) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let response = agent
        .get(url)
        .set("User-Agent", "aif-project/1.0")
        .call()
        .map_err(|e| format!("Unable to fetch image from URL {}: {}", url, e))?;
    let mut buf: Vec<u8> = Vec::new();
    response
        .into_reader()
        .take((MAX_IMAGE_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read image from {}: {}", url, e))?;
    ensure_within_limit(buf.len()).map_err(|e| e.0)?;
    Ok(buf)
}
