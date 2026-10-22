//! Small formatting helpers for the desktop statistics presentation.
//! These functions do not query the library or alter playback accounting.

use super::i18n::{Language, translate};
use chrono::{DateTime, Duration, NaiveDateTime, Offset, TimeZone};

/// Format play counts consistently without converting them through a float.
pub fn grouped_number(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + (digits.len() - 1) / 3);
    for (index, digit) in digits.bytes().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(char::from(digit));
    }
    grouped
}

/// A finite, bounded height for native chart bars, including an empty chart.
pub fn bar_ratio(count: u64, maximum: u64) -> f32 {
    if maximum == 0 {
        return 0.0;
    }
    (count.min(maximum) as f64 / maximum as f64) as f32
}

/// Round the chart's upper bound to 1, 2, or 5 times a power of ten.
/// An empty chart keeps a nonzero scale; extreme counters never overflow.
pub fn chart_ceiling(maximum: u64) -> u64 {
    if maximum == 0 {
        return 1;
    }
    let magnitude = 10_u64.pow(maximum.ilog10());
    for multiplier in [1, 2, 5, 10] {
        if let Some(ceiling) = magnitude.checked_mul(multiplier)
            && ceiling >= maximum
        {
            return ceiling;
        }
    }
    u64::MAX
}

/// Describe a retained play in the same local timezone used by the chart.
/// Calendar dates take precedence over short elapsed durations across midnight.
/// Future clock readings use an absolute date rather than a negative age.
pub fn relative_played_at<Tz: TimeZone>(
    played_at: i64,
    now: DateTime<Tz>,
    language: Language,
) -> String {
    let unavailable = || translate(language, "Unavailable");
    let Some(played) = now.timezone().timestamp_opt(played_at, 0).single() else {
        return unavailable();
    };
    let (Some(played_local), Some(now_local)) = (local_datetime(&played), local_datetime(&now))
    else {
        return unavailable();
    };
    let absolute = || played_local.format("%m/%d %H:%M").to_string();
    if played_at > now.timestamp() {
        return absolute();
    }
    if played_local.date() != now_local.date() {
        if now_local.date().pred_opt() == Some(played_local.date()) {
            return translate(language, "Yesterday {0}")
                .replace("{0}", &played_local.format("%H:%M").to_string());
        }
        return absolute();
    }
    let seconds = now.signed_duration_since(played).num_seconds();
    if seconds < 60 {
        translate(language, "Just now")
    } else if seconds < 3600 {
        translate(language, "{0}m ago").replace("{0}", &(seconds / 60).to_string())
    } else {
        translate(language, "{0}h ago").replace("{0}", &(seconds / 3600).to_string())
    }
}

