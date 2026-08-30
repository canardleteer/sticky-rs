//! UTC filename stamps without a datetime crate.

/// Seconds since Unix epoch as `YYYYMMDDThhmmssZ`.
#[must_use]
pub fn utc_compact_stamp(unix_secs: u64) -> String {
    let (year, month, day, hour, min, sec) = unix_to_utc(unix_secs);
    format!("{year:04}{month:02}{day:02}T{hour:02}{min:02}{sec:02}Z")
}

/// RFC3339 UTC with milliseconds, for live logs and step banners.
#[must_use]
pub fn utc_rfc3339_millis() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let (year, month, day, hour, min, sec) = unix_to_utc(duration.as_secs());
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{:03}Z",
        duration.subsec_millis()
    )
}

fn unix_to_utc(unix_secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let day_secs = (unix_secs % 86_400) as u32;
    let hour = day_secs / 3_600;
    let min = (day_secs % 3_600) / 60;
    let sec = day_secs % 60;
    let (year, month, day) = civil_from_days((unix_secs / 86_400) as i64);
    (year, month, day, hour, min, sec)
}

/// Howard Hinnant `civil_from_days` (days since Unix epoch → y-m-d).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_is_1970() {
        assert_eq!(utc_compact_stamp(0), "19700101T000000Z");
    }

    #[test]
    fn one_billion_is_2001_09_09() {
        assert_eq!(utc_compact_stamp(1_000_000_000), "20010909T014640Z");
    }

    #[test]
    fn rfc3339_millis_has_date_and_fraction() {
        let s = utc_rfc3339_millis();
        assert!(s.contains('T'));
        assert!(s.ends_with('Z'));
        assert!(s.contains('.'));
    }
}
