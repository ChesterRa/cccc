#[cfg(unix)]
mod antigravity_session;
#[cfg(unix)]
mod grok_redirect;
#[cfg(unix)]
mod grok_session;
mod live_claude;
mod live_kilo;
#[cfg(unix)]
mod live_native_acp;
mod live_session;
mod live_support;
#[cfg(unix)]
mod native_acp_session;
mod session;
mod startup_readiness;
mod support;
mod unit;

pub(super) fn configure_isolated_claude_project(temp: &tempfile::TempDir) -> std::path::PathBuf {
    let config_dir = temp.path().join("claude");
    let project = temp
        .path()
        .join("project")
        .canonicalize()
        .expect("probe project");
    let project_key = cccc_runtime::compatible_path(&project).to_string_lossy();
    std::fs::create_dir(&config_dir).expect("isolated Claude config");
    cccc_core::fs::write_json(
        &config_dir.join(".claude.json"),
        &serde_json::json!({
            "bypassPermissionsModeAccepted": true,
            "hasCompletedOnboarding": true,
            "projects": { (project_key): { "hasTrustDialogAccepted": true } }
        }),
    )
    .expect("isolated CLI onboarding and project trust");
    cccc_core::fs::write_json(
        &config_dir.join("settings.json"),
        &serde_json::json!({"skipDangerousModePermissionPrompt": true}),
    )
    .expect("isolated CLI settings");
    config_dir
}
