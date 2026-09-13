use serde_json::{Map, Value, json};

pub(crate) fn apply_cross_group_default(args: &mut Map<String, Value>) -> Result<(), String> {
    let cross_group = args.contains_key("dst_instance_id")
        || match (text(args, "group_id"), text(args, "dst_group_id")) {
            (Some(source), Some(destination)) => source != destination,
            _ => false,
        };
    if !cross_group {
        return Ok(());
    }
    if args.contains_key("to") {
        let valid = args.get("to").is_some_and(|value| match value {
            Value::String(value) => !value.trim().is_empty(),
            Value::Array(values) => {
                !values.is_empty()
                    && values
                        .iter()
                        .all(|value| value.as_str().is_some_and(|value| !value.trim().is_empty()))
            }
            _ => false,
        });
        if !valid {
            return Err(
                "invalid_recipient: cross-group to must be a non-empty string or string array"
                    .into(),
            );
        }
    } else {
        args.insert(
            "to".into(),
            json!([cccc_core::actors::CROSS_GROUP_FOREMAN_RECIPIENT]),
        );
    }
    Ok(())
}

fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cross_group_default_intent_only_applies_when_recipient_is_omitted() {
        let mut omitted = json!({"group_id":"g_local","dst_group_id":"g_remote"})
            .as_object()
            .cloned()
            .expect("omitted args");
        apply_cross_group_default(&mut omitted).expect("default");
        assert_eq!(
            omitted["to"],
            json!([cccc_core::actors::CROSS_GROUP_FOREMAN_RECIPIENT])
        );

        let mut explicit = json!({
            "group_id":"g_local","dst_group_id":"g_remote","to":["peer"]
        })
        .as_object()
        .cloned()
        .expect("explicit args");
        apply_cross_group_default(&mut explicit).expect("explicit");
        assert_eq!(explicit["to"], json!(["peer"]));

        let mut reply = json!({
            "group_id":"g_local","dst_group_id":"g_remote",
            "reply_to":"remote-event","to":["@peer"]
        })
        .as_object()
        .cloned()
        .expect("reply args");
        apply_cross_group_default(&mut reply).expect("cross-group reply");
        assert_eq!(reply["to"], json!(["@peer"]));
        assert_eq!(reply["reply_to"], "remote-event");

        let mut local = json!({"group_id":"g_local","dst_group_id":"g_local"})
            .as_object()
            .cloned()
            .expect("local args");
        apply_cross_group_default(&mut local).expect("local");
        assert!(local.get("to").is_none());

        for invalid in [json!(null), json!([]), json!([" "]), json!(7)] {
            let mut args = json!({
                "group_id":"g_local","dst_group_id":"g_remote","to":invalid
            })
            .as_object()
            .cloned()
            .expect("invalid args");
            assert!(apply_cross_group_default(&mut args).is_err());
        }
    }
}
