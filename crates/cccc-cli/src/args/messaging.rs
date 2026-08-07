use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct SendArgs {
    pub text: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long)]
    pub by: Option<String>,
    #[arg(long = "to")]
    pub recipients: Vec<String>,
    #[arg(long, default_value = "normal")]
    pub priority: String,
    #[arg(long)]
    pub reply_required: bool,
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
    pub recipients: Vec<String>,
    #[arg(long, default_value = "normal")]
    pub priority: String,
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
    #[arg(long)]
    pub no_reply_required: bool,
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
    #[arg(long, default_value = "normal")]
    pub priority: String,
    #[arg(long)]
    pub reply_required: bool,
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
    #[arg(long, default_value = "all")]
    pub kind_filter: String,
    #[arg(long)]
    pub mark_read: bool,
}

#[derive(Debug, Args)]
pub struct ReadArgs {
    pub event_id: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long)]
    pub actor_id: String,
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
