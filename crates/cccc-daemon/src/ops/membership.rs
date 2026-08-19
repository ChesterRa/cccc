use cccc_contracts::DaemonRequest;
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{HomeLayout, cloudflared, fs, membership, settings, web_model_connectors};
use chrono::{DateTime, SecondsFormat, Utc};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value, json};

use super::membership_account::{AccountClient, AccountError};
use super::membership_cloudflared::{self, RuntimeError};
use crate::dispatch::{OpError, OpResult, bool_arg, object, string_arg};

struct PublicUrls {
    hostname: Option<String>,
    web: Option<String>,
    connector: Option<String>,
}

const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "membership_status" => status(home, request),
        "membership_login" => login(home, request),
        "membership_login_poll" => login_poll(home, request),
        "membership_logout" => logout(home, request),
        "membership_reach_install" => reach_install(home, request),
        "membership_reach_on" => reach_on(home, request),
        "membership_reach_off" => reach_off(home, request),
        _ => return None,
    })
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    refresh_cut_from_account(home);
    object(status_payload(home)?)
}

fn login(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let existing = membership::load(home).map_err(OpError::io)?;
    if existing.logged_in && existing.device_token.is_some() {
        return object(status_payload(home)?);
    }
    let origin = requested_account_origin(request).map_err(|error| account_fail(home, error))?;
    let client = AccountClient::new(&origin).map_err(|error| account_fail(home, error))?;
    let started = client
        .start_device_login()
        .map_err(|error| account_fail(home, error))?;
    let expires_at = Utc::now() + chrono::Duration::seconds(started.expires_in as i64);
    membership::update(home, |state| {
        state.account_origin = Some(origin.clone());
        state.pending_login = Some(json!({
            "device_code":started.device_code,
            "user_code":started.user_code,
            "verification_uri":started.verification_uri,
            "interval":started.interval,
            "expires_at":expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            "account_origin":origin,
        }));
        state.last_error = None;
        Ok(())
    })
    .map_err(OpError::io)?;
    object(status_payload(home)?)
}

fn login_poll(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let state = membership::load(home).map_err(OpError::io)?;
    let pending = state.pending_login.as_ref().and_then(Value::as_object);
    if pending.is_none_or(pending_expired) {
        return fail(
            home,
            "membership_network",
            "device code expired; run `cccc login` again",
        );
    }
    let pending = pending.expect("pending login was checked");
    let device_code = text(pending, "device_code", "");
    let origin = bound_account_origin(&state).map_err(|error| account_fail(home, error))?;
    let client = AccountClient::new(&origin).map_err(|error| account_fail(home, error))?;
    match client.poll_device_login(&device_code) {
        Ok(grant) => {
            membership::update(home, |state| {
                state.logged_in = true;
                state.account_origin = Some(origin.clone());
                state.device_id = Some(grant.device_id);
                state.device_token = Some(grant.device_token);
                if grant.hostname.is_some() {
                    state.hostname = grant.hostname;
                }
                state.pending_login = None;
                state.disabled = false;
                state.last_error = None;
                Ok(())
            })
            .map_err(OpError::io)?;
        }
        Err(error) if error.retryable => {
            if error.retry_after_delta > 0 {
                membership::update(home, |state| {
                    if let Some(pending) =
                        state.pending_login.as_mut().and_then(Value::as_object_mut)
                    {
                        let interval = integer(pending, "interval", 5).max(1);
                        pending.insert(
                            "interval".into(),
                            Value::from(interval + error.retry_after_delta),
                        );
                    }
                    state.last_error = None;
                    Ok(())
                })
                .map_err(OpError::io)?;
            }
        }
        Err(error) => return Err(account_fail(home, error)),
    }
    object(status_payload(home)?)
}

