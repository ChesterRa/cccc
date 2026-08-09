use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use weixin_agent::{LoginStatus, QrLoginSession, StandaloneQrLogin, WeixinConfig};

#[derive(Default)]
pub(super) struct LoginRegistry {
    attempts: Mutex<HashMap<String, Arc<tokio::sync::Mutex<LoginAttempt>>>>,
}

struct LoginAttempt {
    login: StandaloneQrLogin,
    session: QrLoginSession,
    verify_code: Option<String>,
}

impl LoginRegistry {
    pub(super) async fn start(&self, home: &HomeLayout, group_id: &str) -> Result<Value, String> {
        let config = WeixinConfig::builder()
            .token("")
            .build()
            .map_err(|error| error.to_string())?;
        let login = StandaloneQrLogin::new(&config);
        let local_tokens = stored_token(home, group_id).into_iter().collect::<Vec<_>>();
        let session = login
            .start(None, &local_tokens)
            .await
            .map_err(|error| format!("Weixin QR login failed: {error}"))?;
        let qrcode_url = session.qrcode_img_content.clone();
        self.attempts
            .lock()
            .expect("Weixin login registry poisoned")
            .insert(
                group_id.to_owned(),
                Arc::new(tokio::sync::Mutex::new(LoginAttempt {
                    login,
                    session,
                    verify_code: None,
                })),
            );
        Ok(json!({
            "status":"waiting_scan","logged_in":false,"running":true,
            "qrcode_url":qrcode_url,"pid":std::process::id(),"updated_at":cccc_contracts::utc_now()
        }))
    }

    pub(super) async fn status(&self, home: &HomeLayout, group_id: &str) -> Result<Value, String> {
        let attempt = self
            .attempts
            .lock()
            .expect("Weixin login registry poisoned")
            .get(group_id)
            .cloned();
        let Some(attempt) = attempt else {
            return stored_login(home, group_id);
        };
        let mut attempt_guard = attempt.lock().await;
        let verify_code = attempt_guard.verify_code.take();
        let status = attempt_guard
            .login
            .poll_status(&attempt_guard.session, verify_code.as_deref())
            .await
            .map_err(|error| format!("Weixin QR status failed: {error}"))?;
        match status {
            LoginStatus::Confirmed {
                bot_token,
                ilink_bot_id,
                base_url,
                ilink_user_id,
            } => {
                save_credentials(
                    home,
                    group_id,
                    &bot_token,
                    &ilink_bot_id,
                    &base_url,
                    &ilink_user_id,
                )?;
                super::weixin_authorization::ensure_login_authorized(
                    home,
                    group_id,
                    &ilink_user_id,
                )?;
                if !ensure_stored_login_authorized(home, group_id)? {
                    return Err("Weixin QR login could not authorize the scanned account".into());
                }
                drop(attempt_guard);
                self.remove_attempt(group_id, &attempt);
                Ok(json!({
                    "status":"logged_in","logged_in":true,"running":false,
                    "account_id":ilink_bot_id,"auto_subscribed":true,
                    "pid":null,"updated_at":cccc_contracts::utc_now()
                }))
            }
            LoginStatus::Expired => {
                drop(attempt_guard);
                self.remove_attempt(group_id, &attempt);
                Ok(json!({
                    "status":"expired","logged_in":false,"running":false,"pid":null,
                    "error":"Weixin QR login expired","updated_at":cccc_contracts::utc_now()
                }))
            }
            LoginStatus::VerifyCodeBlocked => {
                drop(attempt_guard);
                self.remove_attempt(group_id, &attempt);
                Ok(json!({
                    "status":"error","logged_in":false,"running":false,"pid":null,
                    "error":"Too many invalid verification codes; regenerate the QR code",
                    "updated_at":cccc_contracts::utc_now()
                }))
            }
            LoginStatus::BindedRedirect => {
                drop(attempt_guard);
                self.remove_attempt(group_id, &attempt);
                Ok(json!({
                    "status":"error","logged_in":false,"running":false,"pid":null,
                    "error":"This Weixin bot is already bound to another login session",
                    "updated_at":cccc_contracts::utc_now()
                }))
            }
            LoginStatus::ScannedButRedirect { redirect_host } => {
                let base_url = normalize_redirect_host(&redirect_host)?;
                let config = WeixinConfig::builder()
                    .token("")
                    .base_url(base_url)
                    .build()
                    .map_err(|error| error.to_string())?;
                attempt_guard.login = StandaloneQrLogin::new(&config);
                let qrcode_url = attempt_guard.session.qrcode_img_content.clone();
                Ok(json!({
                    "status":"scanned","logged_in":false,"running":true,
                    "qrcode_url":qrcode_url,"pid":std::process::id(),"updated_at":cccc_contracts::utc_now()
                }))
            }
            LoginStatus::NeedVerifyCode => active_status(&attempt_guard, "need_verify_code", true),
            LoginStatus::Scanned => active_status(&attempt_guard, "scanned", false),
            LoginStatus::Wait => active_status(&attempt_guard, "waiting_scan", false),
            _ => active_status(&attempt_guard, "waiting_scan", false),
        }
    }

