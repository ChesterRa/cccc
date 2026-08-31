use cccc_core::{HomeLayout, fs, web_runtime_proof};
use serde_json::Value;

use crate::dispatch::OpError;

pub(super) fn live_web_port(home: &HomeLayout) -> Result<u16, OpError> {
    let runtime: Value = fs::read_json(&home.daemon_dir().join("web_runtime.json")).map_err(|_| {
        OpError::new(
            "membership_gate",
            "CCCC Web is not running with a known live binding; start `cccc` before enabling reach",
        )
    })?;
    let pid = required_u32(&runtime, "pid", "identity")?;
    if !crate::ops::membership_cloudflared::process_is_alive(pid) {
        return Err(gate("CCCC Web runtime is no longer running"));
    }
    let runtime_id = required_string(&runtime, "runtime_id", "identity")?;
    let proof_key = required_string(&runtime, "runtime_proof_key", "proof key")?;
    let host = runtime["host"]
        .as_str()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !matches!(host.as_str(), "127.0.0.1" | "localhost" | "0.0.0.0") {
        return Err(OpError::new(
            "membership_gate",
            "CCCC Web must accept connections on 127.0.0.1 before reach can start",
        ));
    }
    let port = runtime["port"]
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .ok_or_else(|| gate("CCCC Web runtime port is invalid"))?;
    if !identity_matches(port, runtime_id, proof_key) {
        return Err(gate(
            "CCCC Web recorded binding did not prove its runtime identity",
        ));
    }
    Ok(port)
}

fn identity_matches(port: u16, expected_runtime_id: &str, proof_key: &str) -> bool {
    let Ok(client) = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_millis(500))
        .timeout(std::time::Duration::from_millis(750))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
    else {
        return false;
    };
    let challenge = uuid::Uuid::new_v4().simple().to_string();
    let Ok(response) = client
        .get(format!("http://127.0.0.1:{port}/api/v1/ready"))
        .query(&[("challenge", &challenge)])
        .send()
    else {
        return false;
    };
    response.status().is_success()
        && response.json::<Value>().is_ok_and(|payload| {
            payload["ok"] == true
                && payload["result"]["web"] == "ready"
                && payload["result"]["runtime_id"] == expected_runtime_id
                && payload["result"]["proof"]
                    .as_str()
                    .is_some_and(|proof| web_runtime_proof::verify(proof_key, &challenge, proof))
        })
}

fn required_u32(runtime: &Value, key: &str, label: &str) -> Result<u32, OpError> {
    runtime[key]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| gate(&format!("CCCC Web runtime {label} is invalid")))
}

fn required_string<'a>(runtime: &'a Value, key: &str, label: &str) -> Result<&'a str, OpError> {
    runtime[key]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| gate(&format!("CCCC Web runtime {label} is missing")))
}

fn gate(message: &str) -> OpError {
    OpError::new(
        "membership_gate",
        format!("{message}; restart `cccc` before enabling reach"),
    )
}

#[cfg(test)]
#[path = "tests/web_identity.rs"]
mod tests;
