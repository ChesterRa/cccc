use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct SessionsArgs {
    #[command(subcommand)]
    pub action: SessionsAction,
}

#[derive(Debug, Subcommand)]
pub enum SessionsAction {
    /// Enable same-user native session collaboration once for this runtime.
    Enable {
        /// Native app: codex, claude, pi or opencode.
        #[arg(long,value_parser=["codex","claude","pi","opencode"])]
        runtime: String,
    },
    /// Disable this CCCC integration without closing the native apps.
    Disable {
        /// Native app: codex, claude, pi or opencode.
        #[arg(long,value_parser=["codex","claude","pi","opencode"])]
        runtime: String,
    },
    /// List native sources that can currently receive collaboration messages.
    List {
        /// Also include registered sources that cannot currently receive.
        #[arg(long)]
        include_offline: bool,
    },
    /// Create an ordinary Group from selected native sources.
    Create {
        /// Native source IDs from sessions list.
        #[arg(long,num_args=1..)]
        sources: Vec<String>,
        /// Display title.
        #[arg(long)]
        title: Option<String>,
    },
    /// Add selected native sources without replacing existing Group members.
    Join {
        /// Target Group ID.
        #[arg(long)]
        group: String,
        /// Native source IDs from sessions list.
        #[arg(long,num_args=1..)]
        sources: Vec<String>,
    },
    /// Pause one source's Group access without stopping its native process.
    Leave {
        /// Target Group ID.
        #[arg(long)]
        group: String,
        /// Native source ID from sessions list.
        #[arg(long = "from")]
        source_id: String,
    },
    /// Show local recovery instructions; never starts or resumes the runtime.
    Recovery {
        /// Target Group ID.
        #[arg(long)]
        group: String,
        /// Native Actor ID to inspect.
        #[arg(long)]
        actor: String,
        /// Explicitly recheck Codex's existing receive channel before reading.
        #[arg(long)]
        refresh: bool,
    },
    /// Send directly; the first message creates or reuses an ordinary Group.
    Send {
        /// Recipient native source IDs from sessions list.
        #[arg(long,num_args=1..,conflicts_with="group")]
        to: Vec<String>,
        /// Target Group ID.
        #[arg(long)]
        group: Option<String>,
        /// Omit for a human message; local callers may explicitly select a source.
        #[arg(long = "from")]
        source_id: Option<String>,
        /// Message text.
        #[arg(long)]
        text: String,
    },
    #[command(hide = true)]
    Hook {
        /// Native app: codex, claude, pi or opencode.
        #[arg(long,value_parser=["codex","claude"])]
        runtime: String,
        #[arg(long, conflicts_with = "host_shell_pid")]
        host_pid: Option<u32>,
        #[arg(long)]
        host_shell_pid: Option<u32>,
        #[arg(long)]
        cccc_home: Option<PathBuf>,
        #[arg(long)]
        provider_home: Option<PathBuf>,
    },
    #[command(hide = true)]
    Receive {
        #[arg(long)]
        host_pid: Option<u32>,
    },
    /// Extension-owned receive lease for one native context.
    #[command(hide = true)]
    ExtensionBridge {
        /// Native app: codex, claude, pi or opencode.
        #[arg(long,value_parser=["pi","opencode"])]
        runtime: String,
        #[arg(long)]
        host_pid: u32,
        #[arg(long)]
        session_id: String,
        /// Native application working directory.
        #[arg(long)]
        cwd: PathBuf,
        #[arg(long)]
        provider_home: PathBuf,
        #[arg(long)]
        backend_pid: Option<u32>,
    },
}
