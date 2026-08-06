mod actor;
mod group;
mod integrations;
mod messaging;

pub use actor::{ActorAction, ActorArgs, ActorTarget};
pub use group::{GroupAction, GroupArgs};
pub use integrations::{
    ImAction, ImArgs, ImSetArgs, PromptArgs, SpaceAction, SpaceArgs, SpaceCredentialAction,
};
pub use messaging::{
    InboxArgs, LedgerAction, LedgerArgs, ReadArgs, ReplyArgs, SendArgs, TailArgs, TrackedSendArgs,
};

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum WebModeArg {
    Normal,
    Exhibit,
}

#[derive(Debug, Args)]
pub struct WebArgs {
    #[arg(long, value_enum)]
    pub mode: Option<WebModeArg>,
    #[arg(long, conflicts_with = "mode")]
    pub exhibit: bool,
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    #[arg(long)]
    pub runtime: Option<String>,
    #[arg(long, default_value = ".")]
    pub path: String,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Show the detected installation and update source without changing files.
    #[arg(long)]
    pub check: bool,
}

#[derive(Debug, Parser)]
#[command(
    name = "cccc",
    version = crate::PRODUCT_VERSION,
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
    /// Update CCCC through the installer that owns this executable.
    Update(UpdateArgs),
    Version,
    Home,
    Mcp,
    #[command(hide = true)]
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    Web(WebArgs),
}

#[derive(Debug, Subcommand)]
pub enum HookAction {
    CodexState,
    ClaudeState,
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
    Hermes {
        #[command(subcommand)]
        action: HermesAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum HermesAction {
    Status,
    Prepare {
        #[arg(long = "path", default_value = ".")]
        cwd: String,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        force: bool,
    },
    #[command(name = "mcp-test")]
    McpTest {
        #[arg(long = "path", default_value = ".")]
        cwd: String,
        #[arg(long, default_value = "g_probe")]
        group_id: String,
        #[arg(long, default_value = "hermes-probe")]
        actor_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exhibit_web_modes() {
        let cli = Cli::try_parse_from(["cccc", "web", "--exhibit"]).expect("exhibit");
        assert!(matches!(
            cli.command,
            Some(CommandKind::Web(WebArgs { exhibit: true, .. }))
        ));

        let cli = Cli::try_parse_from(["cccc", "web", "--mode", "exhibit"]).expect("mode");
        assert!(matches!(
            cli.command,
            Some(CommandKind::Web(WebArgs {
                mode: Some(WebModeArg::Exhibit),
                ..
            }))
        ));
    }

    #[test]
    fn parses_update_check() {
        let cli = Cli::try_parse_from(["cccc", "update", "--check"]).expect("update check");
        assert!(matches!(
            cli.command,
            Some(CommandKind::Update(UpdateArgs { check: true }))
        ));
    }

    #[test]
    fn parses_complete_tracked_send_options() {
        let cli = Cli::try_parse_from([
            "cccc",
            "tracked-send",
            "implement",
            "--title",
            "Task",
            "--outcome",
            "done",
            "--checklist",
            "code\ntests",
            "--assignee",
            "peer",
            "--waiting-on",
            "actor",
            "--handoff-to",
            "lead",
            "--notes",
            "note",
            "--no-reply-required",
            "--idempotency-key",
            "retry-1",
        ])
        .expect("tracked send");
        let Some(CommandKind::TrackedSend(args)) = cli.command else {
            panic!("wrong command");
        };
        assert_eq!(args.title, "Task");
        assert!(args.no_reply_required);
        assert_eq!(args.idempotency_key, "retry-1");
    }
}
