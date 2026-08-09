use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

const PLATFORM: &str = "weixin";
const SOURCE: &str = "weixin_login";

pub(super) fn ensure_login_authorized(
    home: &HomeLayout,
    group_id: &str,
    user_id: &str,
) -> Result<(), String> {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return Err("Weixin QR login returned an empty user id".into());
    }
    let store = GroupStore::new(home.clone()).map_err(|error| error.to_string())?;
    cccc_core::integration_state::group_update(&store, group_id, "im_bridge", |value| {
        if !value.is_object() {
            *value = json!({});
        }
        let state = value.as_object_mut().expect("IM state initialized");
        let entry = json!({
            "chat_id": user_id,
            "chat_title": user_id,
            "chat_type": "p2p",
            "platform": PLATFORM,
            "thread_id": 0,
            "paused": false,
            "verbose": false,
            "authorized_at": epoch_seconds(),
            "authorization_source": SOURCE,
        });
        upsert_authorized(state, user_id, entry);
        remove_matching(state.get_mut("pending"), user_id, false);
        Ok(())
    })
    .map_err(|error| error.to_string())
}

pub(super) fn revoke_login_authorization(
    home: &HomeLayout,
    group_id: &str,
    user_id: &str,
) -> Result<(), String> {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return Ok(());
    }
    let store = GroupStore::new(home.clone()).map_err(|error| error.to_string())?;
    cccc_core::integration_state::group_update(&store, group_id, "im_bridge", |value| {
        let Some(state) = value.as_object_mut() else {
            return Ok(());
        };
        remove_matching(state.get_mut("authorized"), user_id, true);
        Ok(())
    })
    .map_err(|error| error.to_string())
}

pub(super) fn login_authorization_subscription(
    home: &HomeLayout,
    group_id: &str,
    user_id: &str,
) -> Result<Option<bool>, String> {
    let store = GroupStore::new(home.clone()).map_err(|error| error.to_string())?;
    let state = cccc_core::integration_state::group_get(&store, group_id, "im_bridge")
        .map_err(|error| error.to_string())?;
    let matching = match state.get("authorized") {
        Some(Value::Array(items)) => items.iter().find(|item| matches_user(item, user_id)),
        Some(Value::Object(items)) => items.values().find(|item| matches_user(item, user_id)),
        _ => None,
    };
    Ok(matching.map(|item| item["subscribed"].as_bool().unwrap_or(true)))
}

fn upsert_authorized(state: &mut Map<String, Value>, user_id: &str, entry: Value) {
    let authorized = state
        .entry("authorized".to_owned())
        .or_insert_with(|| json!([]));
    match authorized {
        Value::Array(items) => {
            if let Some(item) = items.iter_mut().find(|item| matches_user(item, user_id)) {
                activate(item);
            } else {
                items.push(entry);
            }
        }
        Value::Object(items) => {
            if let Some(item) = items.values_mut().find(|item| matches_user(item, user_id)) {
                activate(item);
            } else {
                items.insert(format!("{PLATFORM}:{user_id}"), entry);
            }
        }
        _ => *authorized = Value::Array(vec![entry]),
    }
}

fn activate(item: &mut Value) {
    let Some(item) = item.as_object_mut() else {
        return;
    };
    item.insert("paused".into(), Value::Bool(false));
    item.insert("subscribed".into(), Value::Bool(true));
}

fn remove_matching(value: Option<&mut Value>, user_id: &str, auto_only: bool) {
    let matches = |item: &Value| {
        matches_user(item, user_id)
            && (!auto_only || item["authorization_source"].as_str() == Some(SOURCE))
    };
    match value {
        Some(Value::Array(items)) => items.retain(|item| !matches(item)),
        Some(Value::Object(items)) => items.retain(|_, item| !matches(item)),
        _ => {}
    }
}

fn matches_user(item: &Value, user_id: &str) -> bool {
    item["chat_id"].as_str() == Some(user_id)
        && item["platform"]
            .as_str()
            .is_none_or(|platform| platform == PLATFORM)
}

fn epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, HomeLayout, String) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("weixin", "")
            .expect("group");
        (temp, home, group.group_id)
    }

    #[test]
    fn qr_login_authorization_is_idempotent_and_clears_pending() {
        let (_temp, home, group_id) = setup();
        let store = GroupStore::new(home.clone()).expect("store");
        cccc_core::integration_state::group_update(&store, &group_id, "im_bridge", |value| {
            *value = json!({"pending":[{"chat_id":"wx-user","platform":"weixin"}]});
            Ok(())
        })
        .expect("state");

        ensure_login_authorized(&home, &group_id, "wx-user").expect("authorize");
        ensure_login_authorized(&home, &group_id, "wx-user").expect("idempotent");

        let state =
            cccc_core::integration_state::group_get(&store, &group_id, "im_bridge").expect("state");
        assert_eq!(state["authorized"].as_array().expect("authorized").len(), 1);
        assert_eq!(state["authorized"][0]["authorization_source"], SOURCE);
        assert!(state["pending"].as_array().expect("pending").is_empty());
    }

    #[test]
    fn qr_login_reactivates_an_existing_authorization() {
        let (_temp, home, group_id) = setup();
        let store = GroupStore::new(home.clone()).expect("store");
        cccc_core::integration_state::group_update(&store, &group_id, "im_bridge", |value| {
            *value = json!({"authorized":[{
                "chat_id":"wx-user","platform":"weixin","paused":true,
                "subscribed":false,"verbose":true,"authorization_source":"weixin_login"
            }]});
            Ok(())
        })
        .expect("state");

        ensure_login_authorized(&home, &group_id, "wx-user").expect("authorize");

        let state =
            cccc_core::integration_state::group_get(&store, &group_id, "im_bridge").expect("state");
        let authorized = state["authorized"].as_array().expect("authorized");
        assert_eq!(authorized.len(), 1);
        assert_eq!(authorized[0]["paused"], false);
        assert_eq!(authorized[0]["subscribed"], true);
        assert_eq!(authorized[0]["verbose"], true);
    }

    #[test]
    fn logout_removes_only_qr_login_authorization() {
        let (_temp, home, group_id) = setup();
        let store = GroupStore::new(home.clone()).expect("store");
        cccc_core::integration_state::group_update(&store, &group_id, "im_bridge", |value| {
            *value = json!({"authorized":[
                {"chat_id":"auto","platform":"weixin","authorization_source":"weixin_login"},
                {"chat_id":"manual","platform":"weixin"}
            ]});
            Ok(())
        })
        .expect("state");

        revoke_login_authorization(&home, &group_id, "auto").expect("revoke");

        let state =
            cccc_core::integration_state::group_get(&store, &group_id, "im_bridge").expect("state");
        assert_eq!(state["authorized"].as_array().expect("authorized").len(), 1);
        assert_eq!(state["authorized"][0]["chat_id"], "manual");
    }
}
