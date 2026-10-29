use serde_json::{Map, Value};

/// An exact `cccc_message_history` read (`event_id`) already shows the message
/// as the Actor receives it in `delivery_text`. Return its event once, as
/// `message`, and omit a chat `text` or `insight` from it only when that
/// complete value appears in `delivery_text`. Whatever the rendering leaves
/// out or abbreviates (an `/install` command's own text, an unrenderable
/// insight, quotes, attachments, source fields) stays in the event.
pub(crate) fn apply(
    tool_name: &str,
    result: &mut Map<String, Value>,
    arguments: &Map<String, Value>,
) {
    let exact_read = arguments
        .get("event_id")
        .and_then(Value::as_str)
        .is_some_and(|event_id| !event_id.trim().is_empty());
    if tool_name != "cccc_message_history" || !exact_read || !result.contains_key("delivery_text") {
        return;
    }
    let mut event = match result.get_mut("messages").and_then(Value::as_array_mut) {
        Some(messages) if matches!(messages.as_slice(), [Value::Object(_)]) => messages.remove(0),
        _ => return,
    };
    let delivery_text = result
        .get("delivery_text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !delivery_text.is_empty()
        && event.get("kind").and_then(Value::as_str) == Some("chat.message")
        && let Some(data) = event.get_mut("data").and_then(Value::as_object_mut)
    {
        // Compare the values as the renderer shows them: text without its
        // trailing line breaks and insight in its normalized form.
        if data
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| delivery_text.contains(text.trim_end_matches(['\r', '\n'])))
        {
            data.remove("text");
        }
        if cccc_core::peer_insight::normalize(data.get("insight"))
            .ok()
            .flatten()
            .is_some_and(|insight| delivery_text.contains(&insight))
        {
            data.remove("insight");
        }
    }
    result.remove("messages");
    result.remove("has_more");
    result.insert("message".into(), event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEXT: &str =
        "Check the dependency boundary before the local patch.\nKeep the API stable.";
    const INSIGHT: &str = "The boundary matters more than the patch.";
    const QUOTE: &str = "An earlier request that is longer than the eighty characters the delivery header keeps for its quote...";

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object")
    }

    fn exact() -> Map<String, Value> {
        object(json!({"group_id":"g_test","event_id":"e1"}))
    }

    fn chat_event(data: Value) -> Value {
        json!({
            "v":1,"id":"e1","ts":"2026-10-28T01:45:03.676332Z","kind":"chat.message",
            "group_id":"g_test","scope_key":"s_project","by":"connect:inst1","data":data
        })
    }

    fn delivery(body: &str) -> String {
        format!(
            "[CCCC] You are peer1 (peer) in group 'test'\n\n[cccc] remote → peer1 (reply:e0) [event_id=e1 sent=2026-10-28T01:45:03Z]\n> \"An earlier request that is longer than the eighty characters the delivery...\":\n{body}"
        )
    }

    fn result(event: Value, delivery_text: Value) -> Map<String, Value> {
        object(json!({"messages":[event],"has_more":false,"delivery_text":delivery_text}))
    }

    #[test]
    fn exact_chat_read_returns_the_body_and_insight_once_with_all_other_metadata() {
        let data = json!({
            "to":["peer1"],"message_mode":"request_reply","reply_to":"e0","quote_text":QUOTE,
            "text":format!("{TEXT}\n"),"insight":INSIGHT,
            "attachments":[{"path":"state/blobs/a.txt","title":"a.txt","bytes":3}],
            "refs":[{"kind":"task","task_id":"T1"}],
            "src_instance_id":"inst1","src_group_id":"g_remote","src_event_id":"e_remote",
            "source_platform":"cccc_connect","source_user_id":"remote"
        });
        let delivery_text = delivery(&format!(
            "{TEXT}\n\n[cccc] Attachments: ...\n- a.txt (3 bytes) [state/blobs/a.txt]\n\n{}\n{INSIGHT}",
            cccc_core::peer_insight::PEER_PERSPECTIVE_AGENT_LABEL
        ));
        let mut projected = result(chat_event(data.clone()), json!(delivery_text));

        apply("cccc_message_history", &mut projected, &exact());

        let mut expected = chat_event(data);
        let expected_data = expected["data"].as_object_mut().expect("data");
        expected_data.remove("text");
        expected_data.remove("insight");
        assert_eq!(
            Value::Object(projected.clone()),
            json!({"delivery_text":delivery_text,"message":expected})
        );
        let payload = serde_json::to_string(&projected).expect("payload");
        let encoded = |value: &str| serde_json::to_string(value).expect("encoded");
        for value in [TEXT, INSIGHT] {
            let fragment = encoded(value);
            let fragment = fragment.trim_matches('"');
            assert_eq!(payload.matches(fragment).count(), 1, "{value}");
        }
        assert_eq!(projected["message"]["data"]["quote_text"], json!(QUOTE));
    }

    #[test]
    fn fields_missing_from_the_rendered_text_stay_in_the_event() {
        // An /install command renders capability instructions instead of the
        // user's text, and a malformed insight is not rendered at all.
        let data = json!({
            "to":["peer1"],"text":"/install owner/repo","insight":7,
            "refs":[{"title":"slash_command","command":"/install","args_text":"owner/repo"}]
        });
        let mut projected = result(
            chat_event(data.clone()),
            json!(delivery(
                "[cccc] Slash command: /install\n...\n- Raw arguments: owner/repo"
            )),
        );

        apply("cccc_message_history", &mut projected, &exact());

        assert_eq!(projected["message"]["data"], data);
        assert!(!projected.contains_key("messages"));
    }

    #[test]
    fn an_unrenderable_event_is_returned_whole() {
        let event = chat_event(json!({"to":["peer1"],"text":"","insight":INSIGHT}));
        for delivery_text in [Value::Null, json!("")] {
            let mut projected = result(event.clone(), delivery_text.clone());

            apply("cccc_message_history", &mut projected, &exact());

            assert_eq!(
                Value::Object(projected),
                json!({"delivery_text":delivery_text,"message":event})
            );
        }
    }

    #[test]
    fn system_notice_data_is_kept_as_recorded() {
        let mut event = chat_event(json!({"kind":"info","title":"Notice","text":"restart"}));
        event["kind"] = json!("system.notify");
        let mut projected = result(
            event.clone(),
            json!("[cccc] SYSTEM (info) [sent=2026-10-28T01:45:03Z]: Notice\nrestart"),
        );

        apply("cccc_message_history", &mut projected, &exact());

        assert_eq!(projected["message"], event);
    }

    #[test]
    fn lists_and_other_results_are_unchanged() {
        let event = chat_event(json!({"to":["peer1"],"text":TEXT}));
        let list = object(json!({"messages":[event.clone()],"has_more":true}));
        let cases = [
            ("cccc_message_history", list.clone(), Map::new()),
            ("cccc_message_history", list.clone(), exact()),
            (
                "cccc_inbox_read",
                result(event.clone(), json!(TEXT)),
                exact(),
            ),
            (
                "cccc_message_history",
                object(json!({"messages":[event.clone(),event],"delivery_text":TEXT})),
                exact(),
            ),
        ];
        for (tool_name, mut unchanged, arguments) in cases {
            let expected = unchanged.clone();

            apply(tool_name, &mut unchanged, &arguments);

            assert_eq!(unchanged, expected, "{tool_name} {arguments:?}");
        }
    }
}
