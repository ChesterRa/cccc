//! Unattended mode: which Group messages Chet also receives while the user is
//! away. One rule, read by the Web owner of Chet's Group inputs to decide what
//! to register and by the daemon to validate each registration.

use cccc_contracts::Event;
use cccc_contracts::chet::UnattendedSettings;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::{GroupDoc, HomeLayout, actors, settings, user_requests};

/// The current unattended settings of this instance.
pub fn load(home: &HomeLayout) -> std::io::Result<UnattendedSettings> {
    Ok(settings::load(home)?.unattended)
}

/// Whether Chet receives this message as a copy, and so may answer it for the
/// user: the mode is on, a visible member of this Group sent it to the user
/// with a reply obligation after the mode last turned on, and it is not
/// already addressed to Chet. A message from an earlier period of the mode
/// does not qualify after the mode is turned off and on again.
pub fn copies(settings: &UnattendedSettings, group: &GroupDoc, event: &Event) -> bool {
    let Some(since) = settings
        .since
        .as_deref()
        .filter(|_| settings.enabled)
        .and_then(parse)
    else {
        return false;
    };
    event.by != actors::CHET_SERVICE_PRINCIPAL
        && user_requests::targets_user(event)
        && matches!(
            event.data.get("message_mode").and_then(Value::as_str),
            Some("send" | "request_reply")
        )
        && event.data.get("transport").and_then(Value::as_str) != Some("im")
        && !event
            .data
            .get("to")
            .and_then(Value::as_array)
            .is_some_and(|to| to.contains(&json!(actors::CHET_SERVICE_PRINCIPAL)))
        && parse(&event.ts).is_some_and(|ts| ts >= since)
        && event.group_id == group.group_id
        && actors::find(group, &event.by).is_some_and(|actor| actor.internal_kind.is_none())
}

fn parse(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Actor;

    fn group() -> GroupDoc {
        let mut group: GroupDoc = serde_json::from_value(json!({
            "v": 1,
            "group_id": "g",
            "title": "t",
            "created_at": "2026-11-22T00:00:00Z",
            "updated_at": "2026-11-22T00:00:00Z",
        }))
        .expect("group fixture");
        group.actors.push(Actor::new("peer"));
        let mut internal = Actor::new("helper");
        internal.internal_kind = Some("internal".into());
        group.actors.push(internal);
        group
    }

    fn event(by: &str, data: Value, ts: &str) -> Event {
        let mut event = Event::new("chat.message", "g");
        event.by = by.into();
        event.ts = ts.into();
        event.data = data.as_object().expect("object data").clone();
        event
    }

    fn on() -> UnattendedSettings {
        UnattendedSettings {
            enabled: true,
            since: Some("2026-11-22T14:00:00.000000Z".into()),
        }
    }

    const LATER: &str = "2026-11-22T14:00:01.000000Z";

    #[test]
    fn copies_new_messages_to_the_user_only_while_on() {
        let message = event(
            "peer",
            json!({"to":["user"],"message_mode":"request_reply","text":"?"}),
            LATER,
        );
        assert!(copies(&on(), &group(), &message));
        assert!(!copies(&UnattendedSettings::default(), &group(), &message));
        let off = UnattendedSettings {
            enabled: false,
            ..on()
        };
        assert!(!copies(&off, &group(), &message));
    }

    #[test]
    fn skips_earlier_messages_and_ones_not_from_a_visible_member_to_the_user() {
        let earlier = event(
            "peer",
            json!({"to":["user"],"message_mode":"send"}),
            "2026-11-22T13:59:59.000000Z",
        );
        assert!(!copies(&on(), &group(), &earlier));
        for (by, data) in [
            ("peer", json!({"to":["svc:chet"],"message_mode":"send"})),
            (
                "peer",
                json!({"to":["user","svc:chet"],"message_mode":"send"}),
            ),
            ("peer", json!({"to":["other"],"message_mode":"send"})),
            ("user", json!({"to":["user"],"message_mode":"send"})),
            ("svc:chet", json!({"to":["user"],"message_mode":"send"})),
            ("helper", json!({"to":["user"],"message_mode":"send"})),
            ("gone", json!({"to":["user"],"message_mode":"send"})),
            (
                "peer",
                json!({"to":["user"],"message_mode":"send","transport":"im"}),
            ),
            (
                "peer",
                json!({"to":["user"],"message_mode":"send","dst_group_id":"g2"}),
            ),
        ] {
            assert!(
                !copies(&on(), &group(), &event(by, data.clone(), LATER)),
                "{by} {data}"
            );
        }
        let mut elsewhere = event("peer", json!({"to":["user"],"message_mode":"send"}), LATER);
        elsewhere.group_id = "g_other".into();
        assert!(!copies(&on(), &group(), &elsewhere));
    }
}
