//! 診断日時などに使う UTC の ISO 8601 文字列（時刻クレートを増やさないための最小実装）。

use std::time::{SystemTime, UNIX_EPOCH};

/// 現在時刻（例: `2026-10-03T10:00:00Z`）
pub fn now_iso8601() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format_unix(secs)
}

pub fn format_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// 1970-01-01 からの日数を (年, 月, 日) にする（Howard Hinnant の civil_from_days）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix(951_782_400), "2000-02-29T00:00:00Z", "うるう日");
        assert_eq!(format_unix(1_782_986_400), "2026-07-02T10:00:00Z");
        assert_eq!(format_unix(4_102_444_799), "2099-12-31T23:59:59Z");
    }

    #[test]
    fn now_has_iso_shape_and_sorts_as_text() {
        let n = now_iso8601();
        assert_eq!(n.len(), 20);
        assert!(n.ends_with('Z') && n.as_bytes()[10] == b'T');
        assert!(format_unix(1_000_000_000) < format_unix(1_800_000_000), "文字列順が時刻順");
    }
}
