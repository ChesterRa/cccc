mod actor;
mod group;
mod integrations;
mod messaging;
mod sessions;
pub use sessions::{SessionsAction, SessionsArgs};

pub use actor::{ActorAction, ActorArgs, ActorTarget};
pub use group::{GroupAction, GroupArgs};
pub use integrations::{
    ImAction, ImArgs, ImSetArgs, PromptArgs, SpaceAction, SpaceArgs, SpaceAuthAction,
    SpaceCredentialAction, SpaceJobsAction,
};
pub use messaging::{
    CancelReplyArgs, ConnectArgs, DeliverArgs, InboxArgs, LedgerAction, LedgerArgs, ReplyArgs,
    SendArgs, TailArgs, TrackedSendArgs,
};

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum WebModeArg {
    Normal,
    Exhibit,
}

#[derive(Debug, Args)]
pub struct WebArgs {
    /// Web access mode: normal or exhibit.
    #[arg(long, value_enum)]
    pub mode: Option<WebModeArg>,
    /// Serve a read-only Web UI.
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
    /// Runtime to set up; omit to set up the supported runtimes.
    #[arg(long)]
    pub runtime: Option<String>,
    /// Project directory whose MCP configuration to prepare.
    #[arg(long, default_value = ".")]
    pub path: String,
    /// Runtime adapter mode: default or acp.
    #[arg(long,default_value="default",value_parser=["default","acp"])]
    pub runtime_mode: String,
    /// Complete the separate official Antigravity ACP login in this terminal.
    #[arg(long)]
    pub login: bool,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Release channel to check or install.
    #[arg(long, value_enum)]
    pub channel: Option<ReleaseChannelArg>,
    /// Check the latest channel release and installation without changing files.
    #[arg(long)]
    pub check: bool,
    /// Show local installation details without requesting release metadata.
    #[arg(long, requires = "check")]
    pub offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReleaseChannelArg {
    Stable,
    Rc,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Also list the custom runtime among the detected runtimes.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Parser)]
#[command(
    name = "cccc",
    version = crate::PRODUCT_VERSION,
    about = "Collaborative Code Coordination Center",
    long_about = "Collaborative Code Coordination Center\n\nIn an interactive terminal, cccc starts the daemon and Web UI and opens a terminal\nstatus panel. Quitting it (/quit or Ctrl+C) stops the services it started.\n\ncccc --headless (or cccc web) starts the same services without the panel, for\nservers, containers, service managers and scripts. Without a terminal, cccc\nruns headless automatically."
)]
pub struct Cli {
    /// Web UI host to bind (default: saved Web Access setting, CCCC_WEB_HOST, 127.0.0.1).
    #[arg(long, alias = "web-host", global = true)]
    pub host: Option<String>,
    /// Web UI port (default: saved Web Access setting, CCCC_WEB_PORT, 8848).
    #[arg(long, alias = "web-port", global = true)]
    pub port: Option<u16>,
    /// Start the daemon and Web UI without the terminal panel (same as cccc web).
    #[arg(long)]
    pub headless: bool,
    #[command(subcommand)]
    pub command: Option<CommandKind>,
}

