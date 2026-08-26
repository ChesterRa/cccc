use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{HomeLayout, settings};
use serde_json::{Map, Value, json};
use std::process::Command;

use crate::dispatch::{OpError, OpResult, object, string_arg};

const REMOTE_ACCESS_MODE: &str = "tailnet_only";

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "remote_access_state" => state(home),
        "remote_access_configure" => configure(home, request),
        "remote_access_start" => set_running(home, request, true),
        "remote_access_stop" => set_running(home, request, false),
        _ => return None,
    })
}

fn state(home: &HomeLayout) -> OpResult {
    let mut global = settings::load(home).map_err(OpError::io)?;
    normalize(&mut global.remote_access)?;
    object(payload(home, &global.remote_access))
}

fn configure(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    if request
        .args
        .get("provider")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("reach"))
    {
        return Err(OpError::new(
            "remote_access_invalid_config",
            "reach is managed by cccc reach; use `cccc reach on`",
        ));
    }
    let mut global = settings::load(home).map_err(OpError::io)?;
    if text(&global.remote_access, "provider", "off") == "reach"
        && (boolean(&global.remote_access, "enabled", false)
            || super::membership_cloudflared::status(home).running)
    {
        return Err(OpError::new(
            "remote_access_invalid_config",
            "reach is active; use `cccc reach off` before changing remote access configuration",
        ));
    }
    let before_host = text(&global.remote_access, "web_host", "127.0.0.1");
    let before_port = number(&global.remote_access, "web_port", 8848);
    apply_configure_patch(&mut global.remote_access, request);
    normalize(&mut global.remote_access)?;
    let restart_required = before_host != text(&global.remote_access, "web_host", "127.0.0.1")
        || before_port != number(&global.remote_access, "web_port", 8848);
    let remote_access = settings::update(home, |latest| {
        apply_configure_patch(&mut latest.remote_access, request);
        normalize(&mut latest.remote_access)
            .map_err(|error| std::io::Error::other(error.message))?;
        latest
            .remote_access
            .insert("updated_at".into(), Value::String(utc_now()));
        Ok(latest.remote_access.clone())
    })
    .map_err(OpError::io)?;
    let mut result = payload(home, &remote_access);
    if restart_required
        && let Some(remote) = result
            .get_mut("remote_access")
            .and_then(Value::as_object_mut)
    {
        remote.insert("restart_required".into(), Value::Bool(true));
    }
    object(result)
}

fn set_running(home: &HomeLayout, request: &DaemonRequest, running: bool) -> OpResult {
    require_user(request)?;
    let mut global = settings::load(home).map_err(OpError::io)?;
    normalize(&mut global.remote_access)?;
    let provider = text(&global.remote_access, "provider", "off");
    if running && provider == "off" {
        return Err(OpError::new(
            "remote_access_invalid_config",
            "remote access provider is off",
        ));
    }
    if provider == "reach" {
        return if running {
            super::membership::reach_on(home, request)
        } else {
            super::membership::reach_off(home, request)
        };
    }
    let host = text(&global.remote_access, "web_host", "127.0.0.1");
    let public_url = text(&global.remote_access, "web_public_url", "");
    let remotely_reachable = remote_web_exposure(&host, &public_url);
    let (_, admin_token_count) = token_counts(home);
    if running && remotely_reachable && admin_token_count == 0 {
        return Err(OpError::new(
            "remote_access_admin_token_required",
            "an administrator access token is required before remote access can start",
        ));
    }
    if running && !remotely_reachable && !environment_flag("CCCC_REMOTE_ALLOW_LOOPBACK") {
        return Err(OpError::new(
            "remote_access_unreachable",
            "web server binding is not remotely reachable",
        ));
    }
    if provider == "tailscale" {
        let command = if running { "up" } else { "down" };
        let output = Command::new("tailscale")
            .arg(command)
            .output()
            .map_err(|error| OpError::new("remote_access_not_installed", error.to_string()))?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(OpError::new(
                if running {
                    "remote_access_start_failed"
                } else {
                    "remote_access_stop_failed"
                },
                if message.is_empty() {
                    format!("tailscale {command} failed")
                } else {
                    message
                },
            ));
        }
    }
    let remote_access = settings::update(home, |latest| {
        normalize(&mut latest.remote_access)
            .map_err(|error| std::io::Error::other(error.message))?;
        latest
            .remote_access
            .insert("enabled".into(), Value::Bool(running));
        latest
            .remote_access
            .insert("updated_at".into(), Value::String(utc_now()));
        Ok(latest.remote_access.clone())
    })
    .map_err(OpError::io)?;
    object(payload(home, &remote_access))
}

