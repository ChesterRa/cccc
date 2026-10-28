use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ActorArgs {
    #[command(subcommand)]
    pub action: ActorAction,
}

#[derive(Debug, Subcommand)]
pub enum ActorAction {
    /// List Actors and their effective workspaces.
    List {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// Add an Actor to a Group.
    Add {
        /// Actor ID within the Group.
        actor_id: String,
        /// Display title.
        #[arg(long, default_value = "")]
        title: String,
        /// Agent runtime to configure or launch.
        #[arg(long, default_value = "codex")]
        runtime: String,
        /// Runtime adapter mode: default or acp.
        #[arg(long, default_value = "default", value_parser = ["default", "acp"])]
        runtime_mode: String,
        /// Launch command with arguments, parsed as one quoted command line.
        #[arg(long, default_value = "")]
        command: String,
        /// Runtime environment entry KEY=VALUE; repeat for multiple entries.
        #[arg(long = "env")]
        env: Vec<String>,
        /// Attached project directory for the default workspace; empty inherits the Group scope.
        #[arg(long, default_value = "")]
        scope: String,
        /// Terminal submission: enter, newline or none.
        #[arg(long, default_value = "enter")]
        submit: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Remove an Actor from the Group.
    Remove(ActorTarget),
    /// Start a managed Actor, or restore native Group access.
    Start(ActorTarget),
    /// Stop a managed Actor, or pause native Group access.
    Stop(ActorTarget),
    /// Restart a managed Actor.
    Restart(ActorTarget),
    /// Update Actor settings; running managed sessions apply launch settings on restart.
    Update {
        /// Actor ID within the Group.
        actor_id: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Display title.
        #[arg(long)]
        title: Option<String>,
        /// Agent runtime to configure or launch.
        #[arg(long)]
        runtime: Option<String>,
        /// Runtime adapter mode: default or acp.
        #[arg(long, value_parser = ["default", "acp"])]
        runtime_mode: Option<String>,
        /// Attached project directory for the default workspace; empty inherits the Group scope.
        #[arg(long)]
        scope: Option<String>,
        /// Launch command with arguments, parsed as one quoted command line.
        #[arg(long)]
        command: Option<String>,
        /// Runtime environment entry KEY=VALUE; repeat for multiple entries.
        #[arg(long = "env")]
        env: Vec<String>,
        /// Terminal submission: enter, newline or none.
        #[arg(long)]
        submit: Option<String>,
        /// Enable or disable this Actor.
        #[arg(long)]
        enabled: Option<bool>,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Manage runtime-only secrets without writing them to Actor configuration.
    Secrets {
        /// Actor ID within the Group.
        actor_id: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Set a runtime-only secret KEY=VALUE; repeat for multiple secrets.
        #[arg(long = "set")]
        set: Vec<String>,
        /// Remove a runtime-only secret by key; repeat for multiple keys.
        #[arg(long = "unset")]
        unset: Vec<String>,
        /// Remove all runtime-only secrets for this Actor.
        #[arg(long)]
        clear: bool,
        /// List secret keys without their values.
        #[arg(long)]
        keys: bool,
        /// Restart the Actor after updating its secrets.
        #[arg(long)]
        restart: bool,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
}

#[derive(Debug, Args)]
pub struct ActorTarget {
    /// Actor ID within the Group.
    pub actor_id: String,
    /// Target Group ID; omit to use the current Group.
    #[arg(long = "group")]
    pub group_id: Option<String>,
    /// Author identity: user or an Actor ID.
    #[arg(long, default_value = "user")]
    pub by: String,
}
