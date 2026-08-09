use serde_json::{Map, Value, json};

pub(super) fn upsert_authorized(state: &mut Map<String, Value>, mut item: Value) -> Value {
    item["authorized_at"] = json!(epoch_seconds());
    let chat_id = item["chat_id"].as_str().unwrap_or("").to_owned();
    let thread_id = thread_id(&item);
    let authorized = array_mut(state, "authorized");
    authorized.retain(|existing| !same_target(existing, &chat_id, &thread_id));
    authorized.push(item.clone());
    item
}

pub(super) fn revoke(
    state: &mut Map<String, Value>,
    chat_id: &str,
    thread_id: &str,
) -> (bool, bool) {
    let mut changed = [false, false];
    for (index, key) in ["authorized", "subscribers"].into_iter().enumerate() {
        let items = array_mut(state, key);
        changed[index] = revoke_items(items, chat_id, thread_id);
    }
    (changed[0], changed[1])
}

pub(super) fn retain_active(items: &mut Vec<Value>) {
    items.retain(|item| item["subscribed"].as_bool().unwrap_or(true));
}

pub(super) fn set_verbose(
    state: &mut Map<String, Value>,
    chat_id: &str,
    thread_id: &str,
    verbose: bool,
) -> Option<Value> {
    let mut result = None;
    for key in ["authorized", "subscribers"] {
        for item in array_mut(state, key) {
            if same_target(item, chat_id, thread_id) {
                item["verbose"] = Value::Bool(verbose);
                result.get_or_insert_with(|| item.clone());
            }
        }
    }
    result
}

pub(super) fn enrich_verbose(authorized: &mut [Value], subscribers: &[Value]) {
    for item in authorized {
        let chat_id = item["chat_id"].as_str().unwrap_or("");
        let thread_id = thread_id(item);
        if let Some(subscriber) = subscribers
            .iter()
            .find(|candidate| same_target(candidate, chat_id, &thread_id))
        {
            item["verbose"] = json!(subscriber["verbose"].as_bool().unwrap_or(false));
            item["subscribed"] = json!(subscriber["subscribed"].as_bool().unwrap_or(true));
        }
    }
}

fn same_target(item: &Value, chat_id: &str, thread_id: &str) -> bool {
    item["chat_id"].as_str() == Some(chat_id)
        && self::thread_id(item) == normalize_thread_id(thread_id)
}

fn revoke_items(items: &mut Vec<Value>, chat_id: &str, thread_id: &str) -> bool {
    let mut changed = false;
    items.retain_mut(|item| {
        if !same_target(item, chat_id, thread_id) {
            return true;
        }
        if item["platform"]
            .as_str()
            .is_some_and(|platform| platform.eq_ignore_ascii_case("weixin"))
        {
            changed |= item["subscribed"].as_bool() != Some(false);
            item["subscribed"] = Value::Bool(false);
            true
        } else {
            changed = true;
            false
        }
    });
    changed
}

fn thread_id(item: &Value) -> String {
    match item.get("thread_id") {
        Some(Value::String(value)) => normalize_thread_id(value),
        Some(Value::Number(value)) => normalize_thread_id(&value.to_string()),
        _ => String::new(),
    }
}

fn normalize_thread_id(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value == "0" {
        String::new()
    } else {
        value.to_owned()
    }
}

fn array_mut<'a>(state: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    let value = state.entry(key).or_insert_with(|| json!([]));
    if let Value::Object(items) = value {
        *value = Value::Array(
            items
                .iter()
                .map(|(object_key, item)| enrich_object_item(object_key, item.clone()))
                .collect(),
        );
    } else if !value.is_array() {
        *value = json!([]);
    }
    value.as_array_mut().expect("array initialized")
}

fn enrich_object_item(object_key: &str, mut item: Value) -> Value {
    let Some(fields) = item.as_object_mut() else {
        return item;
    };
    let (chat_id, thread_id) = object_key
        .rsplit_once(':')
        .filter(|(chat_id, thread_id)| !chat_id.is_empty() && !thread_id.is_empty())
        .map_or((object_key, Value::from(0)), |(chat_id, thread_id)| {
            (chat_id, Value::from(thread_id))
        });
    fields.entry("chat_id").or_insert_with(|| json!(chat_id));
    fields.entry("thread_id").or_insert(thread_id);
    item
}

fn epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoke_uses_python_compatible_chat_and_thread_identity() {
        let mut state = json!({
            "authorized":[
                {"chat_id":"same","platform":"telegram"},
                {"chat_id":"same","platform":"dingtalk"}
            ],
            "subscribers":[
                {"chat_id":"same","platform":"telegram","subscribed":true},
                {"chat_id":"same","platform":"dingtalk","subscribed":true}
            ]
        });
        let changed = revoke(state.as_object_mut().expect("state"), "same", "");
        assert_eq!(changed, (true, true));
        assert!(state["authorized"].as_array().expect("items").is_empty());
        assert!(state["subscribers"].as_array().expect("items").is_empty());
    }

    #[test]
    fn upsert_replaces_the_same_chat_and_thread_across_platforms() {
        let mut state = json!({
            "authorized":[{"chat_id":"same","thread_id":0,"platform":"telegram"}]
        });
        upsert_authorized(
            state.as_object_mut().expect("state"),
            json!({"chat_id":"same","thread_id":0,"platform":"weixin"}),
        );
        let authorized = state["authorized"].as_array().expect("items");
        assert_eq!(authorized.len(), 1);
        assert_eq!(authorized[0]["platform"], "weixin");
    }

    #[test]
    fn mutations_preserve_targets_encoded_only_in_legacy_object_keys() {
        let mut state = json!({
            "authorized":{"chat-old:42":{"platform":"telegram"}},
            "subscribers":{"chat-old:42":{"subscribed":true,"verbose":true}}
        });

        set_verbose(
            state.as_object_mut().expect("state"),
            "chat-old",
            "42",
            false,
        )
        .expect("target");

        assert_eq!(state["authorized"][0]["chat_id"], "chat-old");
        assert_eq!(state["authorized"][0]["thread_id"], "42");
        assert_eq!(state["subscribers"][0]["chat_id"], "chat-old");
        assert_eq!(state["subscribers"][0]["thread_id"], "42");
        assert_eq!(state["subscribers"][0]["verbose"], false);
    }

    #[test]
    fn weixin_revoke_keeps_an_inactive_recovery_tombstone() {
        let mut state = json!({
            "authorized":[{
                "chat_id":"wx-user","platform":"weixin","subscribed":true,
                "authorization_source":"weixin_login"
            }]
        });

        assert_eq!(
            revoke(state.as_object_mut().expect("state"), "wx-user", ""),
            (true, false)
        );
        let authorized = state["authorized"].as_array().expect("authorized");
        assert_eq!(authorized.len(), 1);
        assert_eq!(authorized[0]["subscribed"], false);

        let mut visible = authorized.clone();
        retain_active(&mut visible);
        assert!(visible.is_empty());
    }
}
