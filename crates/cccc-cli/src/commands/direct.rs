use super::common::{call, print};
use crate::args::DirectAction;
use anyhow::Result;
use cccc_client::DaemonClient;
use serde_json::json;

pub async fn run(client: &DaemonClient, action: DirectAction) -> Result<()> {
    let (action, mut args) = match action {
        DirectAction::Status { group_id } => ("status", json!({"group_id":group_id})),
        DirectAction::Listen {
            bind,
            address,
            name,
        } => (
            "configure",
            json!({"listener":{"bind":bind,"address":address},"display_name":name}),
        ),
        DirectAction::Stop => ("configure", json!({"listener":null})),
        DirectAction::Invite { group_id } => ("invite", json!({"group_id":group_id})),
        DirectAction::Join { group_id } => {
            use tokio::io::AsyncReadExt;
            let mut input = String::new();
            tokio::io::stdin()
                .take(8193)
                .read_to_string(&mut input)
                .await?;
            anyhow::ensure!(input.len() <= 8192, "Invitation is too large");
            (
                "join",
                json!({"group_id":group_id,"invitation":input.trim()}),
            )
        }
        DirectAction::Approve { group_id, id } => ("approve", json!({"group_id":group_id,"id":id})),
        DirectAction::Revoke { group_id, id } => ("revoke", json!({"group_id":group_id,"id":id})),
        DirectAction::Remove { group_id, id } => ("remove", json!({"group_id":group_id,"id":id})),
    };
    args["by"] = json!("user");
    print(call(client, &format!("connect_direct_{action}"), args).await?)
}
