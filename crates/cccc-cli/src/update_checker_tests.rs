use super::*;
use crate::args::ReleaseChannelArg;
use crate::commands::update::Installation;
use cccc_web::UpdateChecker;

fn outcome(owner: InstallationOwner, latest: Latest) -> CheckOutcome {
    CheckOutcome {
        current: "1.2.3".into(),
        installation: Installation {
            owner,
            channel: ReleaseChannelArg::Stable,
            platform_supported: true,
            platform_requirement: "fixture platform",
        },
        latest,
    }
}

fn newer() -> Latest {
    Latest::Found {
        version: "1.2.4".into(),
        status: Status::Newer,
    }
}

#[test]
fn a_newer_release_names_the_command_for_each_installation() {
    let standalone = describe(outcome(InstallationOwner::Standalone, newer()));
    assert_eq!(standalone.latest.as_deref(), Some("1.2.4"));
    assert_eq!(standalone.status.as_deref(), Some("newer"));
    assert_eq!(standalone.failure, None);
    assert_eq!(
        standalone.command.as_deref(),
        Some("cccc update --channel stable")
    );
    assert!(
        standalone
            .release_notes_url
            .as_deref()
            .is_some_and(|url| url.starts_with("https://github.com/") && url.ends_with("1.2.4"))
    );

    let pip = describe(outcome(InstallationOwner::Pip, newer()));
    assert_eq!(
        pip.command.as_deref(),
        Some("python -m pip install --upgrade \"cccc-pair==1.2.4\"")
    );
    assert_eq!(pip.installation, "pip");

    // A source build or another package manager updates itself its own way.
    let unmanaged = describe(outcome(InstallationOwner::Unmanaged, newer()));
    assert_eq!(unmanaged.command, None);
    assert_eq!(unmanaged.status.as_deref(), Some("newer"));
}

#[test]
fn a_failed_check_is_not_reported_as_current() {
    let failed = describe(outcome(
        InstallationOwner::Standalone,
        Latest::Unavailable("fixture timeout".into()),
    ));
    assert_eq!(failed.latest, None);
    assert_eq!(failed.status, None);
    assert_eq!(failed.failure.as_deref(), Some("fixture timeout"));
    assert_eq!(failed.command, None);
    assert_eq!(failed.release_notes_url, None);
}

#[test]
fn a_current_release_offers_nothing_to_run() {
    let current = describe(outcome(
        InstallationOwner::Standalone,
        Latest::Found {
            version: "1.2.3".into(),
            status: Status::Current,
        },
    ));
    assert_eq!(current.status.as_deref(), Some("current"));
    assert_eq!(current.command, None);
}

#[test]
fn the_last_update_is_read_from_its_record_and_a_broken_record_is_left_out() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let checker = HostChecker::new(home.clone(), None);
    assert!(checker.status().last.is_none());

    let path = home.root().join("update/last.json");
    std::fs::create_dir_all(path.parent().expect("update dir")).expect("update dir");
    let recorded = LastResult {
        transaction: "0123456789abcdef0123456789abcdef".into(),
        from_version: Some("0.4.58".into()),
        to_version: "0.4.59".into(),
        mode: Some("restart".into()),
        result: "installed".into(),
        stage: None,
        reason: None,
        agents: cccc_web::UpdateAgents {
            state: "not_observed".into(),
            ..Default::default()
        },
        finished_at: "2026-11-17T07:00:00Z".into(),
        source: Some("release".into()),
        token: None,
        source_id: Some("fixture-release".into()),
    };
    std::fs::write(&path, serde_json::to_vec(&recorded).expect("json")).expect("record");
    let last = checker.status().last.expect("recorded result");
    assert_eq!(last.result, "installed");
    assert_eq!(last.to_version, "0.4.59");
    assert_eq!(last.from_version.as_deref(), Some("0.4.58"));
    assert_eq!(last.source.as_deref(), Some("release"));

    // A restart into a rebuilt program names the file it restarted into, and
    // the agents it found again.
    let agents = cccc_web::UpdateAgents {
        state: "observed".into(),
        reattached: vec![cccc_web::UpdateAgent {
            group_id: "g_fixture".into(),
            actor_id: "peer-1".into(),
            actor_created_at: "2026-11-17T00:00:00Z".into(),
            generation: Some("g1".into()),
            owner_id: Some("o1".into()),
            title: Some("Reviewer".into()),
        }],
        ..Default::default()
    };
    let restarted = LastResult {
        result: "restarted".into(),
        source: Some("local".into()),
        token: Some("fixture-token".into()),
        source_id: Some("fixture-local".into()),
        agents: agents.clone(),
        ..recorded
    };
    std::fs::write(&path, serde_json::to_vec(&restarted).expect("json")).expect("record");
    let last = checker.status().last.expect("recorded result");
    assert_eq!(last.result, "restarted");
    assert_eq!(last.source.as_deref(), Some("local"));
    assert_eq!(last.token.as_deref(), Some("fixture-token"));
    assert_eq!(last.source_id.as_deref(), Some("fixture-local"));
    assert_eq!(last.agents, agents);

    // Recorded while its host looked for the agents, read when nothing looks:
    // the look was cut short, so they stay unconfirmed.
    let cut_short = LastResult {
        agents: cccc_web::UpdateAgents {
            state: "observing".into(),
            unconfirmed: agents.reattached.clone(),
            ..Default::default()
        },
        ..restarted
    };
    std::fs::write(&path, serde_json::to_vec(&cut_short).expect("json")).expect("record");
    let last = checker.status().last.expect("recorded result");
    assert_eq!(last.agents.state, "unconfirmed");
    assert_eq!(last.agents.unconfirmed, agents.reattached);

    std::fs::write(&path, b"{not json").expect("broken record");
    assert!(checker.status().last.is_none());
    assert_eq!(checker.status().current, crate::PRODUCT_VERSION);
}

