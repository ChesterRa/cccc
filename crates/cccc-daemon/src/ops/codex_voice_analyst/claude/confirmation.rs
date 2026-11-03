//! Provider refusals that require a human in a foreground terminal, not permission inference.
use super::command::PreparedClaude;
use std::{
    collections::BTreeMap,
    fmt, io,
    path::{Path, PathBuf},
};

const DISCLAIMER: &str = "--bg with bypassPermissions requires accepting the disclaimer first";

#[derive(Clone)]
pub(crate) struct ConfirmationLaunch {
    pub(crate) executable: String,
    pub(crate) environment: BTreeMap<String, String>,
    pub(crate) config_dir: PathBuf,
    pub(crate) arguments: Vec<String>,
}

impl ConfirmationLaunch {
    pub(super) fn from_prepared(prepared: &PreparedClaude) -> Self {
        let mut arguments = vec![
            "--settings".into(),
            prepared.settings_path.to_string_lossy().into_owned(),
        ];
        // Preserve validated configuration boundaries: the foreground confirmation
        // must not load settings, hooks or MCP sources excluded by the managed launch.
        for (index, argument) in prepared.arguments.iter().enumerate() {
            if matches!(argument.as_str(), "--bare" | "--strict-mcp-config") {
                arguments.push(argument.clone());
            } else if argument == "--setting-sources" {
                arguments.push(argument.clone());
                if let Some(value) = prepared.arguments.get(index + 1) {
                    arguments.push(value.clone());
                }
            } else if argument.starts_with("--setting-sources=") {
                arguments.push(argument.clone());
            }
        }
        Self {
            executable: prepared.executable.clone(),
            environment: prepared.launch_environment.clone(),
            config_dir: prepared.config_dir.clone(),
            arguments,
        }
    }
}

impl fmt::Debug for ConfirmationLaunch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfirmationLaunch")
            .field("executable", &self.executable)
            .field("config_dir", &self.config_dir)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub(super) struct DisclaimerRequired {
    pub(super) launch: ConfirmationLaunch,
    workspace: PathBuf,
}

impl fmt::Display for DisclaimerRequired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Claude Code requires your bypassPermissions confirmation. Open this Actor's terminal in '{}', review Claude's own disclaimer, and keep the terminal open until CCCC switches to the managed session. You may decline or stop the Actor; retry with Start when ready. Configured executable: '{}'; Resolved settings directory: '{}'.",
            self.workspace.display(),
            self.launch.executable,
            self.launch.config_dir.display()
        )
    }
}
impl std::error::Error for DisclaimerRequired {}

pub(super) fn refusal(detail: &str, cwd: &Path, prepared: &PreparedClaude) -> Option<io::Error> {
    detail.contains(DISCLAIMER).then(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            DisclaimerRequired {
                workspace: cwd.to_owned(),
                launch: ConfirmationLaunch::from_prepared(prepared),
            },
        )
    })
}

pub(crate) fn launch(error: &io::Error) -> Option<&ConfirmationLaunch> {
    error
        .get_ref()?
        .downcast_ref::<DisclaimerRequired>()
        .map(|refusal| &refusal.launch)
        .or_else(|| super::workspace_trust::confirmation_launch(error))
}
