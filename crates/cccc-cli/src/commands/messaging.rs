use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::json;

use crate::args::{
    CancelReplyArgs, DeliverArgs, InboxArgs, LedgerAction, LedgerArgs, ReplyArgs, SendArgs,
    TailArgs, TrackedSendArgs,
};
use crate::commands::common::{call, group, print};

pub async fn send(client: &DaemonClient, home: &HomeLayout, args: SendArgs) -> Result<()> {
    let group_id = group(home, args.group_id)?;
    let scope_key = if args.path.trim().is_empty() {
        String::new()
    } else {
        let scope = cccc_core::scope::detect(std::path::Path::new(&args.path))?;
        let attached = cccc_core::GroupStore::new(home.clone())?
            .load(&group_id)?
            .scopes
            .iter()
            .any(|item| item.scope_key == scope.scope_key);
        anyhow::ensure!(attached, "scope not attached: {}", scope.scope_key);
        scope.scope_key
    };
    print(
        call(
            client,
            "message_send",
            json!({
                "group_id":group_id,"text":args.text,"by":sender(args.by),
                "to":args.recipients,"message_mode":args.mode.replace('-', "_"),
                "scope_key":scope_key
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
                "group_id":group(home,args.group_id)?,"text":args.text,"by":sender(args.by),
                "to":args.recipients,"task_priority":args.task_priority,"title":args.title,
                "outcome":args.outcome,
                "checklist":args.checklist.lines().filter(|line|!line.trim().is_empty())
                    .map(|line|json!({"text":line.trim()})).collect::<Vec<_>>(),
                "assignee":args.assignee,"waiting_on":args.waiting_on,"handoff_to":args.handoff_to,
                "notes":args.notes,
                "idempotency_key":args.idempotency_key
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
                "text":args.text,"by":sender(args.by),"to":args.recipients,
                "message_mode":args.mode
            }),
        )
        .await?,
    )
}

pub async fn deliver(client: &DaemonClient, home: &HomeLayout, args: DeliverArgs) -> Result<()> {
    let actor_ids = args
        .actor_ids
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        !actor_ids.is_empty(),
        "deliver requires at least one --to actor id"
    );
    print(
        call(
            client,
            "message_deliver",
            json!({
                "group_id":group(home,args.group_id)?,"source_event_id":args.event_id,
                "by":sender(args.by),"actor_ids":actor_ids,
                "force_ambiguous":args.force_ambiguous
            }),
        )
        .await?,
    )
}

pub async fn cancel_reply(
    client: &DaemonClient,
    home: &HomeLayout,
    args: CancelReplyArgs,
) -> Result<()> {
    print(
        call(
            client,
            "reply_request_cancel",
            json!({
                "group_id":group(home,args.group_id)?,"source_event_id":args.event_id,
                "by":sender(args.by)
            }),
        )
        .await?,
    )
}

pub async fn tail(client: &DaemonClient, home: &HomeLayout, args: TailArgs) -> Result<()> {
    let group_id = group(home, args.group_id)?;
    let read = || {
        call(
            client,
            "ledger_tail",
            json!({"group_id":group_id,"limit":args.limit}),
        )
    };
    if !args.follow {
        return print(read().await?);
    }
    let ledger_path = cccc_core::GroupStore::new(home.clone())?.ledger_path(&group_id)?;
    let (mut follower, _) = cccc_core::ledger::LedgerFollower::at_end(&ledger_path)?;
    let mut seen = std::collections::BTreeSet::new();
    let response = read().await?;
    if !response.ok {
        return print(response);
    }
    for event in response.result["events"].as_array().into_iter().flatten() {
        let key = event["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| event.to_string());
        if seen.insert(key) {
            println!("{}", serde_json::to_string(event)?);
        }
    }
    loop {
        for event in follower.poll(&ledger_path)? {
            let key = event.id.clone();
            if seen.insert(key) {
                println!("{}", serde_json::to_string(&event)?);
            }
        }
        tokio::select! {
            result=tokio::signal::ctrl_c()=>{ result?; return Ok(()); }
            ()=tokio::time::sleep(std::time::Duration::from_secs(1))=>{}
        }
    }
}

pub async fn inbox(client: &DaemonClient, home: &HomeLayout, args: InboxArgs) -> Result<()> {
    let group_id = group(home, args.group_id)?;
    let response = call(
        client,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":args.actor_id,
            "by":args.by,"limit":args.limit}),
    )
    .await?;
    print(response)
}

pub async fn ledger(client: &DaemonClient, home: &HomeLayout, args: LedgerArgs) -> Result<()> {
    let response = match args.action {
        LedgerAction::Snapshot {
            group_id,
            by,
            reason,
        } => {
            call(
                client,
                "ledger_snapshot",
                json!({"group_id":group(home,group_id)?,"by":by,"reason":reason}),
            )
            .await?
        }
        LedgerAction::Compact {
            group_id,
            by,
            reason,
            force,
        } => {
            call(
                client,
                "ledger_compact",
                json!({"group_id":group(home,group_id)?,"by":by,"reason":reason,"force":force}),
            )
            .await?
        }
    };
    print(response)
}

fn sender(value: Option<String>) -> String {
    value
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("CCCC_ACTOR_ID").ok())
        .unwrap_or_else(|| "user".into())
}
