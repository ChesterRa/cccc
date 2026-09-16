use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct SendArgs {
    pub text: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
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
    #[arg(long, default_value = "")]
    pub path: String,
}

#[derive(Debug, Args)]
pub struct TrackedSendArgs {
    pub text: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long)]
    pub by: Option<String>,
    #[arg(long = "to")]
    /// Recipients. One message cannot mix user and agents.
    pub recipients: Vec<String>,
    #[arg(long, default_value = "normal")]
    pub task_priority: String,
    #[arg(long)]
    pub title: String,
    #[arg(long, default_value = "")]
    pub outcome: String,
    #[arg(long, default_value = "")]
    pub checklist: String,
    #[arg(long, default_value = "")]
    pub assignee: String,
    #[arg(long, default_value = "")]
    pub waiting_on: String,
    #[arg(long, default_value = "")]
    pub handoff_to: String,
    #[arg(long, default_value = "")]
    pub notes: String,
    #[arg(long, default_value = "")]
    pub idempotency_key: String,
}

#[derive(Debug, Args)]
pub struct ReplyArgs {
    pub reply_to: String,
    pub text: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long)]
    pub by: Option<String>,
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
    pub event_id: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long)]
    pub by: Option<String>,
    #[arg(long = "to", required = true)]
    pub actor_ids: Vec<String>,
    #[arg(long)]
    pub force_ambiguous: bool,
}

#[derive(Debug, Args)]
pub struct CancelReplyArgs {
    pub event_id: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long)]
    pub by: Option<String>,
}

#[derive(Debug, Args)]
pub struct TailArgs {
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(short = 'n', long = "lines", alias = "limit", default_value_t = 50)]
    pub limit: u64,
    #[arg(short = 'f', long)]
    pub follow: bool,
}

#[derive(Debug, Args)]
pub struct InboxArgs {
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long)]
    pub actor_id: String,
    #[arg(long, default_value_t = 50)]
    pub limit: u64,
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
    Snapshot {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long, default_value = "user")]
        by: String,
        #[arg(long, default_value = "manual")]
        reason: String,
    },
    Compact {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long, default_value = "user")]
        by: String,
        #[arg(long, default_value = "manual")]
        reason: String,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Args)]
pub struct ConnectArgs {
    #[arg(long = "group")]
    /// Your local Group; defaults to the current Actor's Group or active Group.
    pub group_id: Option<String>,
    #[arg(long)]
    pub by: Option<String>,
    #[arg(long)]
    /// Remote instance from the directory. Omit to list accessible connections.
    pub instance: Option<String>,
    #[arg(long, requires = "instance")]
    /// Exact remote Group; required for a Group-scoped connection.
    pub target_group: Option<String>,
    #[arg(long, requires = "instance")]
    pub after: Option<String>,
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=64))]
    pub limit: u32,
}
