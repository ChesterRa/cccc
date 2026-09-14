/// Unix socket fixtures need a short, private root even when macOS TMPDIR is long.
/// Keep this choice local to the fixture rather than mutating process-wide TMPDIR.
pub(super) fn tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("cccc-grok-")
        .tempdir_in("/tmp")
        .expect("short Grok socket fixture directory")
}

#[test]
fn socket_fixture_works_with_a_long_system_temp_directory() {
    use cccc_core::HomeLayout;
    use std::collections::BTreeMap;
    use std::os::unix::net::UnixListener;
    use std::process::Command;

    const CHILD: &str = "CCCC_TEST_LONG_GROK_TMPDIR";
    if std::env::var_os(CHILD).is_none() {
        let parent = tempfile::tempdir().expect("long TMPDIR parent");
        let long_root = parent.path().join("long-temporary-directory-".repeat(6));
        std::fs::create_dir(&long_root).expect("long TMPDIR");
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "ops::codex_voice_analyst::tests::grok_session::socket_fixture::socket_fixture_works_with_a_long_system_temp_directory",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("TMPDIR", &long_root)
            .env("TMP", &long_root)
            .env("TEMP", &long_root)
            .output()
            .expect("run isolated long-TMPDIR regression");
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    assert!(std::env::temp_dir().to_string_lossy().len() > 96);
    let temp = tempdir();
    let home = HomeLayout::from_path(temp.path().join("cccc-home")).expect("home");
    home.initialize().expect("initialize home");
    let executable = super::fake_grok(temp.path());
    let prepared = super::grok::prepare(
        &home,
        &[executable.to_string_lossy().into_owned()],
        &BTreeMap::new(),
        "generation-voice-busy-race",
    )
    .expect("prepare Grok despite long system TMPDIR");
    let socket = prepared
        .leader_command
        .windows(2)
        .find(|parts| parts[0] == "--leader-socket")
        .expect("leader socket argument")[1]
        .clone();
    let _listener = UnixListener::bind(&socket).expect("bind actual Unix socket");
}
