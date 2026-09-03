use super::grok::{prepare_managed, record_managed};
use super::{read, write};
use cccc_core::HomeLayout;
use serde_json::json;
use std::collections::BTreeMap;

fn fixture() -> (tempfile::TempDir, HomeLayout, String, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir(&cwd).expect("workspace");
    (temp, home, "g_managed".into(), cwd)
}

#[test]
fn managed_receipt_resumes_only_the_same_command_workspace_and_identity() {
    let (_temp, home, group_id, cwd) = fixture();
    let command = vec!["grok".into(), "--model".into(), "grok-test".into()];
    let environment = BTreeMap::from([
        (
            "GROK_HOME".into(),
            cwd.join("grok-home").to_string_lossy().into_owned(),
        ),
        ("GROK_API_KEY".into(), "secret-not-fingerprinted".into()),
    ]);
    let session_id = uuid::Uuid::new_v4().to_string();
    record_managed(
        &home,
        &group_id,
        "peer1",
        &cwd,
        &command,
        &environment,
        &session_id,
        false,
        cccc_contracts::RunnerKind::Pty,
    )
    .expect("record");
    assert_eq!(
        prepare_managed(&home, &group_id, "peer1", &cwd, &command, &environment).expect("prepare"),
        Some(session_id)
    );

    let mut changed_secret = environment.clone();
    changed_secret.insert("GROK_API_KEY".into(), "rotated".into());
    assert!(
        prepare_managed(&home, &group_id, "peer1", &cwd, &command, &changed_secret,)
            .expect("secret rotation")
            .is_some()
    );

    let mut changed_identity = environment;
    changed_identity.insert(
        "GROK_HOME".into(),
        cwd.join("other-home").to_string_lossy().into_owned(),
    );
    assert!(
        prepare_managed(&home, &group_id, "peer1", &cwd, &command, &changed_identity,)
            .expect("identity change")
            .is_none()
    );
}

#[test]
fn legacy_raw_pty_receipt_is_not_resumed() {
    let (_temp, home, group_id, cwd) = fixture();
    let command = vec!["grok".into()];
    let environment = BTreeMap::new();
    let session_id = uuid::Uuid::new_v4().to_string();
    record_managed(
        &home,
        &group_id,
        "peer1",
        &cwd,
        &command,
        &environment,
        &session_id,
        false,
        cccc_contracts::RunnerKind::Pty,
    )
    .expect("record");
    let mut legacy = read(&home, &group_id, "peer1").expect("receipt");
    legacy.insert("v".into(), json!(1));
    legacy.remove("transport");
    write(&home, &group_id, "peer1", &legacy).expect("legacy receipt");

    assert!(
        prepare_managed(&home, &group_id, "peer1", &cwd, &command, &environment)
            .expect("prepare")
            .is_none()
    );
}
