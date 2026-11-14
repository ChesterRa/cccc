use anyhow::{Result, ensure};
use cccc_client::DaemonClient;
use cccc_contracts::{
    DaemonRequest,
    session_node::{SESSION_NODE_PROTOCOL, SESSION_NODE_RELAY_TICKET},
};
use cccc_core::HomeLayout;
use serde_json::json;

pub async fn verify(home: &HomeLayout, needs_relay: bool) -> Result<()> {
    let response = DaemonClient::new(home.clone())
        .without_voice_origin()
        .with_timeout(std::time::Duration::from_secs(3))
        .call(&DaemonRequest {
            v: 1,
            op: "session_node_capabilities".into(),
            args: Default::default(),
        })
        .await?;
    validate(&response, needs_relay)
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
    }
}
