//! Elasticsearch date-math expression resolution.
//!
//! Resolves ES date-math expressions (`now`, `now-15m`, `now/d`,
//! `2024-01-01||+1d`) to RFC3339 UTC timestamps. This is the single source of
//! truth used by two callers:
//!
//! - `prism-es-compat`: structured ES `range` queries resolve their bounds here
//!   at translation time.
//! - `prism::backends::text`: raw `query_string` queries (e.g. Kibana
//!   task_manager's `task.runAt:[* TO now]`) carry literal date-math inside
//!   Lucene range expressions; `resolve_datemath_ranges` rewrites those bounds
//!   to RFC3339 before handing the query string to Tantivy, whose date field
//!   would otherwise reject `now` with "The date field has an invalid format".
//!
//! Anything that is not recognized date-math (plain numbers, version strings,
//! ISO timestamps, `*`) is returned unchanged, so it is always safe to run an
//! arbitrary bound through [`resolve_date_math`].

use chrono::{DateTime, Datelike, Duration, Months, TimeZone, Timelike, Utc};

/// Resolve an ES date-math expression to an RFC3339 UTC string. Returns the
/// input unchanged when it is not recognized date-math.
pub fn resolve_date_math(value: &str) -> String {
    match resolve_date_expr(value, Utc::now()) {
        Some(dt) => dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        None => value.to_string(),
    }
}

/// True when `value` looks like date-math (an anchor `now` or a `||`-suffixed
/// date). Used by callers that want to gate the rewrite without paying for a
/// full parse.
pub fn looks_like_date_math(value: &str) -> bool {
    let v = value.trim();
    v.starts_with("now") || v.contains("||")
}

/// Core date-math evaluator parameterized by `now` for deterministic testing.
/// Returns `None` when `expr` is not recognized date-math.
pub fn resolve_date_expr(expr: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let expr = expr.trim();
    if expr.is_empty() {
        return None;
    }

    // Split an optional trailing rounding operator: `/unit` (ES allows exactly
    // one). RFC3339 dates never contain '/', so this is unambiguous.
    let (main, rounding) = match expr.rfind('/') {
        Some(idx) => (&expr[..idx], Some(&expr[idx + 1..])),
        None => (expr, None),
    };

    // Determine the anchor instant and the remaining operator string.
    let (mut dt, mut ops): (DateTime<Utc>, &str) = if let Some(rest) = main.strip_prefix("now") {
        (now, rest)
    } else if let Some(idx) = main.find("||") {
        let (date_part, after) = (&main[..idx], &main[idx + 2..]);
        match parse_anchor_date(date_part) {
            Some(d) => (d, after),
            None => return None,
        }
    } else {
        // No `now` anchor and no `||`: not date-math. Leave to the caller.
        return None;
    };

    // Apply the leading run of `+/-<amount><unit>` operators.
    while !ops.is_empty() {
        let sign = match ops.chars().next() {
            Some('+') => 1i64,
            Some('-') => -1i64,
            _ => break, // malformed trailing text; stop gracefully
        };
        let rest = &ops[1..];
        let num_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if num_end == 0 {
            break;
        }
        let amount: i64 = match rest[..num_end].parse() {
            Ok(n) => n,
            Err(_) => break,
        };
        let unit = match rest[num_end..].chars().next() {
            Some(u) => u,
            None => break,
        };
        ops = &rest[num_end + unit.len_utf8()..];
        dt = add_period(dt, sign * amount, unit)?;
    }

    // Round down to the start of the requested unit.
    if let Some(unit_str) = rounding {
        if let Some(u) = unit_str.chars().next() {
            dt = round_down(dt, u)?;
        }
    }

    Some(dt)
}

/// Parse a date-math anchor: RFC3339, bare `YYYY-MM-DD`, or a naive datetime.
fn parse_anchor_date(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|t| t.and_utc())
        })
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|nd| nd.and_hms_opt(0, 0, 0).map(|t| t.and_utc()))
        })
}

/// Add a signed amount of a calendar/clock period to a datetime.
fn add_period(dt: DateTime<Utc>, signed_amount: i64, unit: char) -> Option<DateTime<Utc>> {
    match unit {
        'y' => add_months(dt, signed_amount * 12),
        'M' => add_months(dt, signed_amount),
        'w' => dt.checked_add_signed(Duration::days(signed_amount * 7)),
        'd' => dt.checked_add_signed(Duration::days(signed_amount)),
        'h' | 'H' => dt.checked_add_signed(Duration::hours(signed_amount)),
        'm' => dt.checked_add_signed(Duration::minutes(signed_amount)),
        's' => dt.checked_add_signed(Duration::seconds(signed_amount)),
        _ => None,
    }
}

/// Signed month arithmetic (chrono's `Months` is unsigned).
fn add_months(dt: DateTime<Utc>, n: i64) -> Option<DateTime<Utc>> {
    if n >= 0 {
        dt.checked_add_months(Months::new(n as u32))
    } else {
        dt.checked_sub_months(Months::new((-n) as u32))
    }
}

