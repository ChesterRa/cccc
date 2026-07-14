use anyhow::{Result, bail};
use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{HomeLayout, active};
use serde_json::Value;

pub async fn call(client: &DaemonClient, op: &str, args: Value) -> Result<DaemonResponse> {
    let args = args.as_object().cloned().unwrap_or_default();
    client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        })
        .await
        .map_err(|error| anyhow::anyhow!("daemon unavailable ({error}); run `cccc daemon start`"))
}

pub fn print(response: DaemonResponse) -> Result<()> {
    if response.ok {
        println!("{}", serde_json::to_string_pretty(&response.result)?);
        return Ok(());
    }
    let error = response.error.map_or_else(
        || "unknown daemon error".into(),
        |error| format!("{}: {}", error.code, error.message),
    );
    bail!(error)
}

pub fn group(home: &HomeLayout, requested: Option<String>) -> Result<String> {
    requested
        .filter(|value| !value.trim().is_empty())
        .or(active::get(home)?)
        .ok_or_else(|| {
            anyhow::anyhow!("no active group; pass --group or run `cccc use <group_id>`")
        })
}

pub fn env(values: Vec<String>) -> Result<serde_json::Map<String, Value>> {
    values
        .into_iter()
        .map(|item| {
            let (key, value) = item
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("environment value must be KEY=VALUE: {item}"))?;
            if key.trim().is_empty() {
                bail!("environment key cannot be empty");
            }
            Ok((key.trim().into(), Value::String(value.into())))
        })
        .collect()
}
