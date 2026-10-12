//! Wall-clock helpers: unix timestamps and RFC 3339 UTC formatting.
//!
//! The database stores unambiguous UTC timestamps (`2026-10-05T12:34:56Z`);
//! the WebUI renders them in the viewer's local timezone.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the unix epoch (saturating at 0 before the epoch).
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Current UTC day as `YYYY-MM-DD` (for per-day usage counters).
pub fn utc_day() -> String {
    rfc3339(unix_now())[..10].to_string()
}

/// Format a unix timestamp as RFC 3339 UTC (`2026-10-05T12:34:56Z`).
pub fn rfc3339(unix_secs: u64) -> String {
    let (y, mo, d, _, _, _) = civil_from_days((unix_secs / 86_400) as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        mo,
        d,
        (unix_secs / 3600) % 24,
        (unix_secs / 60) % 60,
        unix_secs % 60,
    )
}

/// Days since unix epoch -> (year, month, day, hour, min, sec-of-day unused).
/// Howard Hinnant's civil-from-days algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32, u64, u64, u64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d, 0, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_timestamps_format_correctly() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(utc_day().len(), 10);
        assert!(unix_now() > 1_700_000_000);
    }
}
