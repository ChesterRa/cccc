use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use serde_json::{Map, Value};
use std::str::FromStr;

pub fn is_due(trigger: Option<&Map<String, Value>>, last: Option<i64>, now: DateTime<Utc>) -> bool {
    let kind = trigger
        .and_then(|trigger| trigger.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("interval");
    match kind {
        "interval" => {
            let seconds = trigger
                .and_then(|trigger| trigger.get("every_seconds"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            seconds > 0 && now.timestamp() - last.unwrap_or(0) >= seconds
        }
        "at" => {
            let at = trigger
                .and_then(|trigger| trigger.get("at"))
                .and_then(Value::as_str)
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok());
            last.is_none() && at.is_some_and(|at| at.with_timezone(&Utc) <= now)
        }
        "cron" => cron_due(trigger, last, now),
        _ => false,
    }
}

pub(crate) fn next_fire_at(
    trigger: Option<&Map<String, Value>>,
    last: Option<i64>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let kind = trigger
        .and_then(|trigger| trigger.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("interval");
    match kind {
        "interval" => {
            let seconds = trigger
                .and_then(|trigger| trigger.get("every_seconds"))
                .and_then(Value::as_i64)?;
            if seconds <= 0 {
                return None;
            }
            let base = last
                .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
                .unwrap_or(now);
            base.checked_add_signed(TimeDelta::seconds(seconds))
        }
        "at" if last.is_none() => trigger
            .and_then(|trigger| trigger.get("at"))
            .and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc)),
        "cron" => cron_next(trigger, now),
        _ => None,
    }
}

fn cron_due(trigger: Option<&Map<String, Value>>, last: Option<i64>, now: DateTime<Utc>) -> bool {
    let raw = trigger
        .and_then(|trigger| trigger.get("cron"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let Ok(schedule) = parse_cron_schedule(raw) else {
        return false;
    };
    let Ok(timezone) = trigger
        .and_then(|trigger| trigger.get("timezone"))
        .and_then(Value::as_str)
        .unwrap_or("UTC")
        .parse::<Tz>()
    else {
        return false;
    };
    let base = last
        .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
        .unwrap_or_else(|| now - TimeDelta::seconds(61))
        .with_timezone(&timezone);
    schedule
        .after(&base)
        .next()
        .is_some_and(|next| next <= now.with_timezone(&timezone))
}

fn cron_next(trigger: Option<&Map<String, Value>>, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let raw = trigger
        .and_then(|trigger| trigger.get("cron"))
        .and_then(Value::as_str)?;
    let schedule = parse_cron_schedule(raw).ok()?;
    let timezone = trigger
        .and_then(|trigger| trigger.get("timezone"))
        .and_then(Value::as_str)
        .unwrap_or("UTC")
        .parse::<Tz>()
        .ok()?;
    schedule
        .after(&now.with_timezone(&timezone))
        .next()
        .map(|value| value.with_timezone(&Utc))
}

/// Parse an automation cron expression. Day-of-week fields use POSIX numbering
/// (0 or 7 = Sunday, 1 = Monday); the underlying parser numbers days 1-7
/// starting Sunday and rejects 0, so numeric days are translated to weekday
/// names before parsing.
pub fn parse_cron_schedule(raw: &str) -> Result<Schedule, cron::error::Error> {
    Schedule::from_str(&normalize_cron_expression(raw))
}

fn normalize_cron_expression(raw: &str) -> String {
    let fields: Vec<&str> = raw.split_whitespace().collect();
    let mut parts: Vec<String> = if fields.len() == 5 {
        std::iter::once("0".to_owned())
            .chain(fields.iter().map(|field| field.to_string()))
            .collect()
    } else {
        fields.iter().map(|field| field.to_string()).collect()
    };
    if parts.len() >= 6 {
        parts[5] = posix_dow_field(&parts[5]);
    }
    parts.join(" ")
}

const DOW_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

fn posix_dow_field(field: &str) -> String {
    field
        .split(',')
        .map(posix_dow_atom)
        .collect::<Vec<_>>()
        .join(",")
}

fn dow_index(value: &str) -> Option<usize> {
    let lower = value.to_ascii_lowercase();
    if let Some(index) = DOW_NAMES.iter().position(|name| *name == lower) {
        return Some(index);
    }
    lower
        .parse::<usize>()
        .ok()
        .filter(|day| *day <= 7)
        .map(|day| day % 7)
}

fn dow_name(index: usize) -> &'static str {
    DOW_NAMES[index % 7]
}

/// A day-of-week value as written: a weekday name maps to 0-6 and a number
/// stays 0-7, so a range can still tell 0 (Sunday, start of week) from 7
/// (Sunday, end of week) before both fold to the same index.
fn dow_raw(value: &str) -> Option<usize> {
    let lower = value.to_ascii_lowercase();
    if let Some(index) = DOW_NAMES.iter().position(|name| *name == lower) {
        return Some(index);
    }
    lower.parse::<usize>().ok().filter(|day| *day <= 7)
}

/// The days a range covers, in order, without repeats. `0-7` and `1-7` cover
/// the whole week and `7-7` is Sunday alone; a start after the end wraps
/// through Sunday (`5-1` is Friday to Monday). 7 as a start means Sunday.
fn dow_range(start: &str, end: &str) -> Option<Vec<usize>> {
    let (a, b) = (dow_raw(start)?, dow_raw(end)?);
    if a == b {
        return Some(vec![a % 7]);
    }
    let a = if a == 7 { 0 } else { a };
    let days: Vec<usize> = if a <= b {
        (a..=b).map(|day| day % 7).collect()
    } else {
        (a..=6).chain(0..=b).collect()
    };
    let mut seen = [false; 7];
    Some(
        days.into_iter()
            .filter(|day| !std::mem::replace(&mut seen[*day], true))
            .collect(),
    )
}

fn dow_list(days: impl IntoIterator<Item = usize>) -> String {
    days.into_iter().map(dow_name).collect::<Vec<_>>().join(",")
}

fn posix_dow_atom(atom: &str) -> String {
    let (base, step) = match atom.split_once('/') {
        Some((base, step)) => (base, Some(step)),
        None => (atom, None),
    };
    if let Some(step) = step {
        return expand_dow_step(base, step);
    }
    match base.split_once('-') {
        Some((start, end)) => match dow_range(start, end) {
            Some(days) => dow_list(days),
            None => format!(
                "{}-{}",
                dow_index(start).map(dow_name).unwrap_or(start),
                dow_index(end).map(dow_name).unwrap_or(end)
            ),
        },
        None => match dow_index(base) {
            Some(day) if base.parse::<usize>().is_ok() => dow_name(day).to_owned(),
            _ => base.to_owned(),
        },
    }
}

fn expand_dow_step(base: &str, step: &str) -> String {
    let Ok(step) = step.parse::<usize>() else {
        return format!("{base}/{step}");
    };
    if step == 0 {
        return format!("{base}/{step}");
    }
    let values: Vec<usize> = match base.split_once('-') {
        Some((start, end)) => match dow_range(start, end) {
            Some(days) => days.into_iter().step_by(step).collect(),
            None => return format!("{base}/{step}"),
        },
        None if base == "*" || base == "?" => (0..=6).step_by(step).collect(),
        None => match dow_index(base) {
            Some(a) => (a..=6).step_by(step).collect(),
            None => return format!("{base}/{step}"),
        },
    };
    if values.is_empty() {
        return format!("{base}/{step}");
    }
    dow_list(values)
}

#[cfg(test)]
mod tests {
    use super::{is_due, next_fire_at, parse_cron_schedule};
    use chrono::{Datelike, TimeZone, Utc, Weekday};
    use serde_json::json;

    #[test]
    fn supports_canonical_interval_and_at_triggers() {
        let now = Utc::now();
        let interval = json!({"kind":"interval","every_seconds":60});
        assert!(is_due(
            interval.as_object(),
            Some(now.timestamp() - 60),
            now
        ));
        let at = json!({"kind":"at","at":now.to_rfc3339()});
        assert!(is_due(at.as_object(), None, now));
        assert!(!is_due(at.as_object(), Some(now.timestamp()), now));
    }

    fn cron_trigger(expression: &str) -> serde_json::Map<String, serde_json::Value> {
        json!({"kind":"cron","cron":expression})
            .as_object()
            .cloned()
            .unwrap()
    }

    #[test]
    fn cron_day_of_week_uses_posix_numbering() {
        // 2026-09-25 is a Friday; POSIX: 0 and 7 are Sunday, 1 Monday, 2 Tuesday.
        let now = Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap();
        for (expression, weekday) in [
            ("0 0 * * 0", Weekday::Sun),
            ("0 0 * * 7", Weekday::Sun),
            ("0 0 * * 1", Weekday::Mon),
            ("0 0 * * 2", Weekday::Tue),
            ("0 0 * * 6", Weekday::Sat),
            ("0 0 * * sun", Weekday::Sun),
            ("0 0 * * sat", Weekday::Sat),
        ] {
            let trigger = cron_trigger(expression);
            let next = next_fire_at(Some(&trigger), None, now)
                .unwrap_or_else(|| panic!("{expression} should produce a next fire"));
            assert_eq!(
                next.weekday(),
                weekday,
                "{expression} must fire on {weekday}"
            );
        }
    }

    #[test]
    fn cron_day_of_week_weekday_range_fires_next_weekday() {
        let now = Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap(); // Friday noon
        let trigger = cron_trigger("0 9 * * mon-fri");
        let next = next_fire_at(Some(&trigger), None, now).unwrap();
        assert_eq!(next.weekday(), Weekday::Mon);
    }

    /// The weekdays an expression fires on, sampled over two weeks.
    fn fire_weekdays(expression: &str) -> Vec<Weekday> {
        let start = Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap();
        let schedule = parse_cron_schedule(expression)
            .unwrap_or_else(|error| panic!("{expression} should parse: {error}"));
        let mut days: Vec<Weekday> = schedule
            .after(&start)
            .take(14)
            .map(|time| time.weekday())
            .collect();
        days.sort_by_key(|day| day.num_days_from_sunday());
        days.dedup();
        days
    }

    #[test]
    fn cron_day_of_week_ranges_that_span_both_sundays() {
        use Weekday::{Fri, Mon, Sat, Sun, Thu, Tue, Wed};
        let week = vec![Sun, Mon, Tue, Wed, Thu, Fri, Sat];
        // 0 and 7 are both Sunday, so 0-7 is the whole week, not Sunday only.
        assert_eq!(fire_weekdays("0 9 * * 0-7"), week);
        assert_eq!(fire_weekdays("0 9 * * 1-7"), week);
        assert_eq!(fire_weekdays("0 9 * * 7-7"), vec![Sun]);
        assert_eq!(fire_weekdays("0 9 * * 5-7"), vec![Sun, Fri, Sat]);
        assert_eq!(fire_weekdays("0 9 * * 1-5"), vec![Mon, Tue, Wed, Thu, Fri]);
        // Steps over the same range must not collapse either.
        assert_eq!(fire_weekdays("0 9 * * 0-7/2"), vec![Sun, Tue, Thu, Sat]);
        assert_eq!(fire_weekdays("0 9 * * 1-7/2"), vec![Sun, Mon, Wed, Fri]);
        assert_eq!(fire_weekdays("0 9 * * */2"), vec![Sun, Tue, Thu, Sat]);
    }

    #[test]
    fn invalid_cron_expressions_are_rejected() {
        assert!(parse_cron_schedule("not a cron").is_err());
        assert!(parse_cron_schedule("0 0 * * 8").is_err());
        assert!(parse_cron_schedule("0 0 * *").is_err());
    }
}