    pub(super) async fn verify(
        &self,
        home: &HomeLayout,
        group_id: &str,
        verify_code: &str,
    ) -> Result<Value, String> {
        let verify_code = verify_code.trim();
        if verify_code.is_empty() || verify_code.chars().any(char::is_whitespace) {
            return Err("Weixin verification code is invalid".into());
        }
        let attempt = self
            .attempts
            .lock()
            .expect("Weixin login registry poisoned")
            .get(group_id)
            .cloned()
            .ok_or_else(|| "No active Weixin QR login session".to_owned())?;
        attempt.lock().await.verify_code = Some(verify_code.to_owned());
        self.status(home, group_id).await
    }

    pub(super) fn clear(&self, group_id: &str) {
        self.attempts
            .lock()
            .expect("Weixin login registry poisoned")
            .remove(group_id);
    }

    fn remove_attempt(&self, group_id: &str, expected: &Arc<tokio::sync::Mutex<LoginAttempt>>) {
        self.attempts
            .lock()
            .expect("Weixin login registry poisoned")
            .retain(|key, attempt| key != group_id || !Arc::ptr_eq(attempt, expected));
    }
}

fn active_status(
    attempt: &LoginAttempt,
    status: &str,
    verification_required: bool,
) -> Result<Value, String> {
    Ok(json!({
        "status":status,"logged_in":false,"running":true,
        "verification_required":verification_required,
        "qrcode_url":attempt.session.qrcode_img_content,
        "pid":std::process::id(),"updated_at":cccc_contracts::utc_now()
    }))
}

fn normalize_redirect_host(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Weixin QR redirect response has no host".into());
    }
    let mut url = if value.starts_with("http://") || value.starts_with("https://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    if !url.ends_with('/') {
        url.push('/');
    }
    Ok(url)
}

fn stored_login(home: &HomeLayout, group_id: &str) -> Result<Value, String> {
    let path = credentials_path(home, group_id);
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok(json!({"status":"idle","logged_in":false,"running":false,"pid":null}));
    };
    let Ok(mut value) = serde_json::from_str::<Value>(&raw) else {
        return Ok(
            json!({"status":"error","logged_in":false,"running":false,"pid":null,"error":"invalid Weixin credentials"}),
        );
    };
    let logged_in = value
        .get("token")
        .and_then(Value::as_str)
        .is_some_and(|token| !token.trim().is_empty());
    let was_auto_subscribed = value["autoSubscribed"].as_bool().unwrap_or(false);
    let auto_subscribed = synchronize_login_authorization(home, group_id, &mut value)?;
    if auto_subscribed != was_auto_subscribed {
        write_credentials(home, group_id, &value)?;
    }
    Ok(json!({
        "status":if logged_in{"logged_in"}else{"idle"},"logged_in":logged_in,
        "account_id":value.get("accountId").and_then(Value::as_str).unwrap_or(""),
        "auto_subscribed":auto_subscribed,
        "running":false,"pid":null,"updated_at":value.get("savedAt").cloned().unwrap_or(Value::Null)
    }))
}

