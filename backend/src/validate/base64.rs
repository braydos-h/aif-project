//! Strict base64 codec without external dependencies.
//!
//! Mirrors `base64.b64decode(input, validate=True)` for decoding and the
//! standard alphabet for encoding.

/// Decode strict base64. Returns an error message on invalid input.
/// Mirrors `base64.b64decode(input, validate=True)`.
pub(crate) fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
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
    if !bytes.len().is_multiple_of(4) {
        return Err("Image base64 payload is not valid base64".to_string());
    }
    base64_decode_impl(bytes).ok_or_else(|| "Image base64 payload is not valid base64".to_string())
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
            let vals = [
                b64_val(chunk[0])?,
                b64_val(chunk[1])?,
                b64_val(chunk[2])?,
                b64_val(chunk[3])?,
            ];
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

    #[test]
    fn base64_round_trip() {
        let bytes = b"\x89PNG\r\n\x1a\ntest-bytes".to_vec();
        let encoded = base64_encode(&bytes);
        assert_eq!(decode_base64(&encoded).unwrap(), bytes);
    }

    #[test]
    fn rejects_bad_alphabet_and_padding() {
        assert!(decode_base64("!!!").is_err());
        assert!(decode_base64("").is_err());
        assert!(decode_base64("ABC").is_err());
    }
}
