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
        #[arg(long,value_parser=["codex","claude"])]
        runtime: String,
    },
    Disable {
        #[arg(long,value_parser=["codex","claude"])]
        runtime: String,
    },
    /// List native sources that can currently receive collaboration messages.
    List {
        #[arg(long)]
        include_offline: bool,
    },
    /// Create an ordinary Group from selected native sources.
    Create {
        #[arg(long,num_args=1..)]
        sources: Vec<String>,
        #[arg(long)]
        title: Option<String>,
    },
    /// Add selected native sources without replacing existing Group members.
    Join {
        #[arg(long)]
        group: String,
        #[arg(long,num_args=1..)]
        sources: Vec<String>,
    },
    /// Pause one source's Group access without stopping its native process.
    Leave {
        #[arg(long)]
        group: String,
        #[arg(long = "from")]
        source_id: String,
    },
    /// Show local recovery instructions; never starts or resumes the runtime.
    Recovery {
        #[arg(long)]
        group: String,
        #[arg(long)]
        actor: String,
        /// Explicitly recheck Codex's existing receive channel before reading.
        #[arg(long)]
        refresh: bool,
    },
    /// Send directly; the first message creates or reuses an ordinary Group.
    Send {
        #[arg(long,num_args=1..,conflicts_with="group")]
        to: Vec<String>,
        #[arg(long)]
        group: Option<String>,
        /// Omit for a human message; local callers may explicitly select a source.
        #[arg(long = "from")]
        source_id: Option<String>,
        #[arg(long)]
        text: String,
    },
    #[command(hide = true)]
    Hook {
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
}