fn save_credentials(
    home: &HomeLayout,
    group_id: &str,
    token: &str,
    account_id: &str,
    base_url: &str,
    user_id: &str,
) -> Result<(), String> {
    if user_id.trim().is_empty() {
        return Err("Weixin QR login returned an empty user id".into());
    }
    let payload = json!({
        "token":token,"accountId":account_id,"baseUrl":base_url,"userId":user_id,
        "savedAt":cccc_contracts::utc_now(),"autoSubscribed":false
    });
    write_credentials(home, group_id, &payload)
}

pub(super) fn ensure_stored_login_authorized(
    home: &HomeLayout,
    group_id: &str,
) -> Result<bool, String> {
    let mut value: Value = serde_json::from_slice(
        &std::fs::read(credentials_path(home, group_id)).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let was_auto_subscribed = value["autoSubscribed"].as_bool().unwrap_or(false);
    let auto_subscribed = synchronize_login_authorization(home, group_id, &mut value)?;
    if auto_subscribed != was_auto_subscribed {
        write_credentials(home, group_id, &value)?;
    }
    Ok(auto_subscribed)
}

fn synchronize_login_authorization(
    home: &HomeLayout,
    group_id: &str,
    credentials: &mut Value,
) -> Result<bool, String> {
    let logged_in = credentials
        .get("token")
        .and_then(Value::as_str)
        .is_some_and(|token| !token.trim().is_empty());
    let Some(user_id) = credentials
        .get("userId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|user_id| !user_id.is_empty())
        .map(str::to_owned)
        .filter(|_| logged_in)
    else {
        return Ok(false);
    };
    let subscribed = match super::weixin_authorization::login_authorization_subscription(
        home, group_id, &user_id,
    )? {
        Some(subscribed) => subscribed,
        None => {
            super::weixin_authorization::ensure_login_authorized(home, group_id, &user_id)?;
            true
        }
    };
    credentials["autoSubscribed"] = Value::Bool(subscribed);
    Ok(subscribed)
}

fn write_credentials(home: &HomeLayout, group_id: &str, payload: &Value) -> Result<(), String> {
    std::fs::write(
        credentials_path(home, group_id),
        serde_json::to_vec_pretty(payload).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn stored_user_id(home: &HomeLayout, group_id: &str) -> Option<String> {
    let value: Value =
        serde_json::from_slice(&std::fs::read(credentials_path(home, group_id)).ok()?).ok()?;
    value
        .get("userId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|user_id| !user_id.is_empty())
        .map(str::to_owned)
}

fn stored_token(home: &HomeLayout, group_id: &str) -> Option<String> {
    let value: Value =
        serde_json::from_slice(&std::fs::read(credentials_path(home, group_id)).ok()?).ok()?;
    value
        .get("token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
}

pub(super) fn remove_credentials(home: &HomeLayout, group_id: &str) -> Result<(), String> {
    match std::fs::remove_file(credentials_path(home, group_id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn credentials_path(home: &HomeLayout, group_id: &str) -> std::path::PathBuf {
    home.groups_dir()
        .join(group_id)
        .join("state/im_weixin_credentials.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::GroupStore;

    #[test]
    fn redirect_hosts_are_normalized_for_follow_up_polling() {
        assert_eq!(
            normalize_redirect_host("redirect.weixin.example").expect("host"),
            "https://redirect.weixin.example/"
        );
        assert_eq!(
            normalize_redirect_host("https://redirect.weixin.example/api/").expect("url"),
            "https://redirect.weixin.example/api/"
        );
        assert!(normalize_redirect_host("  ").is_err());
    }

    #[test]
    fn stored_login_migrates_existing_credentials_to_auto_subscription() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("weixin", "").expect("group");
        write_credentials(
            &home,
            &group.group_id,
            &json!({
                "token":"token","accountId":"bot","baseUrl":"https://example.test",
                "userId":"wx-user","savedAt":"now"
            }),
        )
        .expect("credentials");

        let status = stored_login(&home, &group.group_id).expect("status");

        assert_eq!(status["auto_subscribed"], true);
        let state = cccc_core::integration_state::group_get(&store, &group.group_id, "im_bridge")
            .expect("state");
        assert_eq!(state["authorized"][0]["chat_id"], "wx-user");
        let credentials: Value = serde_json::from_slice(
            &std::fs::read(credentials_path(&home, &group.group_id)).expect("credentials"),
        )
        .expect("json");
        assert_eq!(credentials["autoSubscribed"], true);
    }

    #[test]
    fn stored_login_repairs_missing_authorization_even_when_marked_automatic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("weixin", "").expect("group");
        write_credentials(
            &home,
            &group.group_id,
            &json!({
                "token":"token","accountId":"bot","baseUrl":"https://example.test",
                "userId":"wx-user","savedAt":"now","autoSubscribed":true
            }),
        )
        .expect("credentials");
        cccc_core::integration_state::group_update(&store, &group.group_id, "im_bridge", |state| {
            *state = json!({"authorized":[]});
            Ok(())
        })
        .expect("state");

        let status = stored_login(&home, &group.group_id).expect("status");

        assert_eq!(status["auto_subscribed"], true);
        let state = cccc_core::integration_state::group_get(&store, &group.group_id, "im_bridge")
            .expect("state");
        assert_eq!(state["authorized"][0]["chat_id"], "wx-user");
    }

    #[test]
    fn stored_login_preserves_explicit_pause_and_unsubscribe_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("weixin", "").expect("group");
        write_credentials(
            &home,
            &group.group_id,
            &json!({
                "token":"token","accountId":"bot","baseUrl":"https://example.test",
                "userId":"wx-user","savedAt":"now","autoSubscribed":true
            }),
        )
        .expect("credentials");
        cccc_core::integration_state::group_update(&store, &group.group_id, "im_bridge", |state| {
            *state = json!({"authorized":[{
                "chat_id":"wx-user","platform":"weixin","paused":true,"subscribed":true,
                "authorization_source":"weixin_login"
            }]});
            Ok(())
        })
        .expect("paused state");

        stored_login(&home, &group.group_id).expect("paused status");
        let state = cccc_core::integration_state::group_get(&store, &group.group_id, "im_bridge")
            .expect("state");
        assert_eq!(state["authorized"][0]["paused"], true);
        assert_eq!(state["authorized"][0]["subscribed"], true);

        cccc_core::integration_state::group_update(&store, &group.group_id, "im_bridge", |state| {
            state["authorized"][0]["subscribed"] = Value::Bool(false);
            Ok(())
        })
        .expect("unsubscribed state");
        let status = stored_login(&home, &group.group_id).expect("unsubscribed status");
        let state = cccc_core::integration_state::group_get(&store, &group.group_id, "im_bridge")
            .expect("state");
        assert_eq!(state["authorized"][0]["subscribed"], false);
        assert_eq!(status["auto_subscribed"], false);
        let credentials: Value = serde_json::from_slice(
            &std::fs::read(credentials_path(&home, &group.group_id)).expect("credentials"),
        )
        .expect("json");
        assert_eq!(credentials["autoSubscribed"], false);
    }

    #[tokio::test]
    async fn logout_removes_credentials_authorization_and_running_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("weixin", "").expect("group");
        write_credentials(
            &home,
            &group.group_id,
            &json!({
                "token":"token","accountId":"bot","baseUrl":"https://example.test",
                "userId":"wx-user","savedAt":"now","autoSubscribed":true
            }),
        )
        .expect("credentials");
        super::super::weixin_authorization::ensure_login_authorized(
            &home,
            &group.group_id,
            "wx-user",
        )
        .expect("authorization");
        cccc_core::integration_state::group_update(&store, &group.group_id, "im_bridge", |state| {
            state["enabled"] = json!(true);
            state["running"] = json!(true);
            state["adapter_available"] = json!(true);
            state["pid"] = json!(1234);
            Ok(())
        })
        .expect("running state");
        let registry = super::super::ImWorkerRegistry::new(
            crate::ledger_event_hub::LedgerEventHub::new(home.clone()),
        );

        registry
            .logout_weixin(&home, &group.group_id)
            .await
            .expect("logout");

        assert!(!credentials_path(&home, &group.group_id).exists());
        let state = cccc_core::integration_state::group_get(&store, &group.group_id, "im_bridge")
            .expect("state");
        assert_eq!(state["enabled"], false);
        assert_eq!(state["running"], false);
        assert_eq!(state["adapter_available"], false);
        assert!(state["pid"].is_null());
        assert!(
            state["authorized"]
                .as_array()
                .expect("authorized")
                .is_empty()
        );
    }
}