fn logout(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let state = membership::load(home).map_err(OpError::io)?;
    let remote = settings::load(home).map_err(OpError::io)?.remote_access;
    membership_cloudflared::stop(home).map_err(runtime_error)?;
    let remote_url = text(&remote, "web_public_url", "");
    let retires_reach = text(&remote, "provider", "off") == "reach"
        || state.hostname.as_deref().is_some_and(|hostname| {
            !remote_url.is_empty()
                && hostname.trim_end_matches('/') == remote_url.trim_end_matches('/')
        });
    if retires_reach {
        settings::update(home, |global| {
            global
                .remote_access
                .insert("enabled".into(), Value::Bool(false));
            global
                .remote_access
                .insert("web_public_url".into(), Value::String(String::new()));
            global.remote_access.insert(
                "updated_at".into(),
                Value::String(cccc_contracts::utc_now()),
            );
            Ok(())
        })
        .map_err(OpError::io)?;
    }
    membership::clear(home).map_err(OpError::io)?;
    let mut payload = status_payload(home)?;
    payload["membership"]["warning"] = Value::String(membership::LOGOUT_WARNING.into());
    object(payload)
}

fn reach_install(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    membership_cloudflared::ensure(home, bool_arg(request, "upgrade", false))
        .map_err(|error| remember_runtime_error(home, error))?;
    object(status_payload(home)?)
}

pub(super) fn reach_on(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    reach_on_with(
        home,
        request,
        live_web_port,
        |home| membership_cloudflared::ensure(home, false).map(|_| ()),
        |home, token| membership_cloudflared::start(home, token).map(|_| ()),
    )
}

fn reach_on_with(
    home: &HomeLayout,
    request: &DaemonRequest,
    resolve_origin_port: impl FnOnce(&HomeLayout) -> Result<u16, OpError>,
    ensure_helper: impl FnOnce(&HomeLayout) -> Result<(), RuntimeError>,
    start_helper: impl FnOnce(&HomeLayout, &str) -> Result<(), RuntimeError>,
) -> OpResult {
    require_user(request)?;
    if environment_flag("CCCC_WEB_ALLOW_UNAUTHENTICATED") {
        return fail(
            home,
            "membership_gate",
            "CCCC_WEB_ALLOW_UNAUTHENTICATED is incompatible with reach",
        );
    }
    if admin_token_count(home) == 0 {
        return fail(
            home,
            "membership_gate",
            "an administrator access token is required before reach can start",
        );
    }
    let remote = settings::load(home).map_err(OpError::io)?.remote_access;
    let provider = text(&remote, "provider", "off");
    let enabled = boolean(&remote, "enabled", false);
    if matches!(provider.as_str(), "manual" | "tailscale") && enabled {
        return fail(
            home,
            "membership_gate",
            &format!(
                "remote access is already using {provider}; turn it off before `cccc reach on`"
            ),
        );
    }
    let state = membership::load(home).map_err(OpError::io)?;
    let Some(device_token) = state.device_token.filter(|token| !token.is_empty()) else {
        return fail(
            home,
            "membership_not_logged_in",
            "not logged in; run `cccc login`",
        );
    };
    if !state.logged_in {
        return fail(
            home,
            "membership_not_logged_in",
            "not logged in; run `cccc login`",
        );
    }
    refresh_cut_from_account(home);
    let state = membership::load(home).map_err(OpError::io)?;
    if state.disabled {
        return fail(home, "membership_disabled", "this device has been disabled");
    }
    let origin = bound_account_origin(&state).map_err(|error| account_fail(home, error))?;
    let client = AccountClient::new(&origin).map_err(|error| account_fail(home, error))?;
    ensure_helper(home).map_err(|error| remember_runtime_error(home, error))?;
    let origin_port = resolve_origin_port(home)?;
    let credentials = match client.issue_reach(&device_token, origin_port) {
        Ok(credentials) => credentials,
        Err(error) if error.code == "membership_disabled" => {
            mark_cut(home, None, None)?;
            return Err(account_fail(home, error));
        }
        Err(error) => return Err(account_fail(home, error)),
    };
    membership::update(home, |state| {
        state.hostname = Some(credentials.hostname.clone());
        state.tunnel_token = Some(credentials.tunnel_token.clone());
        state.last_error = None;
        Ok(())
    })
    .map_err(OpError::io)?;
    start_helper(home, &credentials.tunnel_token)
        .map_err(|error| remember_runtime_error(home, error))?;
    let settings_result = settings::update(home, |global| {
        global
            .remote_access
            .insert("provider".into(), Value::String("reach".into()));
        global
            .remote_access
            .insert("enabled".into(), Value::Bool(true));
        global
            .remote_access
            .insert("require_access_token".into(), Value::Bool(true));
        global.remote_access.insert(
            "web_public_url".into(),
            Value::String(credentials.hostname.clone()),
        );
        global.remote_access.insert(
            "updated_at".into(),
            Value::String(cccc_contracts::utc_now()),
        );
        Ok(())
    });
    if let Err(error) = settings_result {
        if let Err(stop_error) = membership_cloudflared::stop(home) {
            let _ = remember_error(
                home,
                &format!(
                    "failed to persist reach state and failed to stop cloudflared: {}",
                    stop_error.message
                ),
            );
        }
        return Err(OpError::io(error));
    }
    object(status_payload(home)?)
}

