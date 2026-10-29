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
            let group_id = group(home, group_id)?;
            call(
                client,
                "group_update",
                group_update_request(&group_id, &by, title, topic),
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
            let path = std::path::absolute(path)?;
            call(
                client,
                "group_use",
                group_use_request(&group_id, &path.to_string_lossy()),
            )
            .await?
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
            let (op, args) = group_state_request(&group(home, group_id)?, &state, &by)?;
            call(client, op, args).await?
        }
    };
    print(response)
}

fn group_state_request(
    group_id: &str,
    state: &str,
    by: &str,
) -> Result<(&'static str, serde_json::Value)> {
    if state == "stopped" {
        return Ok(("group_stop", json!({"group_id":group_id,"by":by})));
    }
    if !matches!(state, "active" | "idle" | "paused") {
        bail!("state must be active, idle, paused, or stopped");
    }
    Ok((
        "group_set_state",
        json!({"group_id":group_id,"state":state,"by":by}),
    ))
}

fn group_update_request(
    group_id: &str,
    by: &str,
    title: Option<String>,
    topic: Option<String>,
) -> serde_json::Value {
    let mut patch = serde_json::Map::new();
    if let Some(title) = title {
        patch.insert("title".into(), json!(title));
    }
    if let Some(topic) = topic {
        patch.insert("topic".into(), json!(topic));
    }
    json!({"group_id":group_id,"patch":patch,"by":by})
}

fn group_use_request(group_id: &str, path: &str) -> serde_json::Value {
    json!({"group_id":group_id,"path":path,"by":"user"})
}

fn confirm_id(group_id: &str, confirm: &str) -> Result<()> {
    if group_id != confirm {
        bail!("--confirm must exactly match group_id");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{group_state_request, group_update_request, group_use_request};
    use serde_json::json;

    #[test]
    fn stopped_uses_real_group_stop_and_keeps_the_author() {
        let (op, args) =
            group_state_request("g_test", "stopped", "foreman").expect("stopped alias");
        assert_eq!(op, "group_stop");
        assert_eq!(args, json!({"group_id":"g_test","by":"foreman"}));
        for state in ["active", "idle", "paused"] {
            assert_eq!(
                group_state_request("g_test", state, "user")
                    .expect("delivery state")
                    .0,
                "group_set_state"
            );
        }
        assert!(group_state_request("g_test", "unknown", "user").is_err());
    }

    #[test]
    fn group_update_emits_only_the_canonical_patch_shape() {
        let request =
            group_update_request("g_test", "user", Some("title".into()), Some(String::new()));
        assert_eq!(
            request,
            json!({
                "group_id":"g_test",
                "by":"user",
                "patch":{"title":"title","topic":""}
            })
        );
        assert!(request.get("title").is_none());
        assert!(request.get("topic").is_none());
    }

    #[test]
    fn group_use_emits_the_canonical_attached_path_shape() {
        assert_eq!(
            group_use_request("g_test", "/workspace/project"),
            json!({
                "group_id":"g_test",
                "path":"/workspace/project",
                "by":"user"
            })
        );
    }
}
