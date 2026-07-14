use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct GroupArgs {
    #[command(subcommand)]
    pub action: GroupAction,
}

#[derive(Debug, Subcommand)]
pub enum GroupAction {
    Create {
        #[arg(long, default_value = "working-group")]
        title: String,
        #[arg(long, default_value = "")]
        topic: String,
    },
    Show {
        group_id: String,
    },
    Update {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        topic: Option<String>,
        #[arg(long, default_value = "user")]
        by: String,
    },
    DetachScope {
        scope_key: String,
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long, default_value = "user")]
        by: String,
    },
    Delete {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        confirm: String,
        #[arg(long, default_value = "user")]
        by: String,
    },
    Reset {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        confirm: String,
        #[arg(long, default_value = "user")]
        by: String,
    },
    Use {
        group_id: String,
        #[arg(default_value = ".")]
        path: String,
    },
    Start {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long, default_value = "user")]
        by: String,
    },
    Stop {
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long, default_value = "user")]
        by: String,
    },
    SetState {
        state: String,
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long, default_value = "user")]
        by: String,
    },
}