pub(super) fn reach_off(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let remote = settings::load(home).map_err(OpError::io)?.remote_access;
    if text(&remote, "provider", "off") != "reach" {
        return Err(OpError::new(
            "membership_not_in_reach",
            "reach is not the active remote access provider",
        ));
    }
    membership_cloudflared::stop(home).map_err(runtime_error)?;
    if boolean(&remote, "enabled", false) {
        settings::update(home, |global| {
            global
                .remote_access
                .insert("enabled".into(), Value::Bool(false));
            global.remote_access.insert(
                "updated_at".into(),
                Value::String(cccc_contracts::utc_now()),
            );
            Ok(())
        })
        .map_err(OpError::io)?;
    }
    object(status_payload(home)?)
}

fn status_payload(home: &HomeLayout) -> Result<Value, OpError> {
    let state = membership::load(home).map_err(OpError::io)?;
    let remote = settings::load(home).map_err(OpError::io)?.remote_access;
    let provider = text(&remote, "provider", "off");
    let helper = membership_cloudflared::status(home);
    let installed = cloudflared::inspect(home).map_err(OpError::io)?;
    let url_source = state.logged_in.then(|| {
        state
            .hostname
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| text(&remote, "web_public_url", ""))
    });
    let urls = public_urls(home, url_source.as_deref())?;
    let cut = state.disabled;
    let pending = state
        .pending_login
        .as_ref()
        .and_then(Value::as_object)
        .filter(|pending| !pending_expired(pending));
    let mut body = json!({
        "logged_in":state.logged_in,
        "device_id":state.device_id,
        "hostname":urls.hostname,
        "web_url":urls.web,
        "connector_url":urls.connector,
        "online":provider == "reach" && boolean(&remote, "enabled", false) && helper.running && !cut,
        "cut":cut,
        "disabled":cut,
        "in_reach":provider == "reach",
        "account_origin":bound_account_origin(&state).ok().or_else(|| {
            (!state.logged_in).then(membership::account_origin).flatten()
        }),
        "last_error":state.last_error,
        "cloudflared":{
            "installed":installed.installed,
            "matches_pin":installed.matches_pin,
            "version":installed.version,
            "pinned_version":installed.pinned_version,
            "running":helper.running,
        }
    });
    if !state.logged_in
        && let Some(pending) = pending
    {
        body["pending"] = json!({
            "user_code":pending.get("user_code").cloned().unwrap_or(Value::Null),
            "verification_uri":pending.get("verification_uri").cloned().unwrap_or(Value::Null),
            "interval":pending.get("interval").cloned().unwrap_or(Value::Null),
            "expires_at":pending.get("expires_at").cloned().unwrap_or(Value::Null),
        });
    }
    Ok(json!({"membership":body}))
}

fn refresh_cut_from_account(home: &HomeLayout) {
    let Ok(state) = membership::load(home) else {
        return;
    };
    let Some(token) = state.device_token.as_deref().filter(|_| state.logged_in) else {
        return;
    };
    let origin = match bound_account_origin(&state) {
        Ok(origin) => origin,
        Err(_) => return,
    };
    let client = match AccountClient::new(&origin) {
        Ok(client) => client,
        Err(_) => return,
    };
    let remote = match client.fetch_device(token) {
        Ok(remote) => remote,
        Err(error) if error.code == "membership_disabled" => {
            let _ = mark_cut(home, None, None);
            return;
        }
        Err(_) => return,
    };
    if remote.disabled {
        let _ = mark_cut(home, remote.device_id, remote.hostname);
    }
}

