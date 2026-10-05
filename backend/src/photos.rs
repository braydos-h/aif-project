//! Optional private photo retention (roadmap item 8).
//!
//! Disabled by default (`AIF_RETAIN_PHOTOS=0`): uploads are transient and
//! never touch disk. When the operator enables retention, an authenticated
//! estimate with `retain_photo: true` stores the processed photo under an
//! opaque server-generated id in `<data_dir>/photos/` with a matching
//! `photos` row (owner, optional estimate link, expiry, size, MIME).
//!
//! Guarantees: ids are 32-hex server secrets (never built from user URL
//! input — download ids are allow-list validated and ownership-checked);
//! bytes are magic-byte validated and size-bounded before writing; JPEG
//! APP1/Exif and PNG eXIf metadata segments are stripped so retained
//! photos carry no location metadata; per-user quotas and expiry bound
//! growth; history/account deletion cascades (or sweeps) remove rows and
//! files together; orphans in either direction are cleaned on sweep.
//! Retained photos are excluded from database backups by design (see
//! `docs/privacy.md`); back up the photo dir separately if you need it.

use std::path::{Path, PathBuf};

use crate::db::{Db, Photo};
use crate::time_util::{rfc3339, unix_now};
use crate::validate::{validate_image_bytes, MAX_IMAGE_BYTES};

/// Opaque photo ids are always 32 lowercase hex chars.
pub fn valid_photo_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Photo directory inside the data dir (created on demand).
pub fn photo_dir(data_dir: &str) -> PathBuf {
    Path::new(data_dir).join("photos")
}

/// MIME type for validated image bytes (magic bytes, not extensions).
fn mime_for(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"RIFF") && bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
}

/// Strip the JPEG APP1/Exif segment (location metadata lives here).
/// Unknown/malformed input passes through byte-identical; callers validate
/// magic bytes separately.
pub fn strip_jpeg_exif(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return bytes.to_vec();
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..2]);
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            break;
        }
        let marker = bytes[i + 1];
        // SOS / EOI: copy the rest verbatim (entropy-coded data follows).
        if marker == 0xDA || marker == 0xD9 {
            out.extend_from_slice(&bytes[i..]);
            return out;
        }
        // Standalone markers without a length field.
        if marker == 0x00 || marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            out.extend_from_slice(&bytes[i..i + 2]);
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if len < 2 || i + 2 + len > bytes.len() {
            break;
        }
        let is_exif_app1 = marker == 0xE1 && len >= 8 && &bytes[i + 4..i + 10] == b"Exif\0\0";
        if !is_exif_app1 {
            out.extend_from_slice(&bytes[i..i + 2 + len]);
        }
        i += 2 + len;
    }
    out.extend_from_slice(&bytes[i.min(bytes.len())..]);
    out
}

/// Strip PNG `eXIf` chunks (location metadata). Other chunks pass through
/// with their CRCs intact (dropping a chunk never invalidates the rest).
pub fn strip_png_exif(bytes: &[u8]) -> Vec<u8> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 8 || &bytes[..8] != SIG {
        return bytes.to_vec();
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(SIG);
    let mut i = 8;
    while i + 12 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        let end = i + 12 + len;
        if end > bytes.len() {
            break;
        }
        let kind = &bytes[i + 4..i + 8];
        if kind != b"eXIf" {
            out.extend_from_slice(&bytes[i..end]);
        }
        i = end;
        if kind == b"IEND" {
            break;
        }
    }
    out.extend_from_slice(&bytes[i.min(bytes.len())..]);
    out
}

/// Strip location metadata according to the validated image kind.
pub fn strip_metadata(bytes: &[u8]) -> Vec<u8> {
    if bytes.starts_with(b"\xff\xd8\xff") {
        strip_jpeg_exif(bytes)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        strip_png_exif(bytes)
    } else {
        bytes.to_vec()
    }
}

/// Validate, strip metadata, and store one retained photo. Returns the
/// photo id. Enforces the per-user quota before writing.
pub fn store_photo(
    db: &Db,
    data_dir: &str,
    user_id: &str,
    estimate_id: Option<&str>,
    image_bytes: &[u8],
    ttl_days: u64,
    quota_bytes: u64,
) -> Result<String, String> {
    if image_bytes.len() > MAX_IMAGE_BYTES {
        return Err("photo exceeds the 20 MiB image limit".to_string());
    }
    validate_image_bytes(image_bytes).map_err(|e| e.to_string())?;
    let processed = strip_metadata(image_bytes);
    let used = db.photo_bytes_used(user_id)?;
    if used as u64 + processed.len() as u64 > quota_bytes {
        return Err("photo quota exceeded".to_string());
    }
    let dir = photo_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create photo dir: {}", e))?;
    let id = crate::auth::new_id();
    debug_assert!(valid_photo_id(&id));
    let tmp = dir.join(format!("{}.tmp-{}", id, std::process::id()));
    std::fs::write(&tmp, &processed).map_err(|e| format!("cannot write photo: {}", e))?;
    // Restrictive permissions: owner read/write only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    let dest = dir.join(&id);
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("cannot store photo: {}", e));
    }
    let now = unix_now();
    let photo = Photo {
        id: id.clone(),
        user_id: user_id.to_string(),
        estimate_id: estimate_id.map(str::to_string),
        created_at: rfc3339(now),
        expires_at: rfc3339(now + ttl_days.saturating_mul(86_400)),
        bytes: processed.len() as i64,
        mime: mime_for(&processed).to_string(),
    };
    if let Err(e) = db.insert_photo(&photo) {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
    Ok(id)
}

