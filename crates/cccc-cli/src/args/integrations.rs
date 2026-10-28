use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct PromptArgs {
    /// Legacy positional spelling retained for 0.4.35 command compatibility.
    #[arg(value_name = "ACTOR_ID")]
    pub legacy_actor_id: Option<String>,
    /// Actor ID within the Group.
    #[arg(long = "actor-id")]
    pub actor_id: Option<String>,
    /// Target Group ID; omit to use the current Group.
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
    /// Configure an IM bridge for the Group.
    Set(Box<ImSetArgs>),
    /// Remove the IM bridge configuration.
    Unset {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// Show the IM bridge configuration.
    Config {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// Start the Group IM bridge.
    Start {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// Stop the Group IM bridge.
    Stop {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// Show the IM bridge status.
    Status {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// Approve a pending IM chat binding.
    Bind {
        /// Pending IM binding key.
        #[arg(long)]
        key: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// List pending IM chat bindings.
    Pending {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// List authorized IM chats.
    Authorized {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// Reject a pending IM chat binding.
    Reject {
        /// Pending IM binding key.
        #[arg(long)]
        key: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// Revoke an authorized IM chat or thread.
    Revoke {
        /// IM chat ID to revoke.
        #[arg(long)]
        chat_id: String,
        /// IM thread ID within the chat.
        #[arg(long, default_value = "0")]
        thread_id: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// Show the IM bridge logs.
    Logs {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Number of recent log lines to show.
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
        /// Keep following new entries until interrupted.
        #[arg(short = 'f', long)]
        follow: bool,
    },
}

#[derive(Debug, Args)]
pub struct ImSetArgs {
    /// IM platform to configure.
    pub platform: String,
    /// Target Group ID; omit to use the current Group.
    #[arg(long = "group")]
    pub group_id: Option<String>,
    /// Name of the environment variable holding the platform token.
    #[arg(long)]
    pub token_env: Option<String>,
    /// Name of the environment variable holding the bot token.
    #[arg(long)]
    pub bot_token_env: Option<String>,
    /// Name of the environment variable holding the application token.
    #[arg(long)]
    pub app_token_env: Option<String>,
    /// Name of the environment variable holding the application key.
    #[arg(long)]
    pub app_key_env: Option<String>,
    /// Name of the environment variable holding the application secret.
    #[arg(long)]
    pub app_secret_env: Option<String>,
    /// Platform API domain.
    #[arg(long)]
    pub domain: Option<String>,
    /// Mattermost server URL.
    #[arg(long)]
    pub mattermost_url: Option<String>,
    /// Name of the environment variable holding the DingTalk robot code.
    #[arg(long)]
    pub robot_code_env: Option<String>,
    /// DingTalk robot code.
    #[arg(long)]
    pub robot_code: Option<String>,
    /// WeCom bot ID.
    #[arg(long)]
    pub wecom_bot_id: Option<String>,
    /// WeCom bot secret.
    #[arg(long)]
    pub wecom_secret: Option<String>,
    /// Weixin account ID.
    #[arg(long)]
    pub weixin_account_id: Option<String>,
    /// Platform access token.
    #[arg(long)]
    pub token: Option<String>,
}

#[derive(Debug, Args)]
pub struct SpaceArgs {
    #[command(subcommand)]
    pub action: SpaceAction,
}

#[derive(Debug, Subcommand)]
pub enum SpaceAction {
    /// Show Group Space bindings and provider state.
    Status {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
    },
    /// Bind a lane to a remote notebook, creating one when no ID is given.
    Bind {
        /// Remote notebook or space ID.
        #[arg(default_value = "")]
        remote_space_id: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Group Space lane: work or memory.
        #[arg(long)]
        lane: String,
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Remove a Group lane binding.
    Unbind {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Group Space lane: work or memory.
        #[arg(long)]
        lane: String,
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Submit content to a bound lane.
    Ingest {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Group Space lane: work or memory.
        #[arg(long)]
        lane: String,
        /// Ingestion kind: context_sync or resource_ingest.
        #[arg(long, default_value = "context_sync")]
        kind: String,
        /// Ingestion payload as a JSON object.
        #[arg(long, default_value = "{}")]
        payload: String,
        /// Stable request key; reuse only with the same content.
        #[arg(long)]
        idempotency_key: Option<String>,
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Query the knowledge in a bound lane.
    Query {
        /// Question to ask the bound Group Space.
        query: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Group Space lane: work or memory.
        #[arg(long)]
        lane: String,
        /// Query options as a JSON object; only source_ids is supported.
        #[arg(long, default_value = "{}")]
        options: String,
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
    },
    /// List or manage sources in a bound lane.
    Sources {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Group Space lane: work or memory.
        #[arg(long)]
        lane: String,
        /// Source operation: list, refresh, rename or delete.
        #[arg(long, default_value = "list")]
        action: String,
        /// Source ID to operate on.
        #[arg(long)]
        source_id: Option<String>,
        /// New title for the selected source.
        #[arg(long)]
        new_title: Option<String>,
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
    },
    /// Inspect and control Group Space jobs.
    Jobs {
        #[command(subcommand)]
        action: SpaceJobsAction,
    },
    /// Manage provider login sessions.
    Auth {
        #[command(subcommand)]
        action: SpaceAuthAction,
    },
    /// Manage the provider authentication data.
    Credential {
        #[command(subcommand)]
        action: SpaceCredentialAction,
    },
    /// Check Group Space provider health.
    Health {
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SpaceJobsAction {
    /// List ingestion jobs for a Group lane.
    List {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Group Space lane: work or memory.
        #[arg(long)]
        lane: String,
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Filter jobs by state.
        #[arg(long)]
        state: Option<String>,
        /// Maximum number of items to return.
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u64).range(1..=500))]
        limit: u64,
    },
    /// Retry a Group Space job.
    Retry {
        /// Group Space job ID.
        job_id: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Group Space lane: work or memory.
        #[arg(long)]
        lane: String,
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Cancel a Group Space job.
    Cancel {
        /// Group Space job ID.
        job_id: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Group Space lane: work or memory.
        #[arg(long)]
        lane: String,
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SpaceAuthAction {
    /// Show provider login status.
    Status {
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Start an interactive provider login.
    Start {
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
        /// Maximum time to wait for provider login.
        #[arg(long, default_value_t = 900, value_parser = clap::value_parser!(u64).range(60..=1800))]
        timeout_seconds: u64,
        /// Start a new provider login even if an existing session is available.
        #[arg(long)]
        force_reauth: bool,
    },
    /// Cancel a provider login in progress.
    Cancel {
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Disconnect the provider login session.
    Disconnect {
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SpaceCredentialAction {
    /// Show whether provider authentication data is available.
    Status {
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Set provider authentication data.
    Set {
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Provider authentication as JSON.
        #[arg(long, conflicts_with = "auth_json_file")]
        auth_json: Option<String>,
        /// Read provider authentication JSON from this file.
        #[arg(long, conflicts_with = "auth_json")]
        auth_json_file: Option<String>,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Remove provider authentication data.
    Clear {
        /// Group Space provider.
        #[arg(long, default_value = "notebooklm")]
        provider: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
}