#[derive(Debug, Subcommand)]
pub enum CommandKind {
    /// Create a Group for a project directory, or attach it to a selected Group.
    Attach {
        /// Project directory.
        #[arg(default_value = ".")]
        path: String,
        /// Existing Group to attach to; omit to create a new Group.
        #[arg(long = "group")]
        group_id: Option<String>,
    },
    /// Create, show, start, stop and manage a Group.
    Group(GroupArgs),
    /// List Groups.
    Groups,
    /// Make a Group the active one for later commands.
    Use {
        /// Group ID to make active for later commands.
        group_id: String,
    },
    /// Show the active Group.
    Active,
    /// Add, start, stop and manage Actors in a Group.
    Actor(ActorArgs),
    /// Work with user-owned native Codex, Claude, Pi and OpenCode sessions.
    Sessions(SessionsArgs),
    /// Show the session prompt CCCC gives an Actor.
    Prompt(PromptArgs),
    /// Manage a Group's IM bridge.
    Im(ImArgs),
    /// Manage Group Space shared memory.
    Space(SpaceArgs),
    /// Read and consume the next unread Mail batch for an actor.
    Inbox(InboxArgs),
    /// Read cached same-account and Group connection directories. Does not connect or start Actors.
    Connect(ConnectArgs),
    /// Send a message to a Group.
    Send(SendArgs),
    /// Create a task and send one linked delegation message.
    TrackedSend(TrackedSendArgs),
    /// Reply to a message.
    Reply(ReplyArgs),
    /// Promote an existing Mail to Send, or retry a blocked or failed delivery.
    Deliver(DeliverArgs),
    /// Cancel the open reply obligations of a Send + Reply message.
    CancelReply(CancelReplyArgs),
    /// Show recent ledger events of a Group.
    Tail(TailArgs),
    /// Snapshot or compact a Group's ledger.
    Ledger(LedgerArgs),
    /// Start, stop or check the CCCC daemon.
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// List agent runtimes and prepare optional ones.
    Runtime {
        #[command(subcommand)]
        action: RuntimeAction,
    },
    /// Bind this machine to a CCCC membership account.
    Login,
    /// Remove local membership identity. The next login is a new hostname.
    Logout,
    /// Publish this machine's Web UI through membership Reach (Linux/macOS preview).
    Reach {
        #[command(subcommand)]
        action: ReachAction,
    },
    /// Manage standalone Group connections without an account or exposed Web UI.
    Direct {
        #[command(subcommand)]
        action: DirectAction,
    },
    /// Show the version, daemon state, active Group and available runtimes.
    Status,
    /// Check the environment and diagnose problems.
    Doctor(DoctorArgs),
    /// Prepare or report the MCP integration for an agent runtime.
    Setup(SetupArgs),
    /// Update CCCC through the installer that owns this executable.
    Update(UpdateArgs),
    /// Print the CCCC version.
    Version,
    /// Print the CCCC_HOME directory in use.
    Home,
    /// Run the MCP server over stdio, for agent integration.
    Mcp {
        /// Bind this stdio port to an opted-in user-owned native session.
        #[arg(long, value_parser = ["codex", "claude", "pi", "opencode"])]
        local_session_runtime: Option<String>,
        #[arg(long, hide = true, requires = "local_session_runtime")]
        local_session_host_pid: Option<u32>,
    },
    /// Start the daemon and Web UI without the terminal status panel (headless).
    Web(WebArgs),
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum ReachAction {
    /// Enable membership Reach for this machine.
    On,
    /// Disable membership Reach.
    Off,
    /// Show membership Reach status.
    Status,
    /// Install the optional Reach transport.
    Install,
}

#[derive(Debug, Subcommand)]
pub enum DirectAction {
    /// Show standalone connections for a Group.
    Status {
        /// Target Group ID.
        group_id: String,
    },
    /// Enable an encrypted collaboration listener (does not change Web Access).
    Listen {
        /// Local listener bind address.
        #[arg(long)]
        bind: String,
        /// Address to advertise to the other machine.
        #[arg(long)]
        address: String,
        /// Display name for this listener.
        #[arg(long, default_value = "")]
        name: String,
    },
    /// Stop the standalone collaboration listener.
    Stop,
    /// Create a standalone Group connection invitation.
    Invite {
        /// Target Group ID.
        group_id: String,
    },
    /// Read the invitation from standard input, keeping it out of shell history.
    Join {
        /// Target Group ID.
        group_id: String,
    },
    /// Approve a pending standalone connection.
    Approve {
        /// Target Group ID.
        group_id: String,
        /// Connection or approval ID.
        id: String,
    },
    /// Revoke a standalone invitation or approval.
    Revoke {
        /// Target Group ID.
        group_id: String,
        /// Connection or approval ID.
        id: String,
    },
    /// Remove a standalone Group connection.
    Remove {
        /// Target Group ID.
        group_id: String,
        /// Connection or approval ID.
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum DaemonAction {
    /// Start the daemon in the background.
    Start,
    /// Stop the daemon.
    Stop,
    /// Show whether the daemon is running.
    Status,
    /// Run only the daemon in the foreground (no Web server or terminal panel).
    Run,
}

#[derive(Debug, Subcommand)]
pub enum RuntimeAction {
    /// List supported agent runtimes and their availability.
    List {
        /// Also include the custom runtime.
        #[arg(long)]
        all: bool,
    },
    /// Inspect and prepare the optional Hermes adapter.
    Hermes {
        #[command(subcommand)]
        action: HermesAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum HermesAction {
    /// Show the local Hermes installation and adapter state.
    Status,
    /// Configure the cccc MCP server in the Hermes profile.
    Prepare {
        /// Project directory used for runtime preparation.
        #[arg(long = "path", default_value = ".")]
        cwd: String,
        /// Confirm Hermes MCP setup and enable the discovered CCCC tools (required).
        #[arg(long)]
        yes: bool,
        /// Configure again even when the cccc MCP server is already ready.
        #[arg(long)]
        force: bool,
    },
    /// Run Hermes' MCP test against CCCC with a probe Actor.
    #[command(name = "mcp-test")]
    McpTest {
        /// Project directory used for runtime preparation.
        #[arg(long = "path", default_value = ".")]
        cwd: String,
        /// Target Group ID.
        #[arg(long, default_value = "g_probe")]
        group_id: String,
        /// Actor ID within the Group.
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
                offline: false,
            }))
        ));
    }

    #[test]
    fn offline_update_requires_an_explicit_read_only_check() {
        assert!(Cli::try_parse_from(["cccc", "update", "--offline"]).is_err());
        let cli =
            Cli::try_parse_from(["cccc", "update", "--check", "--offline"]).expect("offline check");
        assert!(matches!(
            cli.command,
            Some(CommandKind::Update(UpdateArgs {
                check: true,
                offline: true,
                ..
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
