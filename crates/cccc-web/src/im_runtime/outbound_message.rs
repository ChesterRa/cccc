use cccc_contracts::Event;
use serde_json::Value;

pub(super) fn outbound_text(event: &Event, markdown_bold: bool) -> Option<String> {
    let text = event_text(event)?;
    let sender = sender_label(event);
    Some(if markdown_bold {
        format!("**{sender}**\n\n{text}")
    } else {
        format!("{sender}\n\n{text}")
    })
}

fn event_text(event: &Event) -> Option<String> {
    if event.kind != "system.notify" {
        return event
            .data
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    let title = string_data(event, "title");
    let message = string_data(event, "text").or_else(|| string_data(event, "message"));
    match (title, message) {
        (Some(title), Some(message)) if title != message => Some(format!("{title}\n{message}")),
        (Some(title), _) => Some(title),
        (None, Some(message)) => Some(message),
        (None, None) => None,
    }
}

fn string_data(event: &Event, key: &str) -> Option<String> {
    event
        .data
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn sender_label(event: &Event) -> &str {
    event
        .data
        .get("sender_title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(&event.by)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(sender_title: Option<Value>) -> Event {
        let mut event = Event::new("chat.message", "group");
        event.by = "actor-id".into();
        event.data.insert("text".into(), json!("result"));
        if let Some(sender_title) = sender_title {
            event.data.insert("sender_title".into(), sender_title);
        }
        event
    }

    #[test]
    fn prefers_trimmed_sender_title() {
        assert_eq!(
            outbound_text(&message(Some(json!(" Review Bot "))), false).as_deref(),
            Some("Review Bot\n\nresult")
        );
    }

    #[test]
    fn falls_back_to_actor_id_for_missing_or_blank_title() {
        for sender_title in [None, Some(json!(" \t\n "))] {
            assert_eq!(
                outbound_text(&message(sender_title), true).as_deref(),
                Some("**actor-id**\n\nresult")
            );
        }
    }

    #[test]
    fn renders_system_notification_title_and_message() {
        let mut event = Event::new("system.notify", "group");
        event.by = "system".into();
        event.data.insert("title".into(), json!("Build failed"));
        event
            .data
            .insert("message".into(), json!("Check the worker logs."));

        assert_eq!(
            outbound_text(&event, false).as_deref(),
            Some("system\n\nBuild failed\nCheck the worker logs.")
        );
    }

    #[test]
    fn system_notification_accepts_legacy_text_payload() {
        let mut event = Event::new("system.notify", "group");
        event.by = "system".into();
        event
            .data
            .insert("text".into(), json!("Scheduled reminder"));

        assert_eq!(
            outbound_text(&event, true).as_deref(),
            Some("**system**\n\nScheduled reminder")
        );
    }
}