fn mark_cut(
    home: &HomeLayout,
    device_id: Option<String>,
    hostname: Option<String>,
) -> Result<(), OpError> {
    membership::update(home, |state| {
        state.disabled = true;
        if device_id.is_some() {
            state.device_id = device_id;
        }
        if hostname.is_some() {
            state.hostname = hostname;
        }
        Ok(())
    })
    .map_err(OpError::io)?;
    if let Err(error) = membership_cloudflared::stop(home) {
        let _ = remember_error(home, &error.message);
    }
    let remote = settings::load(home).map_err(OpError::io)?.remote_access;
    if text(&remote, "provider", "off") == "reach" {
        settings::update(home, |global| {
            global
                .remote_access
                .insert("enabled".into(), Value::Bool(false));
            global
                .remote_access
                .insert("web_public_url".into(), Value::String(String::new()));
            global.remote_access.insert(
                "updated_at".into(),
                Value::String(cccc_contracts::utc_now()),
            );
            Ok(())
        })
        .map_err(OpError::io)?;
    }
    Ok(())
}

fn public_urls(home: &HomeLayout, hostname: Option<&str>) -> Result<PublicUrls, OpError> {
    let hostname = hostname
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.starts_with("http://") || value.starts_with("https://") {
                value.trim_end_matches('/').to_owned()
            } else {
                format!("https://{}", value.trim_end_matches('/'))
            }
        });
    let Some(origin) = hostname else {
        return Ok(PublicUrls {
            hostname: None,
            web: None,
            connector: None,
        });
    };
    let admin = AccessTokenStore::new(home.clone())
        .and_then(|store| store.list())
        .map_err(OpError::io)?
        .into_iter()
        .find(|token| token.is_admin)
        .map(|token| token.token);
    let web_url = admin.map(|token| {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("token", &token)
            .finish();
        format!("{origin}/ui/?{query}")
    });
    let connector = web_model_connectors::load(home)
        .map_err(OpError::io)?
        .into_iter()
        .find(|item| !item["revoked"].as_bool().unwrap_or(false));
    let connector_url = connector.and_then(|item| {
        let id = item["connector_id"].as_str()?.trim();
        let secret = item["secret"].as_str()?.trim();
        (!id.is_empty() && !secret.is_empty()).then(|| {
            format!(
                "{origin}/mcp/web-model/{id}/token/{}",
                utf8_percent_encode(secret, PATH_SEGMENT_ENCODE_SET)
            )
        })
    });
    Ok(PublicUrls {
        hostname: Some(origin),
        web: web_url,
        connector: connector_url,
    })
}

fn requested_account_origin(request: &DaemonRequest) -> Result<String, AccountError> {
    if request.args.contains_key("account_origin") {
        string_arg(request, "account_origin")
            .map(|value| membership::canonical_account_origin(value.trim()))
            .filter(|value| !value.is_empty())
    } else {
        membership::account_origin()
    }
    .ok_or_else(|| AccountError {
        code: "membership_unavailable",
        message: "membership account service is not configured".into(),
        retryable: false,
        retry_after_delta: 0,
    })
}

fn bound_account_origin(state: &membership::MembershipState) -> Result<String, AccountError> {
    state
        .account_origin
        .as_deref()
        .or_else(|| {
            state
                .pending_login
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|pending| pending.get("account_origin"))
                .and_then(Value::as_str)
        })
        .map(membership::canonical_account_origin)
        .filter(|origin| !origin.is_empty())
        .ok_or_else(|| AccountError {
            code: "membership_unavailable",
            message: "membership issuer is missing; run `cccc logout` and `cccc login` again"
                .into(),
            retryable: false,
            retry_after_delta: 0,
        })
}

