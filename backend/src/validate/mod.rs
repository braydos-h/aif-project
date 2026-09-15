//! Image reference handling: URL fetch, base64 decode, magic-byte validation.
//!
//! Mirrors `CowWeightEstimator._to_base64_image` / `_validate_image_bytes`
//! in `aif/estimator.py`.
//!
//! Split into focused submodules:
//! - [`base64`] — strict base64 codec.
//! - [`image`] — magic-byte validation and size limits.
//! - [`fetch`] — remote `http(s)` image downloads.

pub mod base64;
pub mod fetch;
pub mod image;

pub use base64::base64_encode;
pub use image::{ensure_within_limit, validate_image_bytes, MAX_IMAGE_BYTES};

/// Error raised when the supplied image bytes are not a recognised format.
#[derive(Debug)]
pub struct ImageValidationError(pub String);

impl std::fmt::Display for ImageValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ImageValidationError {}

/// Return raw base64 image bytes from a URL, a data: URI, or a base64 string.
///
/// Validates that the decoded bytes look like a supported image format.
/// Mirrors `CowWeightEstimator._to_base64_image`.
pub fn to_base64_image(image_reference: &str) -> Result<String, ImageValidationError> {
    if image_reference.starts_with("http://") || image_reference.starts_with("https://") {
        let image_bytes = fetch::fetch_url(image_reference).map_err(ImageValidationError)?;
        validate_image_bytes(&image_bytes)?;
        return Ok(base64_encode(&image_bytes));
    }

    let mut stripped = image_reference;
    // Accept any base64 data URI (some Windows MIME databases label WebP as
    // application/octet-stream).
    let lower = image_reference.to_ascii_lowercase();
    if let Some(idx) = lower.find(";base64,") {
        if lower.starts_with("data:") {
            stripped = &image_reference[idx + ";base64,".len()..];
        }
    }

    let decoded = base64::decode_base64(stripped).map_err(ImageValidationError)?;
    validate_image_bytes(&decoded)?;
    Ok(stripped.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes() -> Vec<u8> {
        b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\nIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\r\n-\xb4\x00\x00\x00\x00IEND\xaeB`\x82".to_vec()
    }

    #[test]
    fn data_uri_stripped() {
        let b64 = base64_encode(&png_bytes());
        let uri = format!("data:image/png;base64,{}", b64);
        let out = to_base64_image(&uri).unwrap();
        assert_eq!(out, b64);
    }

    #[test]
    fn invalid_base64_rejected() {
        // "QUJD" decodes to "ABC" — valid base64 but not a valid image.
        let err = to_base64_image("QUJD").unwrap_err();
        assert!(err.0.contains("do not match a supported format"));
        // "!!!" is not valid base64 at all.
        let err2 = to_base64_image("!!!").unwrap_err();
        assert!(err2.0.contains("not valid base64"));
    }
}
