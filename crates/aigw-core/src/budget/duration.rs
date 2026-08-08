//! Budget duration parsing and reset-time computation.
//!
//! Parses budget_duration strings like "1h", "7d", "1mo" into seconds,
//! and computes the next reset_at timestamp aligned to period boundaries
//! (compatible with litellm's budget reset semantics).

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc, Weekday};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Duration parsing
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Parse a duration string like "30s", "1h", "24h", "7d", "30d", "1mo" into seconds.
///
/// Also accepts aliases: "hourly"–3600, "daily"–86400, "weekly"–604800, "monthly"–2592000.
///
/// # Returns
///
/// * `Some(seconds)` if the input is a valid duration
/// * `None` if the input is empty, invalid, or unparseable
pub fn parse_duration_secs(input: &str) -> Option<i64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Handle named aliases first
    match trimmed.to_lowercase().as_str() {
        "hourly" => return Some(3600),
        "daily" => return Some(86400),
        "weekly" => return Some(604800),
        "monthly" => return Some(2592000),
        _ => {}
    }

    // Parse suffix-based formats
    if let Some(num_str) = trimmed.strip_suffix("mo") {
        let n: i64 = num_str.parse().ok()?;
        // 1 month ≈ 30 days in seconds
        return Some(n.saturating_mul(30 * 86400));
    }

    if let Some(num_str) = trimmed.strip_suffix('d') {
        let n: i64 = num_str.parse().ok()?;
        return Some(n.saturating_mul(86400));
    }

    if let Some(num_str) = trimmed.strip_suffix('h') {
        let n: i64 = num_str.parse().ok()?;
        return Some(n.saturating_mul(3600));
    }

    if let Some(num_str) = trimmed.strip_suffix('s') {
        let n: i64 = num_str.parse().ok()?;
        return Some(n);
    }

    None
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Reset-time computation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Compute the next `reset_at` timestamp based on budget duration.
///
/// Aligns with litellm:
///   * 24h → next UTC midnight (tomorrow 00:00:00)
///   * 7d  → next Monday 00:00:00 UTC
///   * 30d / 1mo → 1st of next month 00:00:00 UTC
///   * shorter durations (e.g. 1h) → now + duration, no alignment
///
/// The `_tz` and `_reset_time` parameters are reserved for future timezone-aware
/// alignment; they are currently unused.
///
/// # Returns
///
/// * `Some(DateTime<Utc>)` — the next reset timestamp
/// * `None` — if the duration string cannot be parsed
pub fn compute_next_reset_at(
    duration: &str,
    now: DateTime<Utc>,
    _tz: Option<&str>,
    _reset_time: Option<&str>,
) -> Option<DateTime<Utc>> {
    let dur_secs = parse_duration_secs(duration)?;

    match dur_secs {
        // 24h: next UTC midnight (tomorrow 00:00:00)
        86400 => {
            let tomorrow = now + Duration::days(1);
            let midnight = Utc
                .with_ymd_and_hms(tomorrow.year(), tomorrow.month(), tomorrow.day(), 0, 0, 0)
                .single()?;
            Some(midnight)
        }

        // 7d: next Monday 00:00:00 UTC
        // litellm: "start of next week (monday 00:00)"
        604800 => {
            let weekday = now.weekday();
            let days_until_monday = match weekday {
                Weekday::Mon => 7,
                Weekday::Tue => 6,
                Weekday::Wed => 5,
                Weekday::Thu => 4,
                Weekday::Fri => 3,
                Weekday::Sat => 2,
                Weekday::Sun => 1,
            };
            let target = now + Duration::days(days_until_monday as i64);
            let midnight = Utc
                .with_ymd_and_hms(target.year(), target.month(), target.day(), 0, 0, 0)
                .single()?;
            Some(midnight)
        }

        // 30d / 31d / 1mo (2592000–2678400): 1st of next month 00:00:00 UTC
        d if (2592000..=2678400).contains(&d) => {
            let (year, month) = if now.month() == 12 {
                (now.year() + 1, 1)
            } else {
                (now.year(), now.month() + 1)
            };
            let first_of_next = Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).single()?;
            Some(first_of_next)
        }

        // Other durations: simple addition, no period alignment
        secs => Some(now + Duration::seconds(secs)),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unit tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    // ── parse_duration_secs ──────────────────────────────────

    #[test]
    fn parse_30s() {
        assert_eq!(parse_duration_secs("30s"), Some(30));
    }

    #[test]
    fn parse_1h() {
        assert_eq!(parse_duration_secs("1h"), Some(3600));
    }

    #[test]
    fn parse_24h() {
        assert_eq!(parse_duration_secs("24h"), Some(86400));
    }

    #[test]
    fn parse_7d() {
        assert_eq!(parse_duration_secs("7d"), Some(604800));
    }

    #[test]
    fn parse_30d() {
        assert_eq!(parse_duration_secs("30d"), Some(2592000));
    }

    #[test]
    fn parse_1mo() {
        assert_eq!(parse_duration_secs("1mo"), Some(2592000));
    }

    #[test]
    fn parse_2mo() {
        assert_eq!(parse_duration_secs("2mo"), Some(2 * 2592000));
    }

    #[test]
    fn parse_aliases() {
        assert_eq!(parse_duration_secs("hourly"), Some(3600));
        assert_eq!(parse_duration_secs("daily"), Some(86400));
        assert_eq!(parse_duration_secs("weekly"), Some(604800));
        assert_eq!(parse_duration_secs("monthly"), Some(2592000));
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(parse_duration_secs("HOURLY"), Some(3600));
        assert_eq!(parse_duration_secs("Daily"), Some(86400));
    }

    #[test]
    fn parse_invalid() {
        assert_eq!(parse_duration_secs(""), None);
        assert_eq!(parse_duration_secs("garbage"), None);
        assert_eq!(parse_duration_secs("xyz"), None);
        assert_eq!(parse_duration_secs("1x"), None);
    }

    // ── compute_next_reset_at ────────────────────────────────

    #[test]
    fn compute_next_24h() {
        // 2026-08-02T14:30:00Z → next UTC midnight = 2026-08-03T00:00:00Z
        let now = Utc
            .with_ymd_and_hms(2026, 8, 2, 14, 30, 0)
            .single()
            .unwrap();
        let next = compute_next_reset_at("24h", now, None, None).unwrap();
        let expected = Utc.with_ymd_and_hms(2026, 8, 3, 0, 0, 0).single().unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn compute_next_7d_from_monday() {
        // Monday 2026-07-27T10:00:00Z → next Monday = 2026-08-03T00:00:00Z
        let now = Utc
            .with_ymd_and_hms(2026, 7, 27, 10, 0, 0)
            .single()
            .unwrap();
        let next = compute_next_reset_at("7d", now, None, None).unwrap();
        let expected = Utc.with_ymd_and_hms(2026, 8, 3, 0, 0, 0).single().unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn compute_next_7d_from_wednesday() {
        // Wednesday 2026-07-29T10:00:00Z → next Monday = 2026-08-03T00:00:00Z
        let now = Utc
            .with_ymd_and_hms(2026, 7, 29, 10, 0, 0)
            .single()
            .unwrap();
        let next = compute_next_reset_at("7d", now, None, None).unwrap();
        let expected = Utc.with_ymd_and_hms(2026, 8, 3, 0, 0, 0).single().unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn compute_next_7d_from_sunday() {
        // Sunday 2026-08-02T10:00:00Z → next Monday = 2026-08-03T00:00:00Z
        let now = Utc.with_ymd_and_hms(2026, 8, 2, 10, 0, 0).single().unwrap();
        let next = compute_next_reset_at("7d", now, None, None).unwrap();
        let expected = Utc.with_ymd_and_hms(2026, 8, 3, 0, 0, 0).single().unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn compute_next_1mo() {
        // 2026-08-15T10:00:00Z → 1st of next month = 2026-09-01T00:00:00Z
        let now = Utc
            .with_ymd_and_hms(2026, 8, 15, 10, 0, 0)
            .single()
            .unwrap();
        let next = compute_next_reset_at("1mo", now, None, None).unwrap();
        let expected = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).single().unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn compute_next_1mo_december_rollover() {
        // 2026-12-15T10:00:00Z → 1st of next month = 2027-01-01T00:00:00Z
        let now = Utc
            .with_ymd_and_hms(2026, 12, 15, 10, 0, 0)
            .single()
            .unwrap();
        let next = compute_next_reset_at("1mo", now, None, None).unwrap();
        let expected = Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).single().unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn compute_next_1h() {
        // Short duration: now + 1h, no alignment
        let now = Utc
            .with_ymd_and_hms(2026, 8, 2, 14, 30, 0)
            .single()
            .unwrap();
        let next = compute_next_reset_at("1h", now, None, None).unwrap();
        assert_eq!(next, now + Duration::hours(1));
    }

    #[test]
    fn compute_next_30s() {
        let now = Utc
            .with_ymd_and_hms(2026, 8, 2, 14, 30, 0)
            .single()
            .unwrap();
        let next = compute_next_reset_at("30s", now, None, None).unwrap();
        assert_eq!(next, now + Duration::seconds(30));
    }

    #[test]
    fn compute_next_invalid_duration() {
        let now = Utc::now();
        assert_eq!(compute_next_reset_at("garbage", now, None, None), None);
        assert_eq!(compute_next_reset_at("", now, None, None), None);
    }
}
