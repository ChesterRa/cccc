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
    let checker = HostChecker::new(home.clone());
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
        agents: "not_observed".into(),
        finished_at: "2026-11-17T07:00:00Z".into(),
    };
    std::fs::write(&path, serde_json::to_vec(&recorded).expect("json")).expect("record");
    let last = checker.status().last.expect("recorded result");
    assert_eq!(last.result, "installed");
    assert_eq!(last.to_version, "0.4.59");
    assert_eq!(last.from_version.as_deref(), Some("0.4.58"));

    std::fs::write(&path, b"{not json").expect("broken record");
    assert!(checker.status().last.is_none());
    assert_eq!(checker.status().current, crate::PRODUCT_VERSION);
}
