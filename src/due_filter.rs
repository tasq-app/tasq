//! Parsing for `due:` filter terms.
//!
//! Extends `due:` filtering beyond an exact `YYYY-MM-DD` match to ranges
//! anchored on today, reusing the `t:` threshold grammar (see
//! [`crate::threshold`]) so `due:` and `t:` offsets mean the same thing:
//!
//! ```text
//! due:YYYY-MM-DD   // exact match — a single-day range
//! due:[+-]?Nu       // range from today to the offset, u ∈ {d, w, m, b}
//! ```
//!
//! `due:+1w` matches tasks due between today and 7 days from now
//! (inclusive). `due:-3d` matches tasks due between 3 days ago and today.
//! No sign defaults to `+` (forward), matching `t:`'s convention.

use chrono::NaiveDate;

use crate::threshold::{self, ThresholdSpec};

/// Parse the *value* of a `due:` filter term (e.g. `"2026-08-01"`, `"+1w"`,
/// `"-3d"`) into an inclusive `(from, to)` ISO-date range to compare against
/// a task's `due:` field. An absolute date becomes a single-day range.
/// Returns `None` for unrecognized forms — callers fall back to treating the
/// term as literal search text.
pub fn parse_due_range(value: &str, today: NaiveDate) -> Option<(String, String)> {
    let spec = threshold::parse_threshold(value)?;
    match spec {
        ThresholdSpec::Absolute(d) => {
            let s = iso(d);
            Some((s.clone(), s))
        }
        ThresholdSpec::Relative { before, .. } => {
            let bound = threshold::resolve_at(&spec, today)?;
            let (from, to) = if before {
                (bound, today)
            } else {
                (today, bound)
            };
            debug_assert!(from <= to, "due-range bound computed out of order");
            Some((iso(from), iso(to)))
        }
    }
}

fn iso(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn absolute_date_is_single_day_range() {
        assert_eq!(
            parse_due_range("2026-08-01", d("2026-07-28")),
            Some(("2026-08-01".to_string(), "2026-08-01".to_string()))
        );
    }

    #[test]
    fn plus_offset_ranges_forward_from_today() {
        assert_eq!(
            parse_due_range("+1w", d("2026-07-28")),
            Some(("2026-07-28".to_string(), "2026-08-04".to_string()))
        );
    }

    #[test]
    fn bare_offset_defaults_forward() {
        assert_eq!(
            parse_due_range("7d", d("2026-07-28")),
            Some(("2026-07-28".to_string(), "2026-08-04".to_string()))
        );
    }

    #[test]
    fn minus_offset_ranges_backward_from_today() {
        assert_eq!(
            parse_due_range("-3d", d("2026-07-28")),
            Some(("2026-07-25".to_string(), "2026-07-28".to_string()))
        );
    }

    #[test]
    fn month_offset_clamps_at_month_end() {
        // Mar 31 - 1m -> Feb 28 (non-leap year), same clamping as `t:`.
        assert_eq!(
            parse_due_range("-1m", d("2026-03-31")),
            Some(("2026-02-28".to_string(), "2026-03-31".to_string()))
        );
    }

    #[test]
    fn business_day_offset_skips_weekends() {
        // Fri 2026-07-31 + 1b -> Mon 2026-08-03 (skip Sat/Sun).
        assert_eq!(
            parse_due_range("1b", d("2026-07-31")),
            Some(("2026-07-31".to_string(), "2026-08-03".to_string()))
        );
    }

    #[test]
    fn rejects_invalid_forms() {
        for bad in ["", "abc", "1y", "1.5d", "d", "-", "+", "1z"] {
            assert!(
                parse_due_range(bad, d("2026-07-28")).is_none(),
                "expected None for {bad:?}"
            );
        }
    }
}
