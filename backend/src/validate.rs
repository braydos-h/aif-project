//! Image reference handling: URL fetch, base64 decode, magic-byte validation.
//!
//! Mirrors `CowWeightEstimator._to_base64_image` / `_validate_image_bytes`
//! in `aif/estimator.py`.

use std::io::Read;

/// Error raised when the supplied image bytes are not a recognised format.
#[derive(Debug)]
pub struct ImageValidationError(pub String);

impl std::fmt::Display for ImageValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ImageValidationError {}

const MAGIC_JPEG: &[u8] = &[0xFF, 0xD8, 0xFF];
const MAGIC_PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const MAGIC_GIF87A: &[u8] = b"GIF87a";
const MAGIC_GIF89A: &[u8] = b"GIF89a";
const MAGIC_BMP: &[u8] = b"BM";
const MAGIC_RIFF: &[u8] = b"RIFF";

/// Raise an error unless the bytes look like a supported image format.
pub fn validate_image_bytes(image_bytes: &[u8]) -> Result<(), ImageValidationError> {
    if image_bytes.is_empty() {
        return Err(ImageValidationError("Image payload is empty".to_string()));
    }
    // WebP: RIFF....WEBP — check the WEBP tag at offset 8.
    if image_bytes.starts_with(MAGIC_RIFF) && image_bytes.len() >= 12 {
        if &image_bytes[8..12] == b"WEBP" {
            return Ok(());
        }
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

/// Decode strict base64. Returns an error message on invalid input.
/// Mirrors `base64.b64decode(input, validate=True)`.
fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Err("Image base64 payload is not valid base64".to_string());
    }
    // Strict alphabet: no whitespace, no chars outside A-Za-z0-9+/=.
    let mut alphabet_pos = None;
    for (i, b) in bytes.iter().enumerate() {
        let ok = b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/' || *b == b'=';
        if !ok {
            return Err("Image base64 payload is not valid base64".to_string());
        }
        if *b == b'=' && alphabet_pos.is_none() {
            alphabet_pos = Some(i);
        }
    }
    // Padding may only appear at the end, at most 2 chars.
    if let Some(start) = alphabet_pos {
        let padding = bytes.len() - start;
        if padding > 2 || !bytes[start..].iter().all(|b| *b == b'=') {
            return Err("Image base64 payload is not valid base64".to_string());
        }
    }
    if bytes.len() % 4 != 0 {
        return Err("Image base64 payload is not valid base64".to_string());
    }
    base64_decode_impl(bytes).ok_or_else(|| {
        "Image base64 payload is not valid base64".to_string()
    })
}

fn b64_val(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode canonical base64 (already validated) into bytes.
fn base64_decode_impl(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut chunk = [0u8; 4];
    let mut n = 0;
    for &b in input {
        if b == b'=' {
            break;
        }
        chunk[n] = b;
        n += 1;
        if n == 4 {
            let vals = [b64_val(chunk[0])?, b64_val(chunk[1])?, b64_val(chunk[2])?, b64_val(chunk[3])?];
            let v = ((vals[0] as u32) << 18)
                | ((vals[1] as u32) << 12)
                | ((vals[2] as u32) << 6)
                | (vals[3] as u32);
            out.push((v >> 16) as u8);
            out.push((v >> 8) as u8);
            out.push(v as u8);
            n = 0;
        }
    }
    if n == 2 {
        let vals = [b64_val(chunk[0])?, b64_val(chunk[1])?];
        let v = ((vals[0] as u16) << 6) | (vals[1] as u16);
        out.push((v >> 4) as u8);
    } else if n == 3 {
        let vals = [b64_val(chunk[0])?, b64_val(chunk[1])?, b64_val(chunk[2])?];
        let v = ((vals[0] as u16) << 12) | ((vals[1] as u16) << 6) | (vals[2] as u16);
        out.push((v >> 10) as u8);
        out.push((v >> 2) as u8);
    } else if n == 1 {
        return None;
    }
    Some(out)
}

/// Fetch bytes from an http(s) URL with a 30s timeout and a UA header.
fn fetch_url(url: &str) -> Result<Vec<u8>, String> {
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
        .take(20 * 1024 * 1024)
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read image from {}: {}", url, e))?;
    Ok(buf)
}

/// Return raw base64 image bytes from a URL, a data: URI, or a base64 string.
///
/// Validates that the decoded bytes look like a supported image format.
/// Mirrors `CowWeightEstimator._to_base64_image`.
pub fn to_base64_image(image_reference: &str) -> Result<String, ImageValidationError> {
    if image_reference.starts_with("http://") || image_reference.starts_with("https://") {
        let image_bytes = fetch_url(image_reference)
            .map_err(|e| ImageValidationError(e))?;
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

    let decoded = decode_base64(stripped)
        .map_err(|msg| ImageValidationError(msg))?;
    validate_image_bytes(&decoded)?;
    Ok(stripped.to_string())
}

/// Standard base64 encode.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut chunks = bytes.chunks_exact(3);
    for chunk in &mut chunks {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        out.push(ALPHABET[n as usize & 63] as char);
    }
    let rem = chunks.remainder();
    match rem.len() {
        1 => {
            let n = (rem[0] as u32) << 16;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let n = ((rem[0] as u32) << 16) | ((rem[1] as u32) << 8);
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes() -> Vec<u8> {
        b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\nIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\r\n-\xb4\x00\x00\x00\x00IEND\xaeB`\x82".to_vec()
    }

    #[test]
    fn base64_round_trip() {
        let bytes = png_bytes();
        let encoded = base64_encode(&bytes);
        assert!(encoded.starts_with("iVBOR"));
        assert_eq!(decode_base64(&encoded).unwrap(), bytes);
    }

    #[test]
    fn rejects_non_image_bytes() {
        assert!(matches!(
            validate_image_bytes(b"ABC"),
            Err(_)
        ));
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
    fn data_uri_stripped() {
        let b64 = base64_encode(&png_bytes());
        let uri = format!("data:image/png;base64,{}", b64);
        let out = to_base64_image(&uri).unwrap();
        assert_eq!(out, b64);
    }

    #[test]
    fn invalid_base64_rejected() {
        let err = to_base64_image("QUJD").unwrap_err();
        assert!(err.0.contains("not valid base64"));
        let err2 = to_base64_image("!!!").unwrap_err();
        assert!(err2.0.contains("not valid base64"));
    }
}
