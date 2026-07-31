use serde_json::{Value, json};

use crate::AppState;

pub(super) fn apply(state: &AppState, remote: &mut Value) {
    let desired_host = remote["config"]["web_host"].as_str().unwrap_or("127.0.0.1");
    let desired_port = remote["config"]["web_port"].as_u64().unwrap_or(8848);
    let live_host = std::env::var("CCCC_WEB_EFFECTIVE_HOST")
        .or_else(|_| std::env::var("CCCC_WEB_HOST"))
        .unwrap_or_else(|_| "127.0.0.1".into());
    let live_port = std::env::var("CCCC_WEB_EFFECTIVE_PORT")
        .or_else(|_| std::env::var("CCCC_WEB_PORT"))
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(8848);
    let supervised = state.restart.is_some();
    let matches = desired_host == live_host && desired_port == live_port;
    remote["restart_required"] = json!(!matches);
    remote["apply_supported"] = json!(supervised);
    remote["diagnostics"]["live_runtime_present"] = json!(true);
    remote["diagnostics"]["live_runtime_pid"] = json!(std::process::id());
    remote["diagnostics"]["live_runtime_host"] = json!(live_host);
    remote["diagnostics"]["live_runtime_port"] = json!(live_port);
    remote["diagnostics"]["live_runtime_supervisor_managed"] = json!(supervised);
    remote["diagnostics"]["live_runtime_matches_binding"] = json!(matches);
}
