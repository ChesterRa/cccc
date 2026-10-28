use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct SendArgs {
    /// Message text.
    pub text: String,
    /// Target Group ID; omit to use the current Group.
    #[arg(long = "group")]
    pub group_id: Option<String>,
    /// Sender identity; defaults to CCCC_ACTOR_ID, then user.
    #[arg(long)]
    pub by: Option<String>,
    #[arg(long = "to")]
    /// Agent recipients/selectors or user. One message cannot mix both domains.
    pub recipients: Vec<String>,
    #[arg(long, default_value = "send", value_parser = ["send", "request-reply", "mail"])]
    /// Delivery mode. Mail is available only for agent recipients.
    pub mode: String,
    #[arg(
        long = "dst-instance",
        requires = "dst_group_id",
        conflicts_with = "path"
    )]
    /// Exact remote instance ID from `cccc connect`; always pair with --dst-group.
    pub dst_instance_id: Option<String>,
    #[arg(long = "dst-group", requires = "dst_instance_id")]
    /// Remote Group ID, qualified by --dst-instance.
    pub dst_group_id: Option<String>,
    #[arg(long)]
    /// Visible perspective accompanying this message.
    pub insight: Option<String>,
    #[arg(long)]
    /// Stable retry key; reuse with unchanged content after an uncertain response.
    pub idempotency_key: Option<String>,
    /// Project path used to choose the message scope.
    #[arg(long, default_value = "")]
    pub path: String,
}

#[derive(Debug, Args)]
pub struct TrackedSendArgs {
    /// Message text.
    pub text: String,
    /// Target Group ID; omit to use the current Group.
    #[arg(long = "group")]
    pub group_id: Option<String>,
    /// Sender identity; defaults to CCCC_ACTOR_ID, then user.
    #[arg(long)]
    pub by: Option<String>,
    #[arg(long = "to")]
    /// Recipients. One message cannot mix user and agents.
    pub recipients: Vec<String>,
    /// Priority of the linked task.
    #[arg(long, default_value = "normal")]
    pub task_priority: String,
    /// Title of the linked task.
    #[arg(long)]
    pub title: String,
    /// Expected outcome of the linked task.
    #[arg(long, default_value = "")]
    pub outcome: String,
    /// Task checklist items, one per line.
    #[arg(long, default_value = "")]
    pub checklist: String,
    /// Actor ID assigned to the linked task.
    #[arg(long, default_value = "")]
    pub assignee: String,
    /// Who the linked task is waiting on.
    #[arg(long, default_value = "")]
    pub waiting_on: String,
    /// Actor ID for the next task handoff.
    #[arg(long, default_value = "")]
    pub handoff_to: String,
    /// Notes for the linked task.
    #[arg(long, default_value = "")]
    pub notes: String,
    /// Stable request key; reuse only with the same content.
    #[arg(long, default_value = "")]
    pub idempotency_key: String,
}

#[derive(Debug, Args)]
pub struct ReplyArgs {
    /// Event ID of the message to reply to.
    pub reply_to: String,
    /// Message text.
    pub text: String,
    /// Target Group ID; omit to use the current Group.
    #[arg(long = "group")]
    pub group_id: Option<String>,
    /// Sender identity; defaults to CCCC_ACTOR_ID, then user.
    #[arg(long)]
    pub by: Option<String>,
    /// Reply recipients: Actor IDs/selectors or user; omit to retain the original audience.
    #[arg(long = "to")]
    pub recipients: Vec<String>,
    #[arg(long)]
    /// Visible perspective accompanying this reply.
    pub insight: Option<String>,
    #[arg(long)]
    /// Stable retry key for this reply.
    pub idempotency_key: Option<String>,
    #[arg(long, default_value = "send", value_parser = ["send", "mail"])]
    /// Reply delivery mode. Mail is available only for agent recipients.
    pub mode: String,
}

#[derive(Debug, Args)]
pub struct DeliverArgs {
    /// Event ID of the message.
    pub event_id: String,
    /// Target Group ID; omit to use the current Group.
    #[arg(long = "group")]
    pub group_id: Option<String>,
    /// Sender identity; defaults to CCCC_ACTOR_ID, then user.
    #[arg(long)]
    pub by: Option<String>,
    /// Recipient Actor IDs to deliver to; repeat for multiple Actors.
    #[arg(long = "to", required = true)]
    pub actor_ids: Vec<String>,
    /// Explicitly allow redelivery when a previous attempt may already have arrived.
    #[arg(long)]
    pub force_ambiguous: bool,
}

#[derive(Debug, Args)]
pub struct CancelReplyArgs {
    /// Event ID of the message.
    pub event_id: String,
    /// Target Group ID; omit to use the current Group.
    #[arg(long = "group")]
    pub group_id: Option<String>,
    /// Sender identity; defaults to CCCC_ACTOR_ID, then user.
    #[arg(long)]
    pub by: Option<String>,
}

#[derive(Debug, Args)]
pub struct TailArgs {
    /// Target Group ID; omit to use the current Group.
    #[arg(long = "group")]
    pub group_id: Option<String>,
    /// Maximum number of items to return.
    #[arg(short = 'n', long = "lines", alias = "limit", default_value_t = 50)]
    pub limit: u64,
    /// Keep following new entries until interrupted.
    #[arg(short = 'f', long)]
    pub follow: bool,
}

#[derive(Debug, Args)]
pub struct InboxArgs {
    /// Target Group ID; omit to use the current Group.
    #[arg(long = "group")]
    pub group_id: Option<String>,
    /// Actor ID within the Group.
    #[arg(long)]
    pub actor_id: String,
    /// Maximum number of items to return.
    #[arg(long, default_value_t = 50)]
    pub limit: u64,
    /// Author identity: user or an Actor ID.
    #[arg(long, default_value = "user")]
    pub by: String,
}

#[derive(Debug, Args)]
pub struct LedgerArgs {
    #[command(subcommand)]
    pub action: LedgerAction,
}

#[derive(Debug, Subcommand)]
pub enum LedgerAction {
    /// Record a snapshot of the Group's ledger with counts and a hash.
    Snapshot {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
        /// Reason recorded with this ledger operation.
        #[arg(long, default_value = "manual")]
        reason: String,
    },
    /// Snapshot and rotate the active ledger into a retained segment.
    Compact {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
        /// Reason recorded with this ledger operation.
        #[arg(long, default_value = "manual")]
        reason: String,
        /// Accepted for compatibility; the current daemon does not use this flag.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Args)]
pub struct ConnectArgs {
    #[arg(long = "group")]
    /// Your local Group; defaults to the current Actor's Group or active Group.
    pub group_id: Option<String>,
    /// Sender identity; defaults to CCCC_ACTOR_ID, then user.
    #[arg(long)]
    pub by: Option<String>,
    #[arg(long)]
    /// Remote instance from the directory. Omit to list accessible connections.
    pub instance: Option<String>,
    #[arg(long, requires = "instance")]
    /// Exact remote Group; required for a Group-scoped connection.
    pub target_group: Option<String>,
    /// Directory cursor returned by the previous page.
    #[arg(long, requires = "instance")]
    pub after: Option<String>,
    /// Maximum number of items to return.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=64))]
    pub limit: u32,
}