/// Round a datetime DOWN to the start of the given calendar unit (ES rounding).
fn round_down(dt: DateTime<Utc>, unit: char) -> Option<DateTime<Utc>> {
    let y = dt.year();
    let mo = dt.month();
    let d = dt.day();
    let h = dt.hour();
    let mi = dt.minute();
    let s = dt.second();
    let res = match unit {
        'y' => Utc.with_ymd_and_hms(y, 1, 1, 0, 0, 0),
        'M' => Utc.with_ymd_and_hms(y, mo, 1, 0, 0, 0),
        'w' => {
            // ES weeks start on Monday.
            let wd = dt.weekday().num_days_from_monday() as i64;
            let monday = dt - Duration::days(wd);
            Utc.with_ymd_and_hms(monday.year(), monday.month(), monday.day(), 0, 0, 0)
        }
        'd' => Utc.with_ymd_and_hms(y, mo, d, 0, 0, 0),
        'h' | 'H' => Utc.with_ymd_and_hms(y, mo, d, h, 0, 0),
        'm' => Utc.with_ymd_and_hms(y, mo, d, h, mi, 0),
        's' => Utc.with_ymd_and_hms(y, mo, d, h, mi, s),
        _ => return None,
    };
    res.single()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-13T12:30:45Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn now_passthrough() {
        let now = fixed_now();
        assert_eq!(resolve_date_expr("now", now).unwrap(), now);
    }

    #[test]
    fn offsets() {
        let now = fixed_now();
        assert_eq!(
            resolve_date_expr("now-15m", now).unwrap(),
            now - Duration::minutes(15)
        );
        assert_eq!(
            resolve_date_expr("now+1h", now).unwrap(),
            now + Duration::hours(1)
        );
        assert_eq!(
            resolve_date_expr("now-1d", now).unwrap(),
            now - Duration::days(1)
        );
        assert_eq!(
            resolve_date_expr("now+30s", now).unwrap(),
            now + Duration::seconds(30)
        );
    }

    #[test]
    fn rounding() {
        let now = fixed_now();
        let day_start = Utc.with_ymd_and_hms(2026, 8, 13, 0, 0, 0).single().unwrap();
        assert_eq!(resolve_date_expr("now/d", now).unwrap(), day_start);
        // now-1d/d → start of yesterday
        let yest = Utc.with_ymd_and_hms(2026, 8, 12, 0, 0, 0).single().unwrap();
        assert_eq!(resolve_date_expr("now-1d/d", now).unwrap(), yest);
        // now/h → top of the current hour
        let hour_start = Utc
            .with_ymd_and_hms(2026, 8, 13, 12, 0, 0)
            .single()
            .unwrap();
        assert_eq!(resolve_date_expr("now/h", now).unwrap(), hour_start);
        // now/M → first of month
        let month_start = Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).single().unwrap();
        assert_eq!(resolve_date_expr("now/M", now).unwrap(), month_start);
    }

    #[test]
    fn anchor() {
        let now = Utc::now();
        let d = resolve_date_expr("2024-01-01||+1d", now).unwrap();
        assert_eq!(
            d,
            Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).single().unwrap()
        );
        let d2 = resolve_date_expr("2024-06-15||-1M", now).unwrap();
        assert_eq!(
            d2,
            Utc.with_ymd_and_hms(2024, 5, 15, 0, 0, 0).single().unwrap()
        );
    }

    #[test]
    fn not_date_math() {
        let now = Utc::now();
        // Plain dates, numbers, and empty strings are not date-math → None,
        // so the caller leaves them untouched.
        assert_eq!(resolve_date_expr("2024-01-01", now), None);
        assert_eq!(resolve_date_expr("42", now), None);
        assert_eq!(resolve_date_expr("active", now), None);
        assert_eq!(resolve_date_expr("", now), None);
    }

    #[test]
    fn resolve_date_math_produces_rfc3339() {
        let s = resolve_date_math("now-15m");
        assert!(!s.contains("now"));
        assert!(s.contains('T'));
        assert!(s.ends_with('Z'));
    }

    #[test]
    fn resolve_date_math_passes_through_non_math() {
        assert_eq!(resolve_date_math("42"), "42");
        assert_eq!(resolve_date_math("7.14.1"), "7.14.1");
        assert_eq!(resolve_date_math("active"), "active");
        assert_eq!(resolve_date_math("*"), "*");
        assert_eq!(resolve_date_math("2024-01-01"), "2024-01-01");
    }

    #[test]
    fn looks_like_helper() {
        assert!(looks_like_date_math("now"));
        assert!(looks_like_date_math("now-15m"));
        assert!(looks_like_date_math("2024-01-01||+1d"));
        assert!(!looks_like_date_math("42"));
        assert!(!looks_like_date_math("7.14.1"));
        assert!(!looks_like_date_math("2024-01-01"));
    }
}
