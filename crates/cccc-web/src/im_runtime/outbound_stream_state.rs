use cccc_contracts::Event;
use serde_json::Value;
use std::collections::HashMap;
use std::hash::Hash;

pub(super) const MAX_ACTIVE_STREAMS: usize = 1_024;

/// Bound abandoned stream handles. Evicted streams safely fall back to their
/// final `chat.message`, so cleanup must prefer eviction over unbounded growth.
pub(super) fn trim_active<K, V>(streams: &mut HashMap<K, V>)
where
    K: Clone + Eq + Hash,
{
    while streams.len() > MAX_ACTIVE_STREAMS {
        let Some(key) = streams.keys().next().cloned() else {
            break;
        };
        streams.remove(&key);
    }
}

/// Read a string field of a stream event, trimmed. A missing, non-string or
/// whitespace-only field reads as empty; each caller decides what empty means.
pub(super) fn event_string(event: &Event, key: &str) -> String {
    event
        .data
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A captured `chat.message` ledger row, redacted: the group, actor and
    /// recipient names and the message text are replaced, and the long
    /// free-text fields are dropped.
    const CAPTURED_ROW: &str = r#"{"v":1,"id":"b2e2c02bc0254c569b6a6d8ff01fff4c","ts":"2026-11-05T19:45:02.255175Z","kind":"chat.message","group_id":"group","scope_key":"","by":"actor","data":{"format":"plain","message_mode":"mail","reply_to":"e5e2ed05edee44b28325d9c180092b22","sender_runtime":"codex","text":"[redacted]","to":["foreman"]}}"#;

    fn captured() -> Event {
        serde_json::from_str(CAPTURED_ROW).expect("captured row parses")
    }

    #[test]
    fn event_string_reads_a_plain_string_field() {
        assert_eq!(event_string(&captured(), "format"), "plain");
        assert_eq!(event_string(&captured(), "message_mode"), "mail");
    }

    #[test]
    fn event_string_is_empty_for_a_missing_field() {
        assert_eq!(event_string(&captured(), "stream_id"), "");
        assert_eq!(event_string(&captured(), "op"), "");
    }

    #[test]
    fn event_string_is_empty_for_a_non_string_field() {
        // `to` is an array in the captured row.
        assert_eq!(event_string(&captured(), "to"), "");
    }

    #[test]
    fn event_string_trims_and_empties_whitespace() {
        // Mutation of the captured row: padded and whitespace-only values.
        let mut event = captured();
        event.data.insert("format".into(), Value::from("  plain\n"));
        event.data.insert("text".into(), Value::from(" \t\n "));
        assert_eq!(event_string(&event, "format"), "plain");
        assert_eq!(event_string(&event, "text"), "");
    }

    #[test]
    fn abandoned_streams_are_bounded() {
        let mut streams = (0..=MAX_ACTIVE_STREAMS)
            .map(|index| (index, index))
            .collect();

        trim_active(&mut streams);

        assert_eq!(streams.len(), MAX_ACTIVE_STREAMS);
    }
}
