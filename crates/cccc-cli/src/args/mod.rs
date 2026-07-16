mod actor;
mod group;
mod integrations;
mod messaging;

pub use actor::{ActorAction, ActorArgs, ActorTarget};
pub use group::{GroupAction, GroupArgs};
pub use integrations::{ImAction, ImArgs, ImSetArgs, PromptArgs, SpaceAction, SpaceArgs};
pub use messaging::{
    InboxArgs, LedgerAction, LedgerArgs, ReadArgs, ReplyArgs, SendArgs, TailArgs, TrackedSendArgs,
};

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Args)]
pub struct SetupArgs {
    #[arg(long)]
    pub runtime: Option<String>,
    #[arg(long, default_value = ".")]
    pub path: String,
}

#[derive(Debug, Parser)]
#[command(
    name = "cccc",
    version,
    about = "Collaborative Code Coordination Center"
)]
pub struct Cli {
    #[arg(long, alias = "web-host", global = true)]
    pub host: Option<String>,
    #[arg(long, alias = "web-port", global = true)]
    pub port: Option<u16>,
    #[command(subcommand)]
    pub command: Option<CommandKind>,
}

#[derive(Debug, Subcommand)]
pub enum CommandKind {
    Attach {
        #[arg(default_value = ".")]
        path: String,
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    Group(GroupArgs),
    Groups,
    Use {
        group_id: String,
    },
    Active,
    Actor(ActorArgs),
    Prompt(PromptArgs),
    Im(ImArgs),
    Space(SpaceArgs),
    Inbox(InboxArgs),
    Read(ReadArgs),
    Send(SendArgs),
    TrackedSend(TrackedSendArgs),
    Reply(ReplyArgs),
    Tail(TailArgs),
    Ledger(LedgerArgs),
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    Runtime {
        #[command(subcommand)]
        action: RuntimeAction,
    },
    Status,
    Doctor,
    Setup(SetupArgs),
    Version,
    Home,
    Mcp,
    Web,
}

#[derive(Debug, Subcommand)]
pub enum DaemonAction {
    Start,
    Stop,
    Status,
    Run,
}

#[derive(Debug, Subcommand)]
pub enum RuntimeAction {
    List,
}
