use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ActorArgs {
    #[command(subcommand)]
    pub action: ActorAction,
}

#[derive(Debug, Subcommand)]
pub enum ActorAction {
    List {
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Add {
        actor_id: String,
        #[arg(long, default_value = "")]
        title: String,
        #[arg(long, default_value = "codex")]
        runtime: String,
        #[arg(long, default_value = "pty")]
        runner: String,
        #[arg(long, default_value = "")]
        command: String,
        #[arg(long = "env")]
        env: Vec<String>,
        #[arg(long, default_value = "")]
        scope: String,
        #[arg(long, default_value = "enter")]
        submit: String,
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long, default_value = "user")]
        by: String,
    },
    Remove(ActorTarget),
    Start(ActorTarget),
    Stop(ActorTarget),
    Restart(ActorTarget),
    Update {
        actor_id: String,
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        runtime: Option<String>,
        #[arg(long)]
        runner: Option<String>,
        #[arg(long)]
        scope: Option<String>,
        #[arg(long)]
        command: Option<String>,
        #[arg(long = "env")]
        env: Vec<String>,
        #[arg(long)]
        submit: Option<String>,
        #[arg(long)]
        enabled: Option<bool>,
        #[arg(long)]
        runtime_state_source: Option<String>,
        #[arg(long, default_value = "user")]
        by: String,
    },
    Secrets {
        actor_id: String,
        #[arg(long = "group")]
        group_id: Option<String>,
        #[arg(long = "set")]
        set: Vec<String>,
        #[arg(long = "unset")]
        unset: Vec<String>,
        #[arg(long)]
        clear: bool,
        #[arg(long)]
        keys: bool,
        #[arg(long)]
        restart: bool,
        #[arg(long, default_value = "user")]
        by: String,
    },
}

#[derive(Debug, Args)]
pub struct ActorTarget {
    pub actor_id: String,
    #[arg(long = "group")]
    pub group_id: Option<String>,
    #[arg(long, default_value = "user")]
    pub by: String,
}
