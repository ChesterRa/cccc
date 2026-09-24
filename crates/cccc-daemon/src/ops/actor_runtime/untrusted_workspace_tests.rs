use super::*;
use std::path::Path;

const REFUSAL: &str = "Workspace not trusted. Run `claude` in /work/app once and accept the trust \
                       prompt, then retry.";

fn refused(workspace: &str) -> OpError {
    launch_error(
        crate::ops::codex_voice_analyst::claude_workspace_refusal(REFUSAL, Path::new(workspace))
            .expect("refusal is recognized"),
    )
}

#[test]
fn untrusted_claude_workspace_gets_its_own_code_and_names_the_workspace() {
    let error = refused("/work/app");
    assert_eq!(error.code, CLAUDE_WORKSPACE_UNTRUSTED);
    assert_eq!(error.details["workspace"], "/work/app");
    assert!(error.message.contains("'/work/app'"), "{}", error.message);
    assert!(
        error.message.contains("accept the trust prompt"),
        "{}",
        error.message
    );
}

#[test]
fn other_launch_failures_stay_io_errors() {
    assert!(
        crate::ops::codex_voice_analyst::claude_workspace_refusal(
            "--bg with bypassPermissions requires accepting the disclaimer first",
            Path::new("/work/app"),
        )
        .is_none()
    );
    let error = launch_error(std::io::Error::other("Claude Agent View launch timed out"));
    assert_eq!(error.code, "io_error");
    assert!(!error.details.contains_key("workspace"));
}

#[test]
fn only_the_same_untrusted_workspace_absorbs_a_rollback_restart_failure() {
    let original = refused("/work/app");
    assert!(same_untrusted_workspace(&original, &refused("/work/app")));
    // A different workspace, or a different cause, is still a real rollback failure.
    assert!(!same_untrusted_workspace(
        &original,
        &refused("/work/other")
    ));
    assert!(!same_untrusted_workspace(
        &original,
        &OpError::new("io_error", "spawn failed")
    ));
    assert!(!same_untrusted_workspace(
        &OpError::new("io_error", "spawn failed"),
        &original
    ));
}
