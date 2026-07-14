use anyhow::{Context, Result};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Value, json};

use crate::args::{ImAction, ImArgs, ImSetArgs, PromptArgs, SpaceAction, SpaceArgs};
use crate::commands::common::{call, group, print};

pub async fn prompt(client: &DaemonClient, home: &HomeLayout, args: PromptArgs) -> Result<()> {
    print(
        call(
            client,
            "actor_prompt",
            json!({"group_id":group(home,args.group_id)?,"actor_id":args.actor_id}),
        )
        .await?,
    )
}

pub async fn im(client: &DaemonClient, home: &HomeLayout, args: ImArgs) -> Result<()> {
    let (op, value) = match args.action {
        ImAction::Set(args) => {
            let ImSetArgs {
                platform,
                group_id,
                token_env,
                bot_token_env,
                app_token_env,
                app_key_env,
                app_secret_env,
                domain,
                robot_code_env,
                wecom_bot_id,
                wecom_secret,
                weixin_account_id,
            } = *args;
            (
                "im_set",
                json!({
                    "group_id":group(home,group_id)?,"platform":platform,"token_env":token_env,
                    "bot_token_env":bot_token_env,"app_token_env":app_token_env,
                    "app_key_env":app_key_env,"app_secret_env":app_secret_env,"domain":domain,
                    "robot_code_env":robot_code_env,"wecom_bot_id":wecom_bot_id,
                    "wecom_secret":wecom_secret,"weixin_account_id":weixin_account_id
                }),
            )
        }
        ImAction::Unset { group_id } => ("im_unset", group_value(home, group_id)?),
        ImAction::Config { group_id } => ("im_config", group_value(home, group_id)?),
        ImAction::Start { group_id } => ("im_start", group_value(home, group_id)?),
        ImAction::Stop { group_id } => ("im_stop", group_value(home, group_id)?),
        ImAction::Status { group_id } => ("im_status", group_value(home, group_id)?),
        ImAction::Bind { key, group_id } => (
            "im_bind_chat",
            json!({"group_id":group(home,group_id)?,"key":key}),
        ),
        ImAction::Pending { group_id } => ("im_list_pending", group_value(home, group_id)?),
        ImAction::Authorized { group_id } => ("im_list_authorized", group_value(home, group_id)?),
        ImAction::Reject { key, group_id } => (
            "im_reject_pending",
            json!({"group_id":group(home,group_id)?,"key":key}),
        ),
        ImAction::Revoke {
            chat_id,
            thread_id,
            group_id,
        } => (
            "im_revoke_chat",
            json!({"group_id":group(home,group_id)?,"chat_id":chat_id,"thread_id":thread_id}),
        ),
    };
    print(call(client, op, value).await?)
}

pub async fn space(client: &DaemonClient, home: &HomeLayout, args: SpaceArgs) -> Result<()> {
    let (op, value) = match args.action {
        SpaceAction::Status { group_id, provider } => (
            "group_space_status",
            json!({"group_id":group(home,group_id)?,"provider":provider}),
        ),
        SpaceAction::Bind {
            remote_space_id,
            group_id,
            lane,
            provider,
        } => (
            "group_space_bind",
            json!({"group_id":group(home,group_id)?,"provider":provider,"lane":lane,"remote_space_id":remote_space_id,"action":"bind"}),
        ),
        SpaceAction::Unbind {
            group_id,
            lane,
            provider,
        } => (
            "group_space_bind",
            json!({"group_id":group(home,group_id)?,"provider":provider,"lane":lane,"action":"unbind"}),
        ),
        SpaceAction::Sync {
            group_id,
            lane,
            provider,
            force,
        } => (
            "group_space_sync",
            json!({"group_id":group(home,group_id)?,"provider":provider,"lane":lane,"force":force}),
        ),
        SpaceAction::Ingest {
            group_id,
            lane,
            kind,
            payload,
            idempotency_key,
        } => (
            "group_space_ingest",
            json!({"group_id":group(home,group_id)?,"lane":lane,"kind":kind,"payload":json_object(&payload,"payload")?,"idempotency_key":idempotency_key}),
        ),
        SpaceAction::Query {
            query,
            group_id,
            lane,
            options,
        } => (
            "group_space_query",
            json!({"group_id":group(home,group_id)?,"lane":lane,"query":query,"options":json_object(&options,"options")?}),
        ),
        SpaceAction::Sources {
            group_id,
            lane,
            action,
            source_id,
            new_title,
        } => (
            "group_space_sources",
            json!({"group_id":group(home,group_id)?,"lane":lane,"action":action,"source_id":source_id,"new_title":new_title}),
        ),
        SpaceAction::Jobs {
            group_id,
            lane,
            action,
            job_id,
        } => (
            "group_space_jobs",
            json!({"group_id":group(home,group_id)?,"lane":lane,"action":action,"job_id":job_id}),
        ),
        SpaceAction::Auth { action, provider } => (
            "group_space_provider_auth",
            json!({"provider":provider,"action":action}),
        ),
    };
    print(call(client, op, value).await?)
}

fn group_value(home: &HomeLayout, group_id: Option<String>) -> Result<Value> {
    Ok(json!({"group_id":group(home,group_id)?}))
}

fn json_object(value: &str, name: &str) -> Result<Value> {
    let parsed: Value =
        serde_json::from_str(value).with_context(|| format!("invalid {name} JSON"))?;
    if !parsed.is_object() {
        anyhow::bail!("{name} must be a JSON object");
    }
    Ok(parsed)
}
