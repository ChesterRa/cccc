//! Claude Code refuses background sessions in a workspace whose trust prompt was never
//! accepted, and a detached `--bg` launch has no terminal to accept it in.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Prefix of Claude Code's refusal, printed before any session exists.
const REFUSAL: &str = "Workspace not trusted";

#[derive(Debug)]
pub(crate) struct WorkspaceUntrusted {
    pub(crate) workspace: PathBuf,
    executable: String,
    config_dir: PathBuf,
}

impl WorkspaceUntrusted {
    pub(super) fn from_refusal(
        detail: &str,
        workspace: &Path,
        executable: &str,
        config_dir: &Path,
    ) -> Option<io::Error> {
        detail.contains(REFUSAL).then(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                Self {
                    workspace: workspace.to_path_buf(),
                    executable: executable.to_owned(),
                    config_dir: config_dir.to_path_buf(),
                },
            )
        })
    }
}

impl fmt::Display for WorkspaceUntrusted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Claude Code has not trusted the workspace '{}'. Run '{}' once in that directory \
             with CLAUDE_CONFIG_DIR set to '{}', accept the trust prompt and exit, then start \
             the Actor again. Trusting a parent folder does not cover a separate Git repository.",
            self.workspace.display(),
            self.executable,
            self.config_dir.display(),
        )
    }
}

impl std::error::Error for WorkspaceUntrusted {}

pub(crate) fn untrusted_workspace(error: &io::Error) -> Option<&Path> {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<WorkspaceUntrusted>())
        .map(|untrusted| untrusted.workspace.as_path())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::ops::codex_voice_analyst::SessionPurpose;
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn background_launch_in_an_untrusted_workspace_reports_what_to_trust() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let config_dir = temp.path().join("config");
        let executable = temp.path().join("claude");
        std::fs::create_dir(&workspace).expect("workspace");
        std::fs::create_dir(&config_dir).expect("config");
        // Claude Code 2.1.281's actual `--bg` refusal, on stderr with exit status 1.
        std::fs::write(
            &executable,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '2.1.281 (Claude Code)\\n'\n  exit 0\nfi\n\
             printf 'Workspace not trusted. Run `claude` in %s once and accept the trust prompt, then retry.\\n' \"$PWD\" >&2\n\
             exit 1\n",
        )
        .expect("fake Claude");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("executable permissions");

        let result = super::super::launch(
            super::super::command::PreparedClaude {
                executable: executable.to_string_lossy().into_owned(),
                arguments: Vec::new(),
                launch_environment: BTreeMap::new(),
                config_dir: config_dir.clone(),
                settings_path: temp.path().join("managed-settings.json"),
            },
            &workspace,
            "generation-12345678",
            SessionPurpose::Actor,
            None,
        )
        .await;
        let Err(error) = result else {
            panic!("an untrusted workspace must not launch");
        };
        assert_eq!(untrusted_workspace(&error), Some(workspace.as_path()));
        let message = error.to_string();
        assert!(
            message.contains(&workspace.display().to_string()),
            "{message}"
        );
        assert!(
            message.contains(&config_dir.display().to_string()),
            "{message}"
        );
    }
}
