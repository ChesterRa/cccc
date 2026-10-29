use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct GroupArgs {
    #[command(subcommand)]
    pub action: GroupAction,
}

#[derive(Debug, Subcommand)]
pub enum GroupAction {
    /// Create a Group.
    Create {
        /// Display title.
        #[arg(long, default_value = "working-group")]
        title: String,
        /// Short description of the Group purpose.
        #[arg(long, default_value = "")]
        topic: String,
    },
    /// Show a Group and its current runtime state.
    Show {
        /// Target Group ID.
        group_id: String,
    },
    /// Update the Group title or topic.
    Update {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Display title.
        #[arg(long)]
        title: Option<String>,
        /// Short description of the Group purpose.
        #[arg(long)]
        topic: Option<String>,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Detach a project scope from the Group.
    DetachScope {
        /// Attached scope key to detach.
        scope_key: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Delete a Group and its retained local data.
    Delete {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Exact Group ID confirming the destructive operation.
        #[arg(long)]
        confirm: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Replace a Group with a new ID and retained configuration, dropping history and deleting the old Group.
    Reset {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Exact Group ID confirming the destructive operation.
        #[arg(long)]
        confirm: String,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Make an attached directory the Group's active scope.
    Use {
        /// Target Group ID.
        group_id: String,
        /// Attached project directory; defaults to the current directory.
        #[arg(default_value = ".")]
        path: String,
    },
    /// Start the enabled Actors and resume Group delivery.
    Start {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Stop managed Actors and delivery to native members.
    Stop {
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
    /// Change delivery state; stopped is an alias for group stop.
    SetState {
        /// Lifecycle state: active, idle, paused or stopped (stopped calls group stop).
        state: String,
        /// Target Group ID; omit to use the current Group.
        #[arg(long = "group")]
        group_id: Option<String>,
        /// Author identity: user or an Actor ID.
        #[arg(long, default_value = "user")]
        by: String,
    },
}
