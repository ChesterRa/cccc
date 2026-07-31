use serde_json::{Map, Value, json};

pub(super) fn upsert_authorized(state: &mut Map<String, Value>, mut item: Value) -> Value {
    item["authorized_at"] = json!(epoch_seconds());
    let chat_id = item["chat_id"].as_str().unwrap_or("").to_owned();
    let thread_id = item["thread_id"].as_i64().unwrap_or(0);
    let authorized = array_mut(state, "authorized");
    authorized.retain(|existing| !same_target(existing, &chat_id, thread_id));
    authorized.push(item.clone());
    item
}

pub(super) fn revoke(
    state: &mut Map<String, Value>,
    chat_id: &str,
    thread_id: i64,
) -> (bool, bool) {
    let mut changed = [false, false];
    for (index, key) in ["authorized", "subscribers"].into_iter().enumerate() {
        let items = array_mut(state, key);
        let before = items.len();
        items.retain(|item| !same_target(item, chat_id, thread_id));
        changed[index] = items.len() != before;
    }
    (changed[0], changed[1])
}

pub(super) fn set_verbose(
    state: &mut Map<String, Value>,
    chat_id: &str,
    thread_id: i64,
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
        let thread_id = item["thread_id"].as_i64().unwrap_or(0);
        if let Some(subscriber) = subscribers
            .iter()
            .find(|candidate| same_target(candidate, chat_id, thread_id))
        {
            item["verbose"] = json!(subscriber["verbose"].as_bool().unwrap_or(false));
            item["subscribed"] = json!(subscriber["subscribed"].as_bool().unwrap_or(true));
        }
    }
}

fn same_target(item: &Value, chat_id: &str, thread_id: i64) -> bool {
    item["chat_id"].as_str() == Some(chat_id)
        && item["thread_id"].as_i64().unwrap_or(0) == thread_id
}

fn array_mut<'a>(state: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    let value = state.entry(key).or_insert_with(|| json!([]));
    if !value.is_array() {
        *value = json!([]);
    }
    value.as_array_mut().expect("array initialized")
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
        let changed = revoke(state.as_object_mut().expect("state"), "same", 0);
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
}
