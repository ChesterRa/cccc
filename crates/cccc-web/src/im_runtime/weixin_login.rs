use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use weixin_agent::{LoginStatus, QrLoginSession, StandaloneQrLogin, WeixinConfig};

#[derive(Default)]
pub(super) struct LoginRegistry {
    attempts: Mutex<HashMap<String, LoginAttempt>>,
}

struct LoginAttempt {
    login: StandaloneQrLogin,
    session: QrLoginSession,
}

impl LoginRegistry {
    pub(super) async fn start(&self, group_id: &str) -> Result<Value, String> {
        let config = WeixinConfig::builder()
            .token("")
            .build()
            .map_err(|error| error.to_string())?;
        let login = StandaloneQrLogin::new(&config);
        let session = login
            .start(None, &[])
            .await
            .map_err(|error| format!("Weixin QR login failed: {error}"))?;
        let qrcode_url = session.qrcode_img_content.clone();
        self.attempts
            .lock()
            .expect("Weixin login registry poisoned")
            .insert(group_id.to_owned(), LoginAttempt { login, session });
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
            .remove(group_id);
        let Some(attempt) = attempt else {
            return stored_login(home, group_id);
        };
        let status = attempt
            .login
            .poll_status(&attempt.session, None)
            .await
            .map_err(|error| format!("Weixin QR status failed: {error}"))?;
        match status {
            LoginStatus::Confirmed {
                bot_token,
                ilink_bot_id,
                base_url,
                ilink_user_id,
            } => {
                super::weixin_authorization::ensure_login_authorized(
                    home,
                    group_id,
                    &ilink_user_id,
                )?;
                save_credentials(
                    home,
                    group_id,
                    &bot_token,
                    &ilink_bot_id,
                    &base_url,
                    &ilink_user_id,
                )?;
                Ok(json!({
                    "status":"logged_in","logged_in":true,"running":false,
                    "account_id":ilink_bot_id,"auto_subscribed":true,
                    "pid":null,"updated_at":cccc_contracts::utc_now()
                }))
            }
            LoginStatus::Expired | LoginStatus::VerifyCodeBlocked | LoginStatus::BindedRedirect => {
                Ok(json!({
                    "status":"expired","logged_in":false,"running":false,"pid":null,
                    "error":"Weixin QR login expired","updated_at":cccc_contracts::utc_now()
                }))
            }
            other => {
                let status = match other {
                    LoginStatus::Scanned | LoginStatus::ScannedButRedirect { .. } => "scanned",
                    LoginStatus::NeedVerifyCode => "need_verify_code",
                    _ => "waiting_scan",
                };
                let qrcode_url = attempt.session.qrcode_img_content.clone();
                self.attempts
                    .lock()
                    .expect("Weixin login registry poisoned")
                    .insert(group_id.to_owned(), attempt);
                Ok(json!({
                    "status":status,"logged_in":false,"running":true,
                    "qrcode_url":qrcode_url,"pid":std::process::id(),"updated_at":cccc_contracts::utc_now()
                }))
            }
        }
    }

    pub(super) fn clear(&self, group_id: &str) {
        self.attempts
            .lock()
            .expect("Weixin login registry poisoned")
            .remove(group_id);
    }
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
    if logged_in
        && !value["autoSubscribed"].as_bool().unwrap_or(false)
        && let Some(user_id) = value.get("userId").and_then(Value::as_str)
    {
        super::weixin_authorization::ensure_login_authorized(home, group_id, user_id)?;
        value["autoSubscribed"] = Value::Bool(true);
        write_credentials(home, group_id, &value)?;
    }
    Ok(json!({
        "status":if logged_in{"logged_in"}else{"idle"},"logged_in":logged_in,
        "account_id":value.get("accountId").and_then(Value::as_str).unwrap_or(""),
        "auto_subscribed":value["autoSubscribed"].as_bool().unwrap_or(false),
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
    let payload = json!({
        "token":token,"accountId":account_id,"baseUrl":base_url,"userId":user_id,
        "savedAt":cccc_contracts::utc_now(),"autoSubscribed":true
    });
    write_credentials(home, group_id, &payload)
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

pub(super) fn remove_credentials(home: &HomeLayout, group_id: &str) {
    let _ = std::fs::remove_file(credentials_path(home, group_id));
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
}
