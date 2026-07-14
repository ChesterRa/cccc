use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::json;

use crate::args::{
    InboxArgs, LedgerAction, LedgerArgs, ReadArgs, ReplyArgs, SendArgs, TailArgs, TrackedSendArgs,
};
use crate::commands::common::{call, group, print};

pub async fn send(client: &DaemonClient, home: &HomeLayout, args: SendArgs) -> Result<()> {
    print(
        call(
            client,
            "message_send",
            json!({
                "group_id":group(home,args.group_id)?,"text":args.text,"by":args.by,
                "to":args.recipients,"priority":args.priority,"reply_required":args.reply_required
            }),
        )
        .await?,
    )
}

pub async fn tracked(
    client: &DaemonClient,
    home: &HomeLayout,
    args: TrackedSendArgs,
) -> Result<()> {
    print(
        call(
            client,
            "tracked_send",
            json!({
                "group_id":group(home,args.group_id)?,"text":args.text,"by":args.by,
                "to":args.recipients,"priority":args.priority
            }),
        )
        .await?,
    )
}

pub async fn reply(client: &DaemonClient, home: &HomeLayout, args: ReplyArgs) -> Result<()> {
    print(
        call(
            client,
            "reply",
            json!({
                "group_id":group(home,args.group_id)?,"reply_to":args.reply_to,
                "text":args.text,"by":args.by
            }),
        )
        .await?,
    )
}

pub async fn tail(client: &DaemonClient, home: &HomeLayout, args: TailArgs) -> Result<()> {
    print(
        call(
            client,
            "ledger_tail",
            json!({"group_id":group(home,args.group_id)?,"limit":args.limit}),
        )
        .await?,
    )
}

pub async fn inbox(client: &DaemonClient, home: &HomeLayout, args: InboxArgs) -> Result<()> {
    print(call(client, "inbox_list", json!({"group_id":group(home,args.group_id)?,"actor_id":args.actor_id,"limit":args.limit})).await?)
}

pub async fn read(client: &DaemonClient, home: &HomeLayout, args: ReadArgs) -> Result<()> {
    print(call(client, "inbox_mark_read", json!({"group_id":group(home,args.group_id)?,"actor_id":args.actor_id,"event_id":args.event_id})).await?)
}

pub async fn ledger(client: &DaemonClient, home: &HomeLayout, args: LedgerArgs) -> Result<()> {
    let response = match args.action {
        LedgerAction::Snapshot { group_id } => {
            call(
                client,
                "ledger_snapshot",
                json!({"group_id":group(home,group_id)?}),
            )
            .await?
        }
        LedgerAction::Compact { group_id, dry_run } => {
            call(
                client,
                "ledger_compact",
                json!({"group_id":group(home,group_id)?,"dry_run":dry_run}),
            )
            .await?
        }
    };
    print(response)
}
