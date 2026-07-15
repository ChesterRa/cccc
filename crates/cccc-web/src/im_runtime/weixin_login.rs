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
            return Ok(stored_login(home, group_id));
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
                    "account_id":ilink_bot_id,"pid":null,"updated_at":cccc_contracts::utc_now()
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

fn stored_login(home: &HomeLayout, group_id: &str) -> Value {
    let path = credentials_path(home, group_id);
    let Ok(raw) = std::fs::read_to_string(path) else {
        return json!({"status":"idle","logged_in":false,"running":false,"pid":null});
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return json!({"status":"error","logged_in":false,"running":false,"pid":null,"error":"invalid Weixin credentials"});
    };
    let logged_in = value
        .get("token")
        .and_then(Value::as_str)
        .is_some_and(|token| !token.trim().is_empty());
    json!({
        "status":if logged_in{"logged_in"}else{"idle"},"logged_in":logged_in,
        "account_id":value.get("accountId").and_then(Value::as_str).unwrap_or(""),
        "running":false,"pid":null,"updated_at":value.get("savedAt").cloned().unwrap_or(Value::Null)
    })
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
        "savedAt":cccc_contracts::utc_now()
    });
    std::fs::write(
        credentials_path(home, group_id),
        serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn remove_credentials(home: &HomeLayout, group_id: &str) {
    let _ = std::fs::remove_file(credentials_path(home, group_id));
}

fn credentials_path(home: &HomeLayout, group_id: &str) -> std::path::PathBuf {
    home.groups_dir()
        .join(group_id)
        .join("state/im_weixin_credentials.json")
}
