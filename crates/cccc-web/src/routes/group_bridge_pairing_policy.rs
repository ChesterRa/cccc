use cccc_contracts::utc_now;
use chrono::{DateTime, Utc};
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
}