fn live_web_port(home: &HomeLayout) -> Result<u16, OpError> {
    let path = home.daemon_dir().join("web_runtime.json");
    let runtime: Value = fs::read_json(&path).map_err(|_| {
        OpError::new(
            "membership_gate",
            "CCCC Web is not running with a known live binding; start `cccc` before enabling reach",
        )
    })?;
    let pid = runtime
        .get("pid")
        .and_then(Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 0)
        .ok_or_else(|| {
            OpError::new(
                "membership_gate",
                "CCCC Web runtime identity is invalid; restart `cccc` before enabling reach",
            )
        })?;
    if !membership_cloudflared::process_is_alive(pid) {
        return Err(OpError::new(
            "membership_gate",
            "CCCC Web runtime is no longer running; restart `cccc` before enabling reach",
        ));
    }
    let host = runtime
        .get("host")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !matches!(host.as_str(), "127.0.0.1" | "localhost" | "0.0.0.0") {
        return Err(OpError::new(
            "membership_gate",
            "CCCC Web must accept connections on 127.0.0.1 before reach can start",
        ));
    }
    runtime
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .ok_or_else(|| {
            OpError::new(
                "membership_gate",
                "CCCC Web runtime port is invalid; restart `cccc` before enabling reach",
            )
        })
}

fn pending_expired(pending: &Map<String, Value>) -> bool {
    pending
        .get("expires_at")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|expires| expires <= Utc::now())
}

fn account_fail(home: &HomeLayout, error: AccountError) -> OpError {
    let _ = remember_error(home, &error.message);
    OpError::new(error.code, error.message)
}

fn remember_runtime_error(home: &HomeLayout, error: RuntimeError) -> OpError {
    let _ = remember_error(home, &error.message);
    runtime_error(error)
}

fn runtime_error(error: RuntimeError) -> OpError {
    OpError::new(error.code, error.message)
}

fn fail(home: &HomeLayout, code: &str, message: &str) -> OpResult {
    remember_error(home, message).map_err(OpError::io)?;
    Err(OpError::new(code, message))
}

fn remember_error(home: &HomeLayout, message: &str) -> std::io::Result<()> {
    membership::update(home, |state| {
        state.last_error = Some(message.to_owned());
        Ok(())
    })
}

fn require_user(request: &DaemonRequest) -> Result<(), OpError> {
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if by.is_empty() || by == "user" {
        Ok(())
    } else {
        Err(OpError::new(
            "permission_denied",
            "only user can manage membership",
        ))
    }
}

fn admin_token_count(home: &HomeLayout) -> usize {
    AccessTokenStore::new(home.clone())
        .and_then(|store| store.list())
        .map_or(0, |tokens| {
            tokens.iter().filter(|token| token.is_admin).count()
        })
}

fn environment_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn text(config: &Map<String, Value>, key: &str, default: &str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
        .into()
}

fn integer(config: &Map<String, Value>, key: &str, default: u64) -> u64 {
    config
        .get(key)
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        })
        .unwrap_or(default)
}

