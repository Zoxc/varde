use super::*;

// 2026-09-26T12:00:00Z.
const NOW: i64 = 1_790_424_000;

/// How long ago `time` was at `now`, both in seconds.
fn relative_time(time: i64, now: i64) -> String {
    ago(UnixSeconds(time), UnixSeconds(now))
}

#[test]
fn relative_times() {
    assert_eq!(relative_time(NOW, NOW), "Just now");
    assert_eq!(relative_time(NOW - 59, NOW), "Just now");
    assert_eq!(relative_time(NOW - 5 * MINUTE, NOW), "5 min ago");
    assert_eq!(relative_time(NOW - 2 * HOUR - 1, NOW), "2 h ago");
    assert_eq!(relative_time(NOW - DAY, NOW), "Yesterday");
    assert_eq!(relative_time(NOW - 3 * DAY, NOW), "3 days ago");
    assert_eq!(relative_time(NOW - 14 * DAY, NOW), "Sep 12");
    assert_eq!(relative_time(NOW - 365 * DAY, NOW), "Sep 26, 2025");
}

#[test]
fn future_times_show_as_dates() {
    assert_eq!(relative_time(NOW + 1, NOW), "Sep 26");
    assert_eq!(relative_time(NOW + 400 * DAY, NOW), "Oct 31, 2027");
}

#[test]
fn edge_times() {
    assert_eq!(relative_time(0, NOW), "Jan 1, 1970");
    assert_eq!(relative_time(-1, NOW), "Dec 31, 1969");
    assert_eq!(relative_time(951_782_400, NOW), "Feb 29, 2000");
    assert_eq!(relative_time(253_402_300_799, NOW), "Dec 31, 9999");
    assert_eq!(relative_time(-62_135_596_800, NOW), "Jan 1, 1");
    for garbage in [i64::MIN, i64::MAX, 253_402_300_800, -62_135_596_801] {
        assert_eq!(relative_time(garbage, NOW), "Unknown date");
    }
    // A garbage clock never panics either.
    assert_eq!(relative_time(NOW, i64::MIN), "Sep 26, 2026");
    assert_eq!(relative_time(NOW, i64::MAX), "Sep 26, 2026");
    assert_eq!(relative_time(i64::MAX, i64::MIN), "Unknown date");
}
