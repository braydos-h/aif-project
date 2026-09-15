//! Image magic-byte validation and download size limits.
//!
//! Mirrors `CowWeightEstimator._validate_image_bytes` in `aif/estimator.py`.

use super::ImageValidationError;

const MAGIC_JPEG: &[u8] = &[0xFF, 0xD8, 0xFF];
const MAGIC_PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const MAGIC_GIF87A: &[u8] = b"GIF87a";
const MAGIC_GIF89A: &[u8] = b"GIF89a";
const MAGIC_BMP: &[u8] = b"BM";
const MAGIC_RIFF: &[u8] = b"RIFF";

/// Maximum accepted image download size (mirrors the 20 MiB API body limit).
pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

/// Raise an error unless the bytes look like a supported image format.
pub fn validate_image_bytes(image_bytes: &[u8]) -> Result<(), ImageValidationError> {
    if image_bytes.is_empty() {
        return Err(ImageValidationError("Image payload is empty".to_string()));
    }
    // WebP: RIFF....WEBP — check the WEBP tag at offset 8.
    if image_bytes.starts_with(MAGIC_RIFF)
        && image_bytes.len() >= 12
        && &image_bytes[8..12] == b"WEBP"
    {
        return Ok(());
    }
    for magic in [MAGIC_JPEG, MAGIC_PNG, MAGIC_GIF87A, MAGIC_GIF89A, MAGIC_BMP] {
        if image_bytes.starts_with(magic) {
            return Ok(());
        }
    }
    Err(ImageValidationError(
        "Image bytes do not match a supported format (JPEG, PNG, GIF, BMP, WebP)".to_string(),
    ))
}

/// Reject downloads larger than the image size limit instead of silently
/// forwarding a truncated prefix to the model.
pub fn ensure_within_limit(len: usize) -> Result<(), ImageValidationError> {
    if len > MAX_IMAGE_BYTES {
        return Err(ImageValidationError(
            "Image downloaded from URL exceeds 20 MiB limit".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes() -> Vec<u8> {
        b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\nIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\r\n-\xb4\x00\x00\x00\x00IEND\xaeB`\x82".to_vec()
    }

    #[test]
    fn rejects_non_image_bytes() {
        assert!(validate_image_bytes(b"ABC").is_err());
        assert!(validate_image_bytes(&png_bytes()).is_ok());
    }

    #[test]
    fn rejects_webp_with_riff_only() {
        // RIFF without WEBP tag at offset 8 must be rejected.
        let riff_not_webp = b"RIFF\x00\x00\x00\x00JUNK".to_vec();
        assert!(validate_image_bytes(&riff_not_webp).is_err());
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[0, 0, 0, 0]);
        webp.extend_from_slice(b"WEBP");
        assert!(validate_image_bytes(&webp).is_ok());
    }

    #[test]
    fn over_limit_size_rejected() {
        assert!(ensure_within_limit(20 * 1024 * 1024).is_ok());
        let err = ensure_within_limit(20 * 1024 * 1024 + 1).unwrap_err();
        assert!(err.0.contains("exceeds 20 MiB"));
    }
}
