use cccc_contracts::utc_now;
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use std::io;

pub(super) fn consume_pending_invite(invite: &mut Value) -> io::Result<bool> {
    if invite["status"] != "pending" {
        return Err(io::Error::other("pairing invite is not pending"));
    }
    if timestamp_not_live(&invite["expires_at"]) {
        invite["status"] = json!("expired");
        invite["updated_at"] = json!(utc_now());
        return Ok(false);
    }
    Ok(true)
}

pub(super) fn timestamp_not_live(value: &Value) -> bool {
    value
        .as_str()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|value| value.with_timezone(&Utc) <= Utc::now())
}

pub(super) fn migrate_legacy_claim_window(request: &mut Value, now: DateTime<Utc>) -> bool {
    if request["status"] != "approved"
        || request
            .get("claimed_at")
            .is_some_and(|value| !value.is_null())
        || request
            .get("claim_expires_at")
            .is_some_and(|value| !value.is_null())
    {
        return false;
    }

    let grace_expires_at = now + Duration::minutes(10);
    let anchored_expires_at = ["approved_at", "updated_at"]
        .into_iter()
        .filter_map(|field| request[field].as_str())
        .filter_map(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc) + Duration::minutes(10))
        .find(|expires_at| *expires_at > now && *expires_at <= grace_expires_at);
    request["claim_expires_at"] = json!(anchored_expires_at.unwrap_or(grace_expires_at));
    request["claimed_at"] = Value::Null;
    request["claim_window_migrated_at"] = json!(now);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_future_invite_is_consumable() {
        let mut invite = json!({"status":"pending","expires_at":"2099-01-01T00:00:00Z"});
        assert!(consume_pending_invite(&mut invite).expect("invite"));
        assert_eq!(invite["status"], "pending");
    }

    #[test]
    fn expired_invite_is_persistably_marked() {
        let mut invite = json!({"status":"pending","expires_at":"2020-01-01T00:00:00Z"});
        assert!(!consume_pending_invite(&mut invite).expect("invite"));
        assert_eq!(invite["status"], "expired");
    }

    #[test]
    fn legacy_claim_window_is_bounded_and_idempotent() {
        let now = DateTime::parse_from_rfc3339("2026-08-31T12:00:00Z")
            .expect("now")
            .with_timezone(&Utc);
        let mut request = json!({
            "status":"approved","updated_at":"2026-08-30T12:00:00Z"
        });

        assert!(migrate_legacy_claim_window(&mut request, now));
        assert_eq!(request["claim_expires_at"], "2026-08-31T12:10:00Z");
        assert_eq!(request["claim_window_migrated_at"], "2026-08-31T12:00:00Z");
        assert!(!migrate_legacy_claim_window(&mut request, now));
    }

    #[test]
    fn explicit_or_claimed_legacy_records_are_not_reopened() {
        let now = Utc::now();
        for mut request in [
            json!({"status":"approved","claim_expires_at":"malformed"}),
            json!({"status":"approved","claimed_at":"2026-08-31T12:00:00Z"}),
        ] {
            assert!(!migrate_legacy_claim_window(&mut request, now));
        }
    }
}
