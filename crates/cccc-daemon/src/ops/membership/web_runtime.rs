use cccc_core::{HomeLayout, fs, web_runtime_proof};
use serde_json::Value;

use crate::dispatch::OpError;

pub(crate) struct LiveWebBinding {
    pub(crate) host: String,
    pub(crate) port: u16,
}

struct RecordedWebRuntime {
    binding: LiveWebBinding,
    runtime_id: String,
    proof_key: String,
}

pub(super) fn live_web_port(home: &HomeLayout) -> Result<u16, OpError> {
    let runtime = recorded_web_runtime(home)?;
    if !matches!(
        runtime.binding.host.as_str(),
        "127.0.0.1" | "localhost" | "0.0.0.0"
    ) {
        return Err(OpError::new(
            "membership_gate",
            "CCCC Web must accept connections on 127.0.0.1 before reach can start",
        ));
    }
    if !identity_matches(
        "127.0.0.1",
        runtime.binding.port,
        &runtime.runtime_id,
        &runtime.proof_key,
    ) {
        return Err(identity_error());
    }
    Ok(runtime.binding.port)
}

pub(crate) fn validated_live_web_binding(home: &HomeLayout) -> Result<LiveWebBinding, OpError> {
    let runtime = recorded_web_runtime(home)?;
    let verification_host = match runtime.binding.host.as_str() {
        "0.0.0.0" => "127.0.0.1",
        "::" => "::1",
        host => host,
    };
    if !identity_matches(
        verification_host,
        runtime.binding.port,
        &runtime.runtime_id,
        &runtime.proof_key,
    ) {
        return Err(identity_error());
    }
    Ok(runtime.binding)
}

fn recorded_web_runtime(home: &HomeLayout) -> Result<RecordedWebRuntime, OpError> {
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
    let runtime_id = required_string(&runtime, "runtime_id", "identity")?.to_owned();
    let proof_key = required_string(&runtime, "runtime_proof_key", "proof key")?.to_owned();
    let host = runtime["host"]
        .as_str()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if host.is_empty() {
        return Err(gate("CCCC Web runtime host is missing"));
    }
    let port = runtime["port"]
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .ok_or_else(|| gate("CCCC Web runtime port is invalid"))?;
    Ok(RecordedWebRuntime {
        binding: LiveWebBinding { host, port },
        runtime_id,
        proof_key,
    })
}

fn identity_matches(host: &str, port: u16, expected_runtime_id: &str, proof_key: &str) -> bool {
    // `reqwest::blocking` waits construct and drop a shell Tokio runtime on
    // the calling thread; inside an async runtime context that drop panics
    // ("Cannot drop a runtime in a context where blocking is not allowed").
    // Live binding discovery is called from `#[tokio::main]` in the CLI, so
    // run the whole probe on a dedicated thread to keep it safe from any
    // caller context. A probe panic fails closed, like any probe failure.
    std::thread::scope(|scope| {
        let Ok(probe) = std::thread::Builder::new()
            .name("cccc-web-identity-probe".to_owned())
            .spawn_scoped(scope, move || {
                probe_identity(host, port, expected_runtime_id, proof_key)
            })
        else {
            return false;
        };
        probe.join().unwrap_or(false)
    })
}

fn probe_identity(host: &str, port: u16, expected_runtime_id: &str, proof_key: &str) -> bool {
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
    let host = if host.starts_with('[') && host.ends_with(']') {
        host.to_owned()
    } else if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let Ok(response) = client
        .get(format!("http://{host}:{port}/api/v1/ready"))
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

fn identity_error() -> OpError {
    gate("CCCC Web recorded binding did not prove its runtime identity")
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
