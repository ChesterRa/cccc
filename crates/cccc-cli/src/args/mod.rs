mod actor;
mod group;
mod integrations;
mod messaging;

pub use actor::{ActorAction, ActorArgs, ActorTarget};
pub use group::{GroupAction, GroupArgs};
pub use integrations::{
    ImAction, ImArgs, ImSetArgs, PromptArgs, SpaceAction, SpaceArgs, SpaceAuthAction,
    SpaceCredentialAction, SpaceJobsAction,
};
pub use messaging::{
    CancelReplyArgs, DeliverArgs, InboxArgs, LedgerAction, LedgerArgs, ReplyArgs, SendArgs,
    TailArgs, TrackedSendArgs,
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
    /// Accepted for legacy CLI compatibility; the native server does not use a source reloader.
    #[arg(long)]
    pub reload: bool,
    /// Accepted for legacy CLI compatibility.
    #[arg(long, default_value = "info")]
    pub log_level: String,
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
    #[arg(long, value_enum)]
    pub channel: Option<ReleaseChannelArg>,
    /// Show the standalone installation and release channel without changing files.
    #[arg(long)]
    pub check: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReleaseChannelArg {
    Stable,
    Rc,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(long)]
    pub all: bool,
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
    /// Read and consume the next unread Mail batch for an actor.
    Inbox(InboxArgs),
    Send(SendArgs),
    TrackedSend(TrackedSendArgs),
    Reply(ReplyArgs),
    Deliver(DeliverArgs),
    CancelReply(CancelReplyArgs),
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
    /// Bind this machine to a CCCC membership account.
    Login,
    /// Remove local membership identity. The next login is a new hostname.
    Logout,
    Reach {
        #[command(subcommand)]
        action: ReachAction,
    },
    Status,
    Doctor(DoctorArgs),
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

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum ReachAction {
    On,
    Off,
    Status,
    Install,
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
    List {
        #[arg(long)]
        all: bool,
    },
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
    fn parses_membership_verbs() {
        assert!(matches!(
            Cli::try_parse_from(["cccc", "login"])
                .expect("login")
                .command,
            Some(CommandKind::Login)
        ));
        assert!(matches!(
            Cli::try_parse_from(["cccc", "logout"])
                .expect("logout")
                .command,
            Some(CommandKind::Logout)
        ));
        assert!(matches!(
            Cli::try_parse_from(["cccc", "reach", "on"])
                .expect("reach on")
                .command,
            Some(CommandKind::Reach {
                action: ReachAction::On
            })
        ));
        assert!(matches!(
            Cli::try_parse_from(["cccc", "reach", "status"])
                .expect("reach status")
                .command,
            Some(CommandKind::Reach {
                action: ReachAction::Status
            })
        ));
        assert!(matches!(
            Cli::try_parse_from(["cccc", "reach", "install"])
                .expect("reach install")
                .command,
            Some(CommandKind::Reach {
                action: ReachAction::Install
            })
        ));
    }

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
            Some(CommandKind::Update(UpdateArgs {
                check: true,
                channel: None,
            }))
        ));
    }

    #[test]
    fn parses_legacy_prompt_tail_and_runtime_flags() {
        let cli = Cli::try_parse_from(["cccc", "prompt", "--actor-id", "peer"]).expect("prompt");
        assert!(matches!(
            cli.command,
            Some(CommandKind::Prompt(PromptArgs {
                actor_id: Some(ref actor_id),
                ..
            })) if actor_id == "peer"
        ));

        let cli = Cli::try_parse_from(["cccc", "tail", "--lines", "12"]).expect("tail");
        assert!(matches!(
            cli.command,
            Some(CommandKind::Tail(TailArgs { limit: 12, .. }))
        ));

        let cli = Cli::try_parse_from(["cccc", "runtime", "list", "--all"]).expect("runtime list");
        assert!(matches!(
            cli.command,
            Some(CommandKind::Runtime {
                action: RuntimeAction::List { all: true }
            })
        ));
    }

    #[test]
    fn parses_legacy_space_subcommands() {
        let cli = Cli::try_parse_from([
            "cccc", "space", "jobs", "list", "--lane", "work", "--state", "failed", "--limit", "25",
        ])
        .expect("space jobs list");
        assert!(matches!(
            cli.command,
            Some(CommandKind::Space(SpaceArgs {
                action: SpaceAction::Jobs {
                    action: SpaceJobsAction::List { limit: 25, .. }
                }
            }))
        ));

        let cli = Cli::try_parse_from([
            "cccc",
            "space",
            "auth",
            "start",
            "--timeout-seconds",
            "120",
            "--force-reauth",
            "--by",
            "operator",
        ])
        .expect("space auth start");
        assert!(matches!(
            cli.command,
            Some(CommandKind::Space(SpaceArgs {
                action: SpaceAction::Auth {
                    action: SpaceAuthAction::Start {
                        timeout_seconds: 120,
                        force_reauth: true,
                        ..
                    }
                }
            }))
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
            "--task-priority",
            "high",
            "--idempotency-key",
            "retry-1",
        ])
        .expect("tracked send");
        let Some(CommandKind::TrackedSend(args)) = cli.command else {
            panic!("wrong command");
        };
        assert_eq!(args.title, "Task");
        assert_eq!(args.task_priority, "high");
        assert_eq!(args.idempotency_key, "retry-1");
    }

    #[test]
    fn parses_message_delivery_control_commands() {
        let cli = Cli::try_parse_from([
            "cccc",
            "deliver",
            "event-1",
            "--to",
            "peer-1,peer-2",
            "--force-ambiguous",
        ])
        .expect("deliver");
        let Some(CommandKind::Deliver(args)) = cli.command else {
            panic!("wrong command");
        };
        assert_eq!(args.event_id, "event-1");
        assert_eq!(args.actor_ids, ["peer-1,peer-2"]);
        assert!(args.force_ambiguous);

        let cli = Cli::try_parse_from(["cccc", "cancel-reply", "event-2"]).expect("cancel reply");
        let Some(CommandKind::CancelReply(args)) = cli.command else {
            panic!("wrong command");
        };
        assert_eq!(args.event_id, "event-2");
    }
}
