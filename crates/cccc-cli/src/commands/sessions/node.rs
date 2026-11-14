use super::*;
use crate::args::SessionNodeAction;
use tokio::io::AsyncBufReadExt;

pub(super) async fn run(home: &HomeLayout, action: SessionNodeAction) -> Result<()> {
    match action {
        SessionNodeAction::Host { port } => {
            ensure_daemon(home).await?;
            let ticket = cccc_web::session_node::create_invitation(home, port).await?;
            println!("{}", cccc_web::session_node::encode_ticket(&ticket)?);
            eprintln!(
                "Pairing ticket expires in 10 minutes and enrolls one computer. Keep it private. Leave this gateway running; press Ctrl-C to stop."
            );
            cccc_web::session_node::stop_signal().await;
            cccc_web::session_node::stop(home).await;
            Ok(())
        }
        SessionNodeAction::Connect { ticket_stdin, name } => {
            ensure_daemon(home).await?;
            let ticket = if ticket_stdin {
                eprintln!("Paste the hub's session-node ticket, then press Enter:");
                let mut line = String::new();
                tokio::io::BufReader::new(tokio::io::stdin().take(16 * 1024 + 1))
                    .read_line(&mut line)
                    .await?;
                Some(cccc_web::session_node::decode_ticket(&line)?)
            } else {
                None
            };
            cccc_web::session_node::connect(home, ticket, &name).await
        }
        SessionNodeAction::Status => {
            let mut status = json!({"endpoint":cccc_core::session_nodes::endpoint(home)?,"devices":cccc_core::session_nodes::peers(home)?});
            if let Some(client) = cccc_core::session_nodes::saved_client(home)? {
                status["connector"] = json!({"hub_id":client.endpoint.endpoint_id,"node_id":client.node_id,"connected":client.owner.alive() && client.connected,"memberships":client.memberships});
            }
            print(cccc_contracts::DaemonResponse::success(
                status.as_object().expect("status object").clone(),
            ))
        }
        SessionNodeAction::Revoke { node_id } => {
            cccc_web::session_node::revoke(home, &node_id).await?;
            print(cccc_contracts::DaemonResponse::success(
                json!({"revoked":true,"node_id":node_id})
                    .as_object()
                    .expect("revoke result")
                    .clone(),
            ))
        }
    }
}

async fn ensure_daemon(home: &HomeLayout) -> Result<()> {
    home.initialize()?;
    // Start only a vacant Home. Never take over or restart a running daemon
    // just to add a network connector.
    cccc_daemon::DetachedDaemon::new(std::env::current_exe()?, ["daemon", "run"])
        .start(home)
        .await?;
    cccc_web::session_node::verify_daemon(home, false).await
}
