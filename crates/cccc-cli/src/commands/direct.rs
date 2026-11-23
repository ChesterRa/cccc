use super::common::{call, print};
use crate::args::DirectAction;
use anyhow::{Result, ensure};
use cccc_client::DaemonClient;
use serde_json::{Value, json};

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
        DirectAction::Invite { group_id } => return invite(client, &group_id).await,
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

async fn invite(client: &DaemonClient, group_id: &str) -> Result<()> {
    let status = call(
        client,
        "connect_direct_status",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await?;
    if !status.ok {
        return print(status);
    }
    let supported = |cap: &str| {
        status
            .result
            .get("capabilities")
            .and_then(|caps| caps.get(cap))
            .and_then(Value::as_bool)
            == Some(true)
    };
    ensure!(
        supported("reusable_invitations") && supported("permanent_invitations"),
        "Update and restart the daemon to create a permanent direct invitation"
    );
    let response = call(
        client,
        "connect_direct_invite",
        json!({"group_id":group_id,"by":"user","reusable":true,"expires_in_seconds":0}),
    )
    .await?;
    if response.ok {
        let received = response
            .result
            .get("invitation")
            .and_then(Value::as_str)
            .and_then(|text| cccc_core::direct::decode_invitation(text).ok());
        ensure!(
            received.is_some_and(|key| key.permanent() && key.host.group_id == group_id),
            "The daemon did not return a permanent invitation for this Group. Check existing invitations before trying again"
        );
    }
    print(response)
}
