use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct PromptArgs {
    pub actor_id: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct ImArgs {
    #[command(subcommand)]
    pub action: ImAction,
}

#[derive(Debug, Subcommand)]
pub enum ImAction {
    Set(Box<ImSetArgs>),
    Unset {
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Config {
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Start {
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Stop {
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Status {
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Bind {
        #[arg(long)]
        key: String,
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Pending {
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Authorized {
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Reject {
        #[arg(long)]
        key: String,
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Revoke {
        #[arg(long)]
        chat_id: String,
        #[arg(long, default_value_t = 0)]
        thread_id: i64,
        #[arg(long = "group")]
        group_id: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct ImSetArgs {
    pub platform: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long)]
    pub token_env: Option<String>,
    #[arg(long)]
    pub bot_token_env: Option<String>,
    #[arg(long)]
    pub app_token_env: Option<String>,
    #[arg(long)]
    pub app_key_env: Option<String>,
    #[arg(long)]
    pub app_secret_env: Option<String>,
    #[arg(long)]
    pub domain: Option<String>,
    #[arg(long)]
    pub robot_code_env: Option<String>,
    #[arg(long)]
    pub wecom_bot_id: Option<String>,
    #[arg(long)]
    pub wecom_secret: Option<String>,
    #[arg(long)]
    pub weixin_account_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct SpaceArgs {
    #[command(subcommand)]
    pub action: SpaceAction,
}

#[derive(Debug, Subcommand)]
pub enum SpaceAction {
    Status {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long, default_value = "notebooklm")]
        provider: String,
    },
    Bind {
        #[arg(default_value = "")]
        remote_space_id: String,
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        lane: String,
        #[arg(long, default_value = "notebooklm")]
        provider: String,
    },
    Unbind {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        lane: String,
        #[arg(long, default_value = "notebooklm")]
        provider: String,
    },
    Sync {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        lane: String,
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        #[arg(long)]
        force: bool,
    },
    Ingest {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        lane: String,
        #[arg(long, default_value = "context_sync")]
        kind: String,
        #[arg(long, default_value = "{}")]
        payload: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    Query {
        query: String,
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        lane: String,
        #[arg(long, default_value = "{}")]
        options: String,
    },
    Sources {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        lane: String,
        #[arg(long, default_value = "list")]
        action: String,
        #[arg(long)]
        source_id: Option<String>,
        #[arg(long)]
        new_title: Option<String>,
    },
    Jobs {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        lane: String,
        #[arg(long, default_value = "list")]
        action: String,
        #[arg(long)]
        job_id: Option<String>,
    },
    Auth {
        #[arg(default_value = "status")]
        action: String,
        #[arg(long, default_value = "notebooklm")]
        provider: String,
    },
}
