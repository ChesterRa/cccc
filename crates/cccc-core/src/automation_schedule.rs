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
    let expression = if raw.split_whitespace().count() == 5 {
        format!("0 {raw}")
    } else {
        raw.to_owned()
    };
    let Ok(schedule) = Schedule::from_str(&expression) else {
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
    let expression = if raw.split_whitespace().count() == 5 {
        format!("0 {raw}")
    } else {
        raw.to_owned()
    };
    let schedule = Schedule::from_str(&expression).ok()?;
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

#[cfg(test)]
mod tests {
    use super::is_due;
    use chrono::Utc;
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
}
