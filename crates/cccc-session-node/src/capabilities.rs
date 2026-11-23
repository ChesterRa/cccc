use anyhow::{Context, Result, ensure};
use cccc_client::DaemonClient;
use cccc_contracts::{
    DaemonRequest,
    local_session::LocalSessionRuntime,
    session_node::{
        DEVICE_SESSION_RUNTIMES, LEGACY_DEVICE_SESSION_RUNTIMES, SESSION_NODE_PROTOCOL,
        SESSION_NODE_RELAY_TICKET,
    },
};
use cccc_core::HomeLayout;
use serde_json::{Value, json};

pub async fn verify(home: &HomeLayout, needs_relay: bool) -> Result<Vec<LocalSessionRuntime>> {
    let response = response(home).await?;
    validate(&response, needs_relay)?;
    advertised_runtimes(response.result.get("runtimes"))
}

pub(crate) fn advertised_runtimes(value: Option<&Value>) -> Result<Vec<LocalSessionRuntime>> {
    let Some(value) = value else {
        return Ok(LEGACY_DEVICE_SESSION_RUNTIMES.to_vec());
    };
    let names: Vec<String> = serde_json::from_value(value.clone())
        .context("Invalid device-session runtime capability list")?;
    Ok(DEVICE_SESSION_RUNTIMES
        .into_iter()
        .filter(|runtime| names.iter().any(|name| name == runtime.as_str()))
        .collect())
}

async fn response(home: &HomeLayout) -> Result<cccc_contracts::DaemonResponse> {
    let response = DaemonClient::new(home.clone())
        .without_voice_origin()
        .with_timeout(std::time::Duration::from_secs(3))
        .call(&DaemonRequest {
            v: 1,
            op: "session_node_capabilities".into(),
            args: Default::default(),
        })
        .await?;
    Ok(response)
}

fn validate(response: &cccc_contracts::DaemonResponse, needs_relay: bool) -> Result<()> {
    ensure!(
        response.ok && response.result.get("protocol") == Some(&json!(SESSION_NODE_PROTOCOL)),
        "The running daemon does not support device sessions; update/restart that daemon explicitly before pairing"
    );
    ensure!(
        !needs_relay
            || response.result.get("relay_ticket_version")
                == Some(&json!(SESSION_NODE_RELAY_TICKET)),
        "The running daemon does not support relay pairing; update/restart it explicitly before public device access"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_daemons_keep_direct_support_but_cannot_silently_accept_relay_descriptors() {
        let legacy = cccc_contracts::DaemonResponse::success(
            json!({"protocol":1,"runtimes":["codex","claude"]})
                .as_object()
                .expect("packet")
                .clone(),
        );
        assert!(validate(&legacy, false).is_ok());
        assert!(validate(&legacy, true).is_err());
        let current = cccc_contracts::DaemonResponse::success(
            json!({"protocol":1,"relay_ticket_version":2})
                .as_object()
                .expect("packet")
                .clone(),
        );
        assert!(validate(&current, true).is_ok());
        // A retired approval capability does not disable ordinary relay tickets.
        let retired = cccc_contracts::DaemonResponse::success(
            json!({"protocol":1,"relay_ticket_version":2,"join_identity_binding":false})
                .as_object()
                .expect("packet")
                .clone(),
        );
        assert!(validate(&retired, true).is_ok());
    }
}
