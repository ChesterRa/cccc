use super::*;

fn args(offline: bool) -> UpdateArgs {
    UpdateArgs {
        channel: Some(ReleaseChannelArg::Stable),
        check: true,
        offline,
    }
}

#[tokio::test]
async fn check_queries_metadata_for_every_owner_without_changing_the_installation() {
    for marker in [
        None,
        Some("standalone-v1"),
        Some("pip-v1"),
        Some("foreign-v1"),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable = temp.path().join("cccc");
        std::fs::write(&executable, b"fixture executable").expect("executable");
        let marker_path = temp.path().join(INSTALL_MARKER);
        if let Some(marker) = marker {
            std::fs::write(&marker_path, marker).expect("marker");
        }
        let mut queried = false;
        let mut output = Vec::new();
        report(&mut output, &executable, "1.2.3", &args(false), async {
            queried = true;
            Ok("1.2.4".into())
        })
        .await
        .expect("check");
        let output = String::from_utf8(output).expect("text");
        assert!(
            queried,
            "every installation can check without gaining write ownership"
        );
        assert!(output.contains("Current version: 1.2.3"));
        assert!(output.contains("Latest stable version: 1.2.4"));
        if platform_requirement(
            std::env::consts::OS,
            std::env::consts::ARCH,
            cfg!(target_env = "musl"),
        )
        .0
        {
            assert!(output.contains("newer release available"));
            if marker == Some("pip-v1") {
                assert!(output.contains("cccc-pair==1.2.4"));
                assert!(!output.contains("Next step: cccc update"));
            }
        }
        assert_eq!(
            std::fs::read(&executable).expect("executable"),
            b"fixture executable"
        );
        assert_eq!(
            std::fs::read_to_string(&marker_path).ok().as_deref(),
            marker
        );
        assert_eq!(
            std::fs::read_dir(temp.path()).expect("directory").count(),
            1 + usize::from(marker.is_some())
        );
        assert_eq!(
            standalone_install_dir(&executable).is_ok(),
            marker == Some("standalone-v1")
        );
    }
}

#[tokio::test]
async fn offline_check_never_polls_release_discovery() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut queried = false;
    let mut output = Vec::new();
    report(
        &mut output,
        &temp.path().join("cccc"),
        "1.2.3",
        &args(true),
        async {
            queried = true;
            bail!("network must not be used")
        },
    )
    .await
    .expect("offline inspection");
    assert!(!queried);
    let output = String::from_utf8(output).expect("text");
    assert!(output.contains("Latest version: not checked (offline)"));
    assert!(!output.contains("already current"));
}

#[tokio::test]
async fn network_failure_keeps_local_diagnostics_and_does_not_claim_current() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join(INSTALL_MARKER), "pip-v1").expect("marker");
    let mut output = Vec::new();
    let error = report(
        &mut output,
        &temp.path().join("cccc"),
        "1.2.3",
        &args(false),
        async { bail!("fixture timeout") },
    )
    .await
    .expect_err("failed check");
    let output = String::from_utf8(output).expect("text");
    assert!(output.contains("Current version: 1.2.3"));
    assert!(output.contains("Installation: pip"));
    assert!(output.contains("update status is unknown"));
    assert!(!output.contains("already current"));
    assert!(format!("{error:#}").contains("fixture timeout"));
}

#[tokio::test]
async fn checks_distinguish_same_newer_and_explicit_cross_channel_releases() {
    if !platform_requirement(
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(target_env = "musl"),
    )
    .0
    {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join(INSTALL_MARKER), "pip-v1").expect("marker");
    for (current, latest, channel, expected) in [
        ("1.2.3+local", "1.2.3", None, "already current"),
        ("1.2.4", "1.2.3", None, "automatic downgrade is refused"),
        (
            "1.3.0-rc10",
            "1.3.0-rc2",
            Some(ReleaseChannelArg::Rc),
            "automatic downgrade is refused",
        ),
        (
            "1.3.0-rc2",
            "1.2.3",
            Some(ReleaseChannelArg::Stable),
            "channel switch available",
        ),
        (
            "1.3.0",
            "1.3.0-rc2",
            Some(ReleaseChannelArg::Rc),
            "channel switch available",
        ),
    ] {
        let mut output = Vec::new();
        let mut options = args(false);
        options.channel = channel;
        report(
            &mut output,
            &temp.path().join("cccc"),
            current,
            &options,
            async { Ok(latest.into()) },
        )
        .await
        .expect("check");
        let output = String::from_utf8(output).expect("text");
        assert!(output.contains(expected), "{output}");
        if expected != "channel switch available" {
            assert!(!output.contains("Next step:"));
        }
        if expected == "channel switch available" && channel == Some(ReleaseChannelArg::Rc) {
            assert!(output.contains("--pre --index-url https://test.pypi.org/simple/"));
        }
    }
}

#[test]
fn unsupported_builds_get_the_actual_migration_boundary() {
    assert!(!platform_requirement("linux", "aarch64", false).0);
    assert!(!platform_requirement("windows", "aarch64", false).0);
    assert!(
        platform_requirement("linux", "x86_64", true)
            .1
            .contains("musl/Alpine")
    );
    assert!(
        platform_requirement("macos", "x86_64", false)
            .1
            .contains("v0.4.37")
    );
    assert!(
        platform_requirement("linux", "x86_64", false)
            .1
            .contains("glibc 2.28")
    );
}
