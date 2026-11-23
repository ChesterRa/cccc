use cccc_contracts::session_node::DEVICE_SESSION_RUNTIMES;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum SessionNodeAction {
    /// Open the daemon's encrypted device-session gateway and print one pairing ticket.
    Host {
        /// UDP port. Omit to let the OS choose an available port.
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    /// Connect this computer's existing Codex/Claude sessions to a hub.
    #[command(
        after_help = "To pair Codex, run cccc sessions node connect --runtime codex --ticket-stdin. For Claude Code, use --runtime claude. To reconnect, run cccc sessions node connect."
    )]
    Connect {
        /// Read one pairing ticket from stdin; omit to resume the saved pairing.
        #[arg(long)]
        ticket_stdin: bool,
        /// Enable this native integration before pairing; does not filter exported sessions.
        #[arg(
            long,
            value_parser = clap::builder::PossibleValuesParser::new(
                DEVICE_SESSION_RUNTIMES.map(|runtime| runtime.as_str()),
            ),
            requires = "ticket_stdin"
        )]
        runtime: Option<String>,
        /// Name shown on the hub (defaults to this computer's hostname).
        #[arg(long)]
        name: Option<String>,
    },
    /// Inspect paired devices without exposing keys or pairing secrets.
    Status,
    /// Stop the device-session gateway until the next pairing ticket. Paired devices stay enrolled.
    Stop,
    /// Revoke a device. Existing sessions keep running but lose hub access.
    Revoke {
        #[arg(long)]
        node_id: String,
    },
}
