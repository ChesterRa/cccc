use super::*;
use crate::args::SessionNodeAction;
#[path = "node_connect.rs"]
mod connect;

pub(super) async fn run(home: &HomeLayout, action: SessionNodeAction) -> Result<()> {
    match action {
        SessionNodeAction::Host { port } => {
            ensure_daemon(home).await?;
            let client = DaemonClient::new(home.clone());
            let response = manage(
                &client,
                "session_node_invite",
                json!({"by":"user","port":port}),
            )
            .await?;
            if !response.ok {
                return print(response);
            }
            let ticket = response
                .result
                .get("ticket")
                .and_then(Value::as_str)
                .unwrap_or_default();
            println!("{ticket}");
            eprintln!(
                "Pairing ticket expires in 10 minutes and enrolls one computer. Keep it private. The CCCC daemon keeps the gateway running; stop it with cccc sessions node stop."
            );
            Ok(())
        }
        SessionNodeAction::Stop => {
            let client = DaemonClient::new(home.clone());
            print(manage(&client, "session_node_stop", json!({"by":"user"})).await?)
        }
        SessionNodeAction::Connect {
            ticket_stdin,
            name,
            runtime,
        } => connect::run(home, ticket_stdin, name, runtime.as_deref()).await,
        SessionNodeAction::Status => {
            let client = DaemonClient::new(home.clone());
            // This computer's own connector stays inspectable without a running daemon.
            let mut status =
                match manage(&client, "session_node_status", json!({"by":"user"})).await {
                    Ok(response) if response.ok => Value::Object(response.result),
                    Ok(response) => json!({"host_error":response.error.map(|error| error.code)}),
                    Err(_) => json!({"host_error":"daemon_unavailable"}),
                };
            if let Some(client) = cccc_core::session_nodes::saved_client(home)? {
                status["connector"] = json!({"hub_id":client.endpoint.endpoint_id,"node_id":client.node_id,"connected":client.owner.alive() && client.connected,"memberships":client.memberships});
            }
            print(cccc_contracts::DaemonResponse::success(
                status.as_object().expect("status object").clone(),
            ))
        }
        SessionNodeAction::Revoke { node_id } => {
            let client = DaemonClient::new(home.clone());
            print(
                manage(
                    &client,
                    "session_node_revoke",
                    json!({"by":"user","node_id":node_id}),
                )
                .await?,
            )
        }
    }
}

/// A daemon from before device-session management answers `unknown_op`; say what to do instead.
async fn manage(
    client: &DaemonClient,
    op: &str,
    args: Value,
) -> Result<cccc_contracts::DaemonResponse> {
    let mut response = call(client, op, args).await?;
    if let Some(error) = response
        .error
        .as_mut()
        .filter(|error| error.code == "unknown_op")
    {
        error.code = "session_node_upgrade_required".into();
        error.message = "The running CCCC daemon is older than this command; restart it with `cccc daemon stop` and `cccc daemon start`".into();
    }
    Ok(response)
}

async fn ensure_daemon(home: &HomeLayout) -> Result<()> {
    home.initialize()?;
    // Start only a vacant Home. Never take over or restart a running daemon
    // just to add a network connector.
    cccc_daemon::DetachedDaemon::new(std::env::current_exe()?, ["daemon", "run"])
        .start(home)
        .await?;
    cccc_session_node::verify_daemon(home, false).await
}
