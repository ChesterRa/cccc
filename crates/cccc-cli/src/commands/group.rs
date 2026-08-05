use anyhow::{Result, bail};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::json;

use crate::args::{GroupAction, GroupArgs};
use crate::commands::common::{call, group, print};

pub async fn run(client: &DaemonClient, home: &HomeLayout, args: GroupArgs) -> Result<()> {
    let response = match args.action {
        GroupAction::Create { title, topic } => {
            call(
                client,
                "group_create",
                json!({"title":title,"topic":topic,"by":"user"}),
            )
            .await?
        }
        GroupAction::Show { group_id } => {
            call(client, "group_show", json!({"group_id":group_id})).await?
        }
        GroupAction::Update {
            group_id,
            title,
            topic,
            by,
        } => {
            if title.is_none() && topic.is_none() {
                bail!("--title or --topic is required");
            }
            call(
                client,
                "group_update",
                json!({"group_id":group(home,group_id)?,"title":title,"topic":topic,"by":by}),
            )
            .await?
        }
        GroupAction::DetachScope {
            scope_key,
            group_id,
            by,
        } => {
            call(
                client,
                "group_detach_scope",
                json!({"group_id":group(home,group_id)?,"scope_key":scope_key,"by":by}),
            )
            .await?
        }
        GroupAction::Delete {
            group_id,
            confirm,
            by,
        } => {
            let group_id = group(home, group_id)?;
            confirm_id(&group_id, &confirm)?;
            call(client, "group_delete", json!({"group_id":group_id,"by":by})).await?
        }
        GroupAction::Reset {
            group_id,
            confirm,
            by,
        } => {
            let group_id = group(home, group_id)?;
            confirm_id(&group_id, &confirm)?;
            call(
                client,
                "group_reset",
                json!({"group_id":group_id,"confirm":confirm,"by":by}),
            )
            .await?
        }
        GroupAction::Use { group_id, path } => {
            let attach = call(
                client,
                "attach",
                json!({"group_id":group_id,"path":path,"by":"user"}),
            )
            .await?;
            if !attach.ok {
                return print(attach);
            }
            call(client, "group_use", json!({"group_id":group_id})).await?
        }
        GroupAction::Start { group_id, by } => {
            call(
                client,
                "group_start",
                json!({"group_id":group(home,group_id)?,"by":by}),
            )
            .await?
        }
        GroupAction::Stop { group_id, by } => {
            call(
                client,
                "group_stop",
                json!({"group_id":group(home,group_id)?,"by":by}),
            )
            .await?
        }
        GroupAction::SetState {
            state,
            group_id,
            by,
        } => {
            if !matches!(state.as_str(), "active" | "idle" | "paused" | "stopped") {
                bail!("state must be active, idle, paused, or stopped");
            }
            call(
                client,
                "group_set_state",
                json!({"group_id":group(home,group_id)?,"state":state,"by":by}),
            )
            .await?
        }
    };
    print(response)
}

fn confirm_id(group_id: &str, confirm: &str) -> Result<()> {
    if group_id != confirm {
        bail!("--confirm must exactly match group_id");
    }
    Ok(())
}