/// Read one retained photo file (ownership checked by the caller via the
/// `photos` row). Returns `None` when the file is missing (orphan row).
pub fn read_photo(data_dir: &str, id: &str) -> Option<Vec<u8>> {
    if !valid_photo_id(id) {
        return None;
    }
    std::fs::read(photo_dir(data_dir).join(id)).ok()
}

/// Delete one photo row and its file. Missing files are tolerated.
pub fn delete_photo(db: &Db, data_dir: &str, id: &str) -> Result<bool, String> {
    if !valid_photo_id(id) {
        return Ok(false);
    }
    let existed = db.delete_photo(id)?;
    let _ = std::fs::remove_file(photo_dir(data_dir).join(id));
    Ok(existed)
}

/// Sweep: delete expired rows + files, drop orphan files without rows, and
/// drop rows whose files are missing. Returns (expired, orphans, missing).
pub fn sweep_photos(db: &Db, data_dir: &str, now: &str) -> Result<(usize, usize, usize), String> {
    let dir = photo_dir(data_dir);
    let expired = db.delete_expired_photos(now)?;
    for id in &expired {
        let _ = std::fs::remove_file(dir.join(id));
    }
    let mut orphans = 0usize;
    let mut missing = 0usize;
    let entries = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for name in entries {
        if !valid_photo_id(&name) {
            continue;
        }
        if let Ok(None) = db.photo_by_id(&name) {
            let _ = std::fs::remove_file(dir.join(&name));
            orphans += 1;
        }
    }
    // Rows whose files vanished are pruned so listings never 404.
    if let Ok(all) = db.all_photo_ids() {
        for id in all {
            if !dir.join(&id).is_file() {
                let _ = db.delete_photo(&id);
                missing += 1;
            }
        }
    }
    Ok((expired.len(), orphans, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg_with_exif() -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        // APP1 Exif segment (to be stripped).
        v.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x10]);
        v.extend_from_slice(b"Exif\0\0GPS-DATA-HERE!");
        // APP0 segment (kept).
        v.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x08]);
        v.extend_from_slice(b"JFIF\0A");
        // SOS + payload (kept verbatim).
        v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x02, 0x03, 0x04, 0xAA, 0xBB]);
        v
    }

    #[test]
    fn jpeg_exif_is_stripped_but_image_survives() {
        let stripped = strip_jpeg_exif(&jpeg_with_exif());
        assert!(!stripped.windows(4).any(|w| w == b"Exif"));
        assert!(stripped.windows(4).any(|w| w == b"JFIF"));
        assert!(stripped.ends_with(&[0xAA, 0xBB]));
        assert!(stripped.starts_with(&[0xFF, 0xD8]));
    }

    fn png_with_exif() -> Vec<u8> {
        fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut v = Vec::new();
            v.extend_from_slice(&(data.len() as u32).to_be_bytes());
            v.extend_from_slice(kind);
            v.extend_from_slice(data);
            v.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // fake CRC
            v
        }
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend_from_slice(&chunk(b"IHDR", &[0; 8]));
        v.extend_from_slice(&chunk(b"eXIf", b"GPS-DATA"));
        v.extend_from_slice(&chunk(b"IDAT", &[1, 2, 3]));
        v.extend_from_slice(&chunk(b"IEND", &[]));
        v
    }

    #[test]
    fn png_exif_chunk_is_dropped() {
        let stripped = strip_png_exif(&png_with_exif());
        assert!(!stripped.windows(4).any(|w| w == b"eXIf"));
        assert!(stripped.windows(4).any(|w| w == b"IDAT"));
        assert!(stripped.ends_with(b"IEND\xDE\xAD\xBE\xEF"));
    }

    #[test]
    fn photo_ids_are_strict() {
        assert!(valid_photo_id(&"a".repeat(32)));
        assert!(!valid_photo_id("../../etc/passwdxxxxxxxxxxxx"));
        assert!(!valid_photo_id("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"));
        assert!(!valid_photo_id("abc"));
    }

    #[test]
    fn store_and_sweep_round_trip() {
        let db = Db::open_temp("photos").unwrap();
        db.create_user("u1", "a@example.com", "A", "h", "user")
            .unwrap();
        let dir = std::env::temp_dir().join(format!("aif-photos-{}", std::process::id()));
        let dir_str = dir.to_str().unwrap();
        // Minimal valid PNG (signature + IHDR + IEND).
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0; 13]);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&[0xAE, 0x42, 0x60, 0x82]);
        // validate_image_bytes may require more; fall back to BMP (simplest
        // header: "BM" + size). BMP has no metadata segments by construction.
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[100u8, 0, 0, 0]);
        bmp.extend_from_slice(&[0u8; 100]);
        let id = store_photo(&db, dir_str, "u1", None, &bmp, 30, 1024 * 1024).unwrap();
        assert!(valid_photo_id(&id));
        assert!(read_photo(dir_str, &id).is_some());
        assert!(read_photo(dir_str, "../nopexxxxxxxxxxxxxxxxxxxxxxxx").is_none());
        // Quota enforcement.
        assert!(store_photo(&db, dir_str, "u1", None, &bmp, 30, 1).is_err());
        // Expiry sweep removes rows and files.
        let (expired, orphans, _) = sweep_photos(&db, dir_str, "2999-01-01T00:00:00Z").unwrap();
        assert_eq!(expired, 1);
        assert_eq!(orphans, 0);
        assert!(read_photo(dir_str, &id).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