fn apply_configure_patch(config: &mut Map<String, Value>, request: &DaemonRequest) {
    for key in [
        "provider",
        "mode",
        "enabled",
        "require_access_token",
        "web_host",
        "web_port",
        "web_public_url",
    ] {
        if let Some(value) = request.args.get(key) {
            config.insert(key.into(), value.clone());
        }
    }
}

fn normalize(config: &mut Map<String, Value>) -> Result<(), OpError> {
    let provider = text(config, "provider", "off").to_ascii_lowercase();
    if !matches!(provider.as_str(), "off" | "manual" | "tailscale" | "reach") {
        return Err(OpError::new(
            "remote_access_invalid_config",
            "provider must be off, manual, tailscale, or reach",
        ));
    }
    let mode = match text(config, "mode", REMOTE_ACCESS_MODE)
        .to_ascii_lowercase()
        .as_str()
    {
        REMOTE_ACCESS_MODE | "team" | "serve" | "funnel" => REMOTE_ACCESS_MODE.to_owned(),
        _ => {
            return Err(OpError::new(
                "remote_access_invalid_config",
                "unsupported remote access mode",
            ));
        }
    };
    let port = number(config, "web_port", 8848);
    if !(1..=65_535).contains(&port) {
        return Err(OpError::new(
            "remote_access_invalid_config",
            "web_port must be between 1 and 65535",
        ));
    }
    let public_url = text(config, "web_public_url", "");
    let host = text(config, "web_host", "127.0.0.1");
    if provider == "tailscale" && !public_url.is_empty() {
        return Err(OpError::new(
            "remote_access_invalid_config",
            "tailscale does not use web_public_url",
        ));
    }
    if remote_web_exposure(&host, &public_url) && !boolean(config, "require_access_token", true) {
        return Err(OpError::new(
            "remote_access_invalid_config",
            "remote Web exposure requires an access token",
        ));
    }
    if provider == "manual"
        && public_url.is_empty()
        && !is_loopback_host(&host)
        && !environment_flag("CCCC_REMOTE_ALLOW_INSECURE")
    {
        return Err(OpError::new(
            "remote_access_invalid_config",
            "plain HTTP LAN exposure requires CCCC_REMOTE_ALLOW_INSECURE=1; prefer an HTTPS reverse proxy or encrypted overlay",
        ));
    }
    config.insert("provider".into(), Value::String(provider));
    config.insert("mode".into(), Value::String(mode));
    config.insert("web_port".into(), Value::Number(port.into()));
    Ok(())
}