// Chrono allows a valid UTC timestamp whose timezone offset moves its local
// representation outside NaiveDateTime's limits. Use checked arithmetic so a
// corrupt/extreme history timestamp cannot panic while projecting a UI row.
fn local_datetime<Tz: TimeZone>(date: &DateTime<Tz>) -> Option<NaiveDateTime> {
    date.naive_utc()
        .checked_add_signed(Duration::seconds(i64::from(
            date.offset().fix().local_minus_utc(),
        )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};

    fn now() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 22, 12, 0, 0)
            .unwrap()
    }

    fn relative(timestamp: i64) -> String {
        relative_played_at(timestamp, now(), Language::English)
    }

    #[test]
    fn numbers_group_at_thousands_without_losing_large_integer_precision() {
        for (value, expected) in [
            (0, "0"),
            (9, "9"),
            (999, "999"),
            (1000, "1,000"),
            (12_345, "12,345"),
            (100_000, "100,000"),
            (1_000_001, "1,000,001"),
            (u64::MAX, "18,446,744,073,709,551,615"),
        ] {
            assert_eq!(grouped_number(value), expected);
        }
    }

    #[test]
    fn bar_ratios_are_finite_and_clamped_for_zero_and_extreme_counts() {
        for (count, maximum, expected) in [
            (0, 0, 0.0),
            (10, 0, 0.0),
            (0, 10, 0.0),
            (2, 8, 0.25),
            (5, 10, 0.5),
            (10, 10, 1.0),
            (20, 10, 1.0),
            (u64::MAX, 1, 1.0),
            (u64::MAX, u64::MAX, 1.0),
        ] {
            let ratio = bar_ratio(count, maximum);
            assert!(ratio.is_finite());
            assert!((0.0..=1.0).contains(&ratio));
            assert_eq!(ratio, expected);
        }
        assert!(bar_ratio(1, u64::MAX) > 0.0);
    }

    #[test]
    fn chart_scale_rounds_up_at_readable_boundaries_and_never_overflows() {
        for (maximum, expected) in [
            (0, 1),
            (1, 1),
            (2, 2),
            (3, 5),
            (5, 5),
            (6, 10),
            (10, 10),
            (11, 20),
            (20, 20),
            (21, 50),
            (50, 50),
            (51, 100),
            (100, 100),
            (101, 200),
            (999, 1000),
            (1000, 1000),
            (1001, 2000),
            (10_000_000_000_000_000_000, 10_000_000_000_000_000_000),
            (10_000_000_000_000_000_001, u64::MAX),
            (u64::MAX, u64::MAX),
        ] {
            let ceiling = chart_ceiling(maximum);
            assert_eq!(ceiling, expected);
            assert!(ceiling >= maximum);
            assert!(ceiling > 0);
        }
    }

    #[test]
    fn same_day_relative_time_uses_seconds_minutes_and_hours_boundaries() {
        let timestamp = now().timestamp();
        for (elapsed, expected) in [
            (0, "Just now"),
            (59, "Just now"),
            (60, "1m ago"),
            (3599, "59m ago"),
            (3600, "1h ago"),
            (7200, "2h ago"),
        ] {
            assert_eq!(relative(timestamp - elapsed), expected);
        }
    }

    #[test]
    fn previous_calendar_dates_take_priority_even_seconds_after_midnight() {
        let timezone = FixedOffset::east_opt(8 * 3600).unwrap();
        let midnight = timezone.with_ymd_and_hms(2026, 10, 22, 0, 0, 10).unwrap();
        let played = midnight - Duration::seconds(20);
        assert_eq!(
            relative_played_at(played.timestamp(), midnight, Language::English),
            "Yesterday 23:59"
        );
        let older = timezone.with_ymd_and_hms(2026, 10, 20, 23, 59, 0).unwrap();
        assert_eq!(relative(older.timestamp()), "10/20 23:59");
    }

    #[test]
    fn relative_dates_use_the_requested_timezone_across_utc_date_boundaries() {
        let utc_now = Utc.with_ymd_and_hms(2026, 10, 22, 0, 30, 0).unwrap();
        let played = Utc.with_ymd_and_hms(2026, 10, 21, 23, 30, 0).unwrap();
        for offset in [14, -12] {
            let timezone = FixedOffset::east_opt(offset * 3600).unwrap();
            assert_eq!(
                relative_played_at(
                    played.timestamp(),
                    utc_now.with_timezone(&timezone),
                    Language::English,
                ),
                "1h ago"
            );
        }
        let timezone = FixedOffset::east_opt(14 * 3600).unwrap();
        let local_midnight = timezone.with_ymd_and_hms(2026, 10, 22, 0, 10, 0).unwrap();
        let previous = Utc.with_ymd_and_hms(2026, 10, 21, 9, 50, 0).unwrap();
        assert_eq!(
            relative_played_at(previous.timestamp(), local_midnight, Language::English),
            "Yesterday 23:50"
        );
    }

    #[test]
    fn future_and_invalid_times_have_safe_nonnegative_fallbacks() {
        assert_eq!(relative(now().timestamp() + 30), "10/22 12:00");
        assert_eq!(relative(now().timestamp() + 86_400), "10/23 12:00");
        assert_eq!(relative(i64::MAX), "Unavailable");
        assert_eq!(relative(i64::MIN), "Unavailable");
        let timezone = FixedOffset::east_opt(3600).unwrap();
        let last_utc = DateTime::<Utc>::MAX_UTC;
        assert_eq!(
            relative_played_at(
                last_utc.timestamp(),
                last_utc.with_timezone(&timezone),
                Language::English,
            ),
            "Unavailable"
        );
    }

    #[test]
    fn relative_text_uses_the_existing_translation_catalog() {
        for language in Language::ALL {
            assert_eq!(
                relative_played_at(now().timestamp(), now(), language),
                translate(language, "Just now")
            );
            assert_eq!(
                relative_played_at(now().timestamp() - 300, now(), language),
                translate(language, "{0}m ago").replace("{0}", "5")
            );
        }
    }
}
