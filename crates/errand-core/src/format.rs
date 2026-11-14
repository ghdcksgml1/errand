//! How times read in Errand, in the time zone of the computer's own clock and the person's language.

use crate::i18n::Language;
use jiff::{SignedDuration, Timestamp, tz::TimeZone};

/// The zone the person's clock shows.
pub fn local_zone() -> TimeZone {
    TimeZone::system()
}

/// Unix seconds as an instant; None when not finite or out of range.
pub fn timestamp_from_seconds(seconds: f64) -> Option<Timestamp> {
    let duration = SignedDuration::try_from_secs_f64(seconds).ok()?;
    Timestamp::from_duration(duration).ok()
}

pub fn seconds(at: Timestamp) -> f64 {
    at.as_duration().as_secs_f64()
}

/// "10/19 09:00".
pub fn month_day_time(at: Timestamp, zone: &TimeZone) -> String {
    at.to_zoned(zone.clone()).strftime("%-m/%-d %H:%M").to_string()
}

/// "방금", "7분 전", "3시간 전", then the date once it is half a day old.
pub fn relative_time(at: Option<Timestamp>, now: Timestamp, zone: &TimeZone, language: Language) -> String {
    let Some(at) = at else { return String::new() };
    let age = (seconds(now) - seconds(at)).max(0.0);
    if age < 60.0 {
        language.just_now().to_owned()
    } else if age < 3600.0 {
        language.minutes_ago((age / 60.0).floor() as i64)
    } else if age < 12.0 * 3600.0 {
        language.hours_ago((age / 3600.0).floor() as i64)
    } else {
        month_day_time(at, zone)
    }
}

/// How long running work has been going: "방금 시작", "12분째", "1시간 5분째".
pub fn elapsed(start: Timestamp, now: Timestamp, language: Language) -> String {
    language.elapsed(((seconds(now) - seconds(start)).max(0.0) / 60.0).floor() as i64)
}

/// A span as a stopwatch shows it, in whole seconds and never below zero, so a clock a little ahead of the
/// server's reads as just now: "0:42", "12:05", "1:02:03".
pub fn stopwatch(span: SignedDuration) -> String {
    let (hours, minutes, seconds) = clock_parts(span);
    if hours > 0 { format!("{hours}:{minutes:02}:{seconds:02}") } else { format!("{minutes}:{seconds:02}") }
}

/// Whole hours, minutes and seconds in a span, none below zero.
pub fn clock_parts(span: SignedDuration) -> (i64, i64, i64) {
    let seconds = span.as_secs().max(0);
    (seconds / 3600, seconds / 60 % 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::tz::offset;

    fn seoul() -> TimeZone {
        TimeZone::fixed(offset(9))
    }
    fn at(seconds: f64) -> Timestamp {
        timestamp_from_seconds(seconds).unwrap()
    }

    #[test]
    fn dates_drop_leading_zeros_from_month_and_day_only() {
        // 2026-10-05T00:07:00Z is 09:07 in Seoul.
        assert_eq!(month_day_time(at(1_791_158_820.0), &seoul()), "10/5 09:07");
    }

    #[test]
    fn relative_times_step_from_minutes_to_hours_to_a_date() {
        let now = at(1_792_044_000.0);
        let ago = |seconds: f64| Some(at(1_792_044_000.0 - seconds));
        let korean = |seconds: f64| relative_time(ago(seconds), now, &seoul(), Language::Korean);
        let english = |seconds: f64| relative_time(ago(seconds), now, &seoul(), Language::English);
        assert_eq!(relative_time(None, now, &seoul(), Language::Korean), "");
        assert_eq!((korean(59.0), english(59.0)), ("방금".to_owned(), "just now".to_owned()));
        assert_eq!(korean(-30.0), "방금");
        assert_eq!((korean(7.0 * 60.0 + 59.0), english(7.0 * 60.0 + 59.0)), ("7분 전".to_owned(), "7 min ago".to_owned()));
        assert_eq!((korean(3.0 * 3600.0), english(3.0 * 3600.0)), ("3시간 전".to_owned(), "3 hr ago".to_owned()));
        assert_eq!(english(12.0 * 3600.0), month_day_time(at(1_792_044_000.0 - 43_200.0), &seoul()));
    }

    #[test]
    fn time_in_a_room_reads_as_a_stopwatch() {
        let span = |seconds: f64| SignedDuration::from_secs_f64(seconds);
        let read: Vec<String> = [0.0, 41.9, 42.0, 725.0, 3599.0, 3723.0, 36_000.0, 360_000.0].map(|seconds| stopwatch(span(seconds))).to_vec();
        assert_eq!(read, ["0:00", "0:41", "0:42", "12:05", "59:59", "1:02:03", "10:00:00", "100:00:00"], "whole seconds, hours only once there is one");
        assert_eq!(stopwatch(span(-3.5)), "0:00", "a clock a little ahead of the server's reads as just now");
    }

    #[test]
    fn elapsed_time_reads_as_ongoing_work() {
        let start = at(1_792_044_000.0);
        let after = |seconds: f64| at(1_792_044_000.0 + seconds);
        assert_eq!(elapsed(start, after(59.0), Language::Korean), "방금 시작");
        assert_eq!(elapsed(start, after(12.0 * 60.0), Language::Korean), "12분째");
        assert_eq!(elapsed(start, after(65.0 * 60.0), Language::Korean), "1시간 5분째");
        assert_eq!(elapsed(start, after(-10.0), Language::Korean), "방금 시작");
        assert_eq!(elapsed(start, after(59.0), Language::English), "just started");
        assert_eq!(elapsed(start, after(12.0 * 60.0), Language::English), "12 min");
        assert_eq!(elapsed(start, after(65.0 * 60.0), Language::English), "1 hr 5 min");
    }
}