fn payload(home: &HomeLayout, config: &Map<String, Value>) -> Value {
    let provider = text(config, "provider", "off");
    let mode = text(config, "mode", REMOTE_ACCESS_MODE);
    let enabled = boolean(config, "enabled", false) && provider != "off";
    let require_token = boolean(config, "require_access_token", true);
    let host = text(config, "web_host", "127.0.0.1");
    let port = number(config, "web_port", 8848);
    let public_url = text(config, "web_public_url", "");
    let (tokens, admin_tokens) = token_counts(home);
    let tailscale_installed = command_exists("tailscale");
    let reach_helper_running =
        provider == "reach" && super::membership_cloudflared::status(home).running;
    let reachable = remote_web_exposure(&host, &public_url);
    let allow_unauthenticated_listener = environment_flag("CCCC_WEB_ALLOW_UNAUTHENTICATED");
    let allow_loopback_remote = environment_flag("CCCC_REMOTE_ALLOW_LOOPBACK");
    let remote_listener_auth_required = reachable;
    let remote_listener_auth_requirement_satisfied =
        !remote_listener_auth_required || admin_tokens > 0;
    let effective_require_token = if !reachable {
        false
    } else if !public_url.is_empty() {
        true
    } else {
        require_token
    };
    let supervised = environment_flag("CCCC_WEB_SUPERVISED");
    let live_host = std::env::var("CCCC_WEB_EFFECTIVE_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let live_port = std::env::var("CCCC_WEB_EFFECTIVE_PORT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    let live_runtime_present = supervised || live_host.is_some() || live_port.is_some();
    let live_matches = live_runtime_present
        && live_host.as_deref().unwrap_or("127.0.0.1") == host
        && live_port.unwrap_or(8848) == port;
    let restart_required = live_runtime_present && !live_matches;
    let display_host = if matches!(host.as_str(), "0.0.0.0" | "::") {
        "127.0.0.1"
    } else {
        host.as_str()
    };
    let desired_local_url = format!("http://{display_host}:{port}");
    let desired_remote_url = if public_url.is_empty() {
        format!("http://{host}:{port}")
    } else {
        public_url.clone()
    };
    let misconfigured = enabled
        && (!remote_listener_auth_requirement_satisfied || (!reachable && !allow_loopback_remote));
    let status = if misconfigured {
        "misconfigured"
    } else if provider == "tailscale" && !tailscale_installed {
        "not_installed"
    } else if provider == "reach" {
        if enabled && reach_helper_running {
            "running"
        } else if enabled || reach_helper_running {
            "error"
        } else {
            "stopped"
        }
    } else if enabled {
        "running"
    } else {
        "stopped"
    };
    let endpoint = if status == "running" {
        if !public_url.is_empty() {
            Some(public_url.clone())
        } else {
            Some(format!("http://{host}:{port}"))
        }
    } else {
        None
    };
    let status_reason = if misconfigured && !remote_listener_auth_requirement_satisfied {
        "missing_access_token"
    } else if misconfigured && !reachable {
        "binding_unreachable"
    } else {
        status
    };
    let next_steps = if !remote_listener_auth_requirement_satisfied {
        vec!["Create an Admin Access Token in Settings > Web Access before exposing Web remotely."]
    } else {
        Vec::new()
    };
    json!({"remote_access":{
        "provider":provider,
        "mode":mode,
        "require_access_token":require_token,
        "enabled":enabled,
        "status":status,
        "status_reason":status_reason,
        "endpoint":endpoint,
        "updated_at":config.get("updated_at").cloned().unwrap_or(Value::Null),
        "restart_required":restart_required,
        "apply_supported":supervised && live_runtime_present,
        "diagnostics":{
            "mode_supported":true,
            "web_bind_reachable":reachable,
            "access_token_present":tokens > 0,
            "access_token_count":tokens,
            "access_token_requirement_satisfied":if effective_require_token {tokens > 0}else{true},
            "admin_access_token_present":admin_tokens > 0,
            "admin_access_token_count":admin_tokens,
            "remote_listener_auth_required":remote_listener_auth_required,
            "remote_listener_auth_requirement_satisfied":remote_listener_auth_requirement_satisfied,
            "allow_unauthenticated_listener_override":allow_unauthenticated_listener,
            "effective_require_access_token":effective_require_token,
            "tailscale_installed":tailscale_installed,
            "reach_helper_running":reach_helper_running
            ,"desired_local_url":desired_local_url
            ,"desired_remote_url":desired_remote_url
            ,"live_runtime_present":live_runtime_present
            ,"live_runtime_pid":if live_runtime_present {Value::from(std::process::id())} else {Value::Null}
            ,"live_runtime_host":live_host
            ,"live_runtime_port":live_port
            ,"live_runtime_supervisor_managed":supervised
            ,"live_runtime_matches_binding":live_matches
        },
        "config":{
            "web_host":host,
            "web_port":port,
            "web_public_url":if public_url.is_empty(){Value::Null}else{Value::String(public_url)},
            "access_token_configured":tokens > 0,
            "access_token_count":tokens,
            "admin_access_token_configured":admin_tokens > 0,
            "admin_access_token_count":admin_tokens,
            "access_token_source":"rust_home"
        },
        "next_steps":next_steps
    }})
}

fn environment_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn require_user(request: &DaemonRequest) -> Result<(), OpError> {
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if by.is_empty() || by == "user" {
        Ok(())
    } else {
        Err(OpError::new(
            "permission_denied",
            "only user can manage remote access",
        ))
    }
}

fn token_counts(home: &HomeLayout) -> (usize, usize) {
    AccessTokenStore::new(home.clone())
        .and_then(|store| store.list())
        .map_or((0, 0), |tokens| {
            let total = tokens.len();
            let admin = tokens.iter().filter(|token| token.is_admin).count();
            (total, admin)
        })
}

fn is_loopback_host(host: &str) -> bool {
    matches!(
        host.trim().to_ascii_lowercase().as_str(),
        "" | "127.0.0.1" | "localhost" | "::1" | "[::1]"
    )
}

fn remote_web_exposure(host: &str, public_url: &str) -> bool {
    !public_url.trim().is_empty() || !is_loopback_host(host)
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

fn boolean(config: &Map<String, Value>, key: &str, default: bool) -> bool {
    config.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn number(config: &Map<String, Value>, key: &str, default: u64) -> u64 {
    config
        .get(key)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
        .unwrap_or(default)
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|path| {
            let candidate = path.join(name);
            candidate.is_file() || cfg!(windows) && path.join(format!("{name}.exe")).is_file()
        })
    })
}
