use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct SendArgs {
    pub text: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long, default_value = "user")]
    pub by: String,
    #[arg(long = "to")]
    pub recipients: Vec<String>,
    #[arg(long, default_value = "normal")]
    pub priority: String,
    #[arg(long)]
    pub reply_required: bool,
}

#[derive(Debug, Args)]
pub struct TrackedSendArgs {
    pub text: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long, default_value = "user")]
    pub by: String,
    #[arg(long = "to")]
    pub recipients: Vec<String>,
    #[arg(long, default_value = "normal")]
    pub priority: String,
}

#[derive(Debug, Args)]
pub struct ReplyArgs {
    pub reply_to: String,
    pub text: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long, default_value = "user")]
    pub by: String,
}

#[derive(Debug, Args)]
pub struct TailArgs {
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(short = 'n', long, default_value_t = 50)]
    pub limit: u64,
}

#[derive(Debug, Args)]
pub struct InboxArgs {
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long)]
    pub actor_id: String,
    #[arg(long, default_value_t = 50)]
    pub limit: u64,
}

#[derive(Debug, Args)]
pub struct ReadArgs {
    pub event_id: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long)]
    pub actor_id: String,
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
    },
    Compact {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
}