/// Writes a new file at `path`, as a build does, instead of rewriting the old one.
#[cfg(unix)]
fn program(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let written = path.with_extension("new");
    std::fs::write(&written, format!("#!/bin/sh\n{body}\n")).expect("program");
    std::fs::set_permissions(&written, std::fs::Permissions::from_mode(0o755)).expect("mode");
    std::fs::rename(&written, path).expect("replace");
}

#[cfg(unix)]
#[tokio::test]
async fn a_rebuilt_launch_program_is_read_and_an_unchanged_one_is_not_run() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let path = temp.path().join("cccc");
    let ran = temp.path().join("ran");
    // The program the host started from leaves a mark if it is run.
    program(&path, &format!("touch '{}'", ran.display()));
    let launch = LaunchProgram {
        identity: FileIdentity::read(&path).expect("identity"),
        path: path.clone(),
    };
    assert_eq!(local_build(home.clone(), Some(launch.clone())).await, None);
    assert!(!ran.exists(), "an unchanged program is not run");

    // A self-check can identify a build that cannot take over this restart.
    program(
        &path,
        r#"echo '{"schema":1,"version":"0.4.59","build":{"source_id":"fixture-source"},"daemon_compatibility":"fixture","relay_schemas":[1]}'"#,
    );
    assert_eq!(
        local_build(home.clone(), Some(launch.clone())).await,
        Some(LocalBuild::Incompatible {
            version: "0.4.59".into()
        })
    );
    // This replacement accepts the restart but cannot retain agents.
    let capabilities = cccc_core::update_capabilities::UpdateCapabilities {
        local_restart: true,
        intent_schemas: vec![cccc_core::attended_update::INTENT_SCHEMA],
        ..Default::default()
    };
    let fixture = serde_json::json!({
        "schema":1, "version":"0.4.59", "build":{"source_id":"fixture-source"},
        "daemon_compatibility":"fixture", "relay_schemas":[crate::update_host::RELAY_SCHEMA],
        "capabilities":capabilities
    });
    program(&path, &format!("echo '{fixture}'"));
    let rebuilt = FileIdentity::read(&path).expect("rebuilt identity");
    let token = rebuilt.token().expect("rebuilt token");
    assert_ne!(token, launch.identity.token().expect("launch token"));
    let keep = cccc_daemon::session_owner::preservation_check(&home, &capabilities).await;
    assert!(!keep.allowed);
    assert_eq!(
        local_build(home.clone(), Some(launch.clone())).await,
        Some(LocalBuild::rebuilt(
            "0.4.59".into(),
            "fixture-source".into(),
            rebuilt.written,
            token,
            keep,
        ))
    );

    // One that cannot check itself, as while a build still writes it, is
    // unreadable rather than nothing new.
    program(&path, "exit 3");
    assert!(matches!(
        local_build(home.clone(), Some(launch.clone())).await,
        Some(LocalBuild::Unreadable { .. })
    ));

    // So is a removed one, which names its path.
    std::fs::remove_file(&path).expect("remove");
    let Some(LocalBuild::Unreadable { reason }) = local_build(home.clone(), Some(launch)).await
    else {
        panic!("a removed program is unreadable");
    };
    assert!(reason.contains(&path.display().to_string()), "{reason}");

    // A host that cannot restart into a rebuilt program does not look.
    assert_eq!(local_build(home, None).await, None);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn the_running_program_is_its_own_launch_program_until_it_is_replaced() {
    // This test's own program, read from its image and from its path, is the
    // same file, so nothing is run and nothing is reported.
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let launch = LaunchProgram::current().expect("the running program");
    assert_eq!(
        launch.path,
        std::env::current_exe().expect("current executable")
    );
    assert_eq!(local_build(home, Some(launch)).await, None);
}
