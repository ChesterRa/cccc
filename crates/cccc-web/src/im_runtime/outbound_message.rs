use cccc_contracts::Event;
use serde_json::Value;

pub(super) fn outbound_text(event: &Event, markdown_bold: bool) -> Option<String> {
    let text = event.data.get("text").and_then(Value::as_str)?;
    let sender = sender_label(event);
    Some(if markdown_bold {
        format!("**{sender}**\n\n{text}")
    } else {
        format!("{sender}\n\n{text}")
    })
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
}
