//! The clock, and times shown as how long ago they were.
//!
//! Times shown are user data, like when a recent file was opened or a
//! recovered design last written, and may be anything, so all arithmetic
//! on them is bounded or checked.

use varde_io::UnixSeconds;

/// The current time, saturating at the ends of `i64`. From iced's clock,
/// since `std`'s panics in browsers.
pub(crate) fn now() -> UnixSeconds {
    use iced::time::SystemTime;

    UnixSeconds(
        match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
            Err(before) => i64::try_from(before.duration().as_secs()).map_or(i64::MIN, |s| -s),
        },
    )
}

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const WEEK: i64 = 7 * DAY;
/// The end of "Yesterday": 48 hours.
const DAY_BEFORE: i64 = 2 * DAY;

/// How long ago `time` was at `now`: "Just now", "5 min ago", "2 h ago",
/// "Yesterday", "3 days ago", then a date. A time in the future also shows
/// as a date.
///
/// "Yesterday" means 24 to 48 hours ago, and dates are in UTC, since there
/// is no time zone database.
pub(crate) fn ago(time: UnixSeconds, now: UnixSeconds) -> String {
    said(time, now, ["Just now", "Yesterday", "Unknown date"])
}

/// [`ago`] to go in a sentence, after "from": "just now", "5 min ago",
/// "yesterday", "Sep 12", "an unknown date".
pub(crate) fn ago_in_sentence(time: UnixSeconds, now: UnixSeconds) -> String {
    said(time, now, ["just now", "yesterday", "an unknown date"])
}

/// [`ago`], saying "just now", "yesterday" and an unknown date as `words`
/// have them.
fn said(time: UnixSeconds, now: UnixSeconds, words: [&str; 3]) -> String {
    let [just_now, yesterday, unknown] = words;
    match now.checked_since(time) {
        Some(0..MINUTE) => just_now.to_owned(),
        Some(elapsed @ MINUTE..HOUR) => format!("{} min ago", elapsed / MINUTE),
        Some(elapsed @ HOUR..DAY) => format!("{} h ago", elapsed / HOUR),
        Some(DAY..DAY_BEFORE) => yesterday.to_owned(),
        Some(elapsed @ DAY_BEFORE..WEEK) => format!("{} days ago", elapsed / DAY),
        _ => date(time, now).unwrap_or_else(|| unknown.to_owned()),
    }
}

/// `time` as a date like "Sep 12", with the year added unless it is the
/// year of `now`, if it's within the years 1 to 9999.
fn date(time: UnixSeconds, now: UnixSeconds) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (year, month, day) = civil(time)?;
    let month = MONTHS[month as usize - 1];
    Some(
        if civil(now).is_some_and(|(now_year, _, _)| now_year == year) {
            format!("{month} {day}")
        } else {
            format!("{month} {day}, {year}")
        },
    )
}

/// The UTC (year, month, day) of `time`, for the years 1 to 9999.
fn civil(UnixSeconds(time): UnixSeconds) -> Option<(i64, u32, u32)> {
    // 0001-01-01T00:00:00Z and 9999-12-31T23:59:59Z. The bounds keep the
    // arithmetic below far from overflowing.
    if !(-62_135_596_800..=253_402_300_799).contains(&time) {
        return None;
    }
    // Howard Hinnant's `civil_from_days`, with eras of 400 years starting on
    // March 1st so the leap day falls at the end.
    let z = time.div_euclid(DAY) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some((year, month as u32, day as u32))
}

#[cfg(test)]
mod tests;