fn boolean(config: &Map<String, Value>, key: &str, default: bool) -> bool {
    config.get(key).and_then(Value::as_bool).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;

    fn account_server(responses: Vec<(u16, &'static str)>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let (sent, received) = mpsc::channel();
        thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut request = [0_u8; 4096];
                let count = stream.read(&mut request).expect("request");
                sent.send(String::from_utf8_lossy(&request[..count]).into_owned())
                    .expect("capture");
                write!(
                    stream,
                    "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("response");
            }
        });
        (format!("http://{address}"), received)
    }

    #[test]
    fn login_poll_remains_bound_to_the_issuing_account_origin() {
        let (origin, requests) = account_server(vec![
            (
                200,
                r#"{"device_code":"code-rust","user_code":"USER-RUST","verification_uri":"https://verify.example.test","expires_in":900,"interval":1}"#,
            ),
            (
                200,
                r#"{"access_token":"device-token-rust","device_id":"device-rust"}"#,
            ),
        ]);
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        let login_request = DaemonRequest {
            v: 1,
            op: "membership_login".into(),
            args: json!({"by":"user","account_origin":origin})
                .as_object()
                .cloned()
                .expect("args"),
        };
        login(&home, &login_request).expect("start login");
        let poll_request = DaemonRequest {
            v: 1,
            op: "membership_login_poll".into(),
            args: json!({"by":"user","account_origin":"https://wrong.example.test"})
                .as_object()
                .cloned()
                .expect("args"),
        };
        login_poll(&home, &poll_request).expect("poll login");

        let first = requests.recv().expect("device-code request");
        let second = requests.recv().expect("device-token request");
        assert!(first.starts_with("POST /v1/device/code "));
        assert!(second.starts_with("POST /v1/device/token "));
        let state = membership::load(&home).expect("membership");
        assert_eq!(state.account_origin.as_deref(), Some(origin.as_str()));
        assert_eq!(state.device_token.as_deref(), Some("device-token-rust"));
    }

    #[test]
    fn reach_on_uses_the_account_port_and_commits_shared_state() {
        let (origin, requests) = account_server(vec![
            (
                200,
                r#"{"device_id":"device-rust","hostname":"https://device-rust.example.test","disabled":false,"online":false}"#,
            ),
            (
                200,
                r#"{"hostname":"https://device-rust.example.test","tunnel_token":"tunnel-rust"}"#,
            ),
        ]);
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        AccessTokenStore::new(home.clone())
            .expect("tokens")
            .create("admin", Vec::new(), true, Some("acc_rust_fixture"))
            .expect("admin token");
        membership::save(
            &home,
            &membership::MembershipState {
                logged_in: true,
                account_origin: Some(origin.clone()),
                device_id: Some("device-rust".into()),
                device_token: Some("device-token".into()),
                ..membership::MembershipState::default()
            },
        )
        .expect("membership");
        settings::update(&home, |global| {
            global
                .remote_access
                .insert("web_port".into(), Value::from(9000));
            Ok(())
        })
        .expect("settings");
        let request = DaemonRequest {
            v: 1,
            op: "membership_reach_on".into(),
            args: json!({"by":"user","account_origin":origin})
                .as_object()
                .cloned()
                .expect("args"),
        };
        let started_token = Arc::new(Mutex::new(None::<String>));
        let captured = Arc::clone(&started_token);
        let result = reach_on_with(
            &home,
            &request,
            |_home| Ok(9000),
            |_home| Ok(()),
            move |_home, token| {
                *captured.lock().expect("capture token") = Some(token.into());
                Ok(())
            },
        )
        .expect("reach on");
        assert_eq!(
            result["membership"]["hostname"],
            "https://device-rust.example.test"
        );
        assert_eq!(
            started_token.lock().expect("started token").as_deref(),
            Some("tunnel-rust")
        );
        let _device_request = requests.recv().expect("device request");
        let reach_request = requests.recv().expect("reach request");
        assert!(reach_request.contains(r#""origin_port":9000"#));
        let state = membership::load(&home).expect("saved state");
        assert_eq!(state.tunnel_token.as_deref(), Some("tunnel-rust"));
        let remote = settings::load(&home).expect("saved settings").remote_access;
        assert_eq!(remote["provider"], "reach");
        assert_eq!(remote["enabled"], true);
    }

    #[test]
    fn public_urls_include_local_admin_and_connector_credentials() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        AccessTokenStore::new(home.clone())
            .expect("tokens")
            .create("admin", Vec::new(), true, Some("acc secret"))
            .expect("admin token");
        web_model_connectors::replace_active(
            &home,
            &json!({
                "connector_id":"wmc_fixture",
                "group_id":"g_fixture",
                "actor_id":"peer1",
                "secret":"connector_/secret-~.",
                "created_at":"2026-08-19T00:00:00Z"
            }),
        )
        .expect("connector");

        let urls = public_urls(&home, Some("d-fixture.example.test/")).expect("urls");
        assert_eq!(
            urls.hostname.as_deref(),
            Some("https://d-fixture.example.test")
        );
        assert_eq!(
            urls.web.as_deref(),
            Some("https://d-fixture.example.test/ui/?token=acc+secret")
        );
        assert_eq!(
            urls.connector.as_deref(),
            Some(
                "https://d-fixture.example.test/mcp/web-model/wmc_fixture/token/connector_%2Fsecret-~."
            )
        );
    }

    #[test]
    fn reach_uses_the_recorded_live_web_port() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        fs::write_json(
            &home.daemon_dir().join("web_runtime.json"),
            &json!({"pid":std::process::id(),"host":"127.0.0.1","port":9123}),
        )
        .expect("runtime state");
        assert_eq!(live_web_port(&home).expect("live port"), 9123);
    }

    #[test]
    fn reach_rejects_a_live_web_binding_that_cannot_accept_loopback() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        fs::write_json(
            &home.daemon_dir().join("web_runtime.json"),
            &json!({"pid":std::process::id(),"host":"192.0.2.10","port":9123}),
        )
        .expect("runtime state");
        let error = live_web_port(&home).expect_err("non-loopback-only binding must fail");
        assert_eq!(error.code, "membership_gate");
        assert!(error.message.contains("127.0.0.1"));
    }

    #[test]
    fn reach_issuance_disabled_applies_cut_before_returning() {
        let (origin, _requests) = account_server(vec![
            (
                200,
                r#"{"device_id":"device-rust","hostname":"https://device-rust.example.test","disabled":false,"online":true}"#,
            ),
            (
                403,
                r#"{"error":{"code":"disabled","message":"device disabled"}}"#,
            ),
        ]);
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        AccessTokenStore::new(home.clone())
            .expect("tokens")
            .create("admin", Vec::new(), true, None)
            .expect("admin token");
        membership::save(
            &home,
            &membership::MembershipState {
                logged_in: true,
                account_origin: Some(origin),
                device_id: Some("device-rust".into()),
                device_token: Some("device-token".into()),
                ..membership::MembershipState::default()
            },
        )
        .expect("membership");
        settings::update(&home, |global| {
            global
                .remote_access
                .insert("provider".into(), Value::String("reach".into()));
            global
                .remote_access
                .insert("enabled".into(), Value::Bool(true));
            global.remote_access.insert(
                "web_public_url".into(),
                Value::String("https://old.example.test".into()),
            );
            Ok(())
        })
        .expect("settings");
        let request = DaemonRequest {
            v: 1,
            op: "membership_reach_on".into(),
            args: json!({"by":"user"}).as_object().cloned().expect("args"),
        };

        let error = reach_on_with(
            &home,
            &request,
            |_home| Ok(8848),
            |_home| Ok(()),
            |_home, _token| panic!("disabled reach must not start the helper"),
        )
        .expect_err("reach must be cut");
        assert_eq!(error.code, "membership_disabled");
        assert!(membership::load(&home).expect("membership").disabled);
        let remote = settings::load(&home).expect("settings").remote_access;
        assert_eq!(remote["enabled"], false);
        assert_eq!(remote["web_public_url"], "");
    }

    #[test]
    fn logout_fails_closed_on_unretired_tracking_after_provider_drift() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        membership::save(
            &home,
            &membership::MembershipState {
                logged_in: true,
                device_id: Some("device-rust".into()),
                hostname: Some("https://device-rust.example.test".into()),
                ..membership::MembershipState::default()
            },
        )
        .expect("membership");
        settings::update(&home, |global| {
            global
                .remote_access
                .insert("provider".into(), Value::String("manual".into()));
            global.remote_access.insert(
                "web_public_url".into(),
                Value::String("https://manual.example.test".into()),
            );
            Ok(())
        })
        .expect("settings");
        let helper_dir = home.root().join("libexec").join("cloudflared");
        std::fs::create_dir_all(&helper_dir).expect("helper dir");
        std::fs::write(helper_dir.join("cloudflared.pid"), "malformed").expect("pid marker");
        let request = DaemonRequest {
            v: 1,
            op: "membership_logout".into(),
            args: json!({"by":"user"}).as_object().cloned().expect("args"),
        };

        let error = logout(&home, &request).expect_err("tracking must be retired first");
        assert_eq!(error.code, "membership_subprocess");
        assert!(membership::load(&home).expect("membership").logged_in);
        assert_eq!(
            settings::load(&home).expect("settings").remote_access["web_public_url"],
            "https://manual.example.test"
        );
    }
}
