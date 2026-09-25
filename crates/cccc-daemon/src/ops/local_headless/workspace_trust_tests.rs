use super::*;
use std::cell::Cell;

#[test]
fn launch_is_retried_only_after_claude_configuration_changes() {
    let temp = tempfile::tempdir().expect("trust records");
    let records = vec![temp.path().join(".claude.json")];
    // Polls: unchanged, changed (still refused), unchanged, changed (launches).
    let changes = [false, true, false, true];
    let poll = Cell::new(0);
    let launches = Cell::new(0);
    wait_for_trust(
        Duration::ZERO,
        &records,
        signature(&records),
        || {
            poll.set(poll.get() + 1);
            if changes[poll.get() - 1] {
                std::fs::write(&records[0], "x".repeat(poll.get())).expect("configuration change");
            }
            true
        },
        || {
            launches.set(launches.get() + 1);
            launches.get() == 2
        },
    );
    assert_eq!(poll.get(), 4);
    assert_eq!(launches.get(), 2);
}

#[test]
fn approval_before_the_watcher_starts_still_triggers_a_launch() {
    let temp = tempfile::tempdir().expect("trust records");
    let records = vec![temp.path().join(".claude.json")];
    // Snapshot precedes the interactive prompt; approval happens before monitoring.
    let before_prompt = signature(&records);
    std::fs::write(
        &records[0],
        r#"{"projects":{"/w":{"hasTrustDialogAccepted":true}}}"#,
    )
    .expect("approve while watcher has not started");
    let polls = Cell::new(0);
    let launched = Cell::new(false);
    wait_for_trust(
        Duration::ZERO,
        &records,
        before_prompt,
        || {
            polls.set(polls.get() + 1);
            polls.get() == 1
        },
        || {
            launched.set(true);
            true
        },
    );
    assert!(
        launched.get(),
        "approval must not wait for another configuration write"
    );
}

#[test]
fn a_closed_prompt_ends_the_wait_without_launching() {
    let polls = Cell::new(0);
    let launched = Cell::new(false);
    wait_for_trust(
        Duration::ZERO,
        &[],
        Vec::new(),
        || {
            polls.set(polls.get() + 1);
            polls.get() < 3
        },
        || {
            launched.set(true);
            true
        },
    );
    assert_eq!(polls.get(), 3);
    assert!(!launched.get());
}

#[test]
fn trust_records_cover_a_configured_directory_and_the_home_directory() {
    let env = BTreeMap::from([
        ("CLAUDE_CONFIG_DIR".to_owned(), "/claude-config".to_owned()),
        ("HOME".to_owned(), "/home/operator".to_owned()),
    ]);
    let records = trust_records(&env);
    assert_eq!(records[0], PathBuf::from("/claude-config/.claude.json"));
    assert!(records.contains(&PathBuf::from("/home/operator/.claude.json")));
    let unique = records
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(unique, records.len());
}

#[test]
fn accepting_trust_rewrites_the_record_and_changes_its_signature() {
    let temp = tempfile::tempdir().expect("tempdir");
    let record = temp.path().join(".claude.json");
    let records = vec![record.clone()];
    let missing = signature(&records);
    std::fs::write(
        &record,
        r#"{"projects":{"/w":{"hasTrustDialogAccepted":false}}}"#,
    )
    .expect("record");
    let untrusted = signature(&records);
    assert_ne!(missing, untrusted);
    std::fs::write(
        &record,
        r#"{"projects":{"/w":{"hasTrustDialogAccepted":true}}}"#,
    )
    .expect("record");
    assert_ne!(untrusted, signature(&records));
}

#[test]
fn userprofile_only_environments_watch_real_trust_changes() {
    let temp = tempfile::tempdir().expect("Windows home fixture");
    let profile = temp.path().to_string_lossy().into_owned();
    let env = BTreeMap::from([("USERPROFILE".into(), profile.clone())]);
    let child_records = trust_records_with(&env, |_| None);
    let inherited_records = trust_records_with(&BTreeMap::new(), |name| {
        (name == "USERPROFILE").then(|| profile.clone())
    });
    assert_eq!(child_records, vec![temp.path().join(".claude.json")]);
    assert_eq!(inherited_records, child_records);
    let before = signature(&inherited_records);
    std::fs::write(
        &inherited_records[0],
        r#"{"projects":{"workspace":{"hasTrustDialogAccepted":true}}}"#,
    )
    .expect("accept workspace trust");
    assert_ne!(signature(&inherited_records), before);
}

#[cfg(unix)]
#[test]
fn trust_prompt_runs_the_configured_command_in_the_actor_terminal() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .create("trust prompt", "")
        .expect("group");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let marker = temp.path().join("started-in");
    let executable = temp.path().join("claude");
    std::fs::write(
        &executable,
        "#!/bin/sh\npwd -P > \"$TRUST_PROMPT_MARKER\"\nexec sleep 30\n",
    )
    .expect("fake Claude");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
        .expect("executable permissions");
    let mut actor = Actor::new("claude");
    actor.runtime = ActorRuntime::Claude;
    actor.command = vec![executable.to_string_lossy().into_owned()];
    actor.env = BTreeMap::from([
        (
            "TRUST_PROMPT_MARKER".to_owned(),
            marker.to_string_lossy().into_owned(),
        ),
        ("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()),
    ]);
    let key = (group.group_id.clone(), actor.id.clone());

    prompt(
        &home,
        &group,
        &actor,
        key,
        workspace.clone(),
        io::Error::other("Workspace not trusted"),
    )
    .expect("the trust prompt opens instead of failing the start");

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !marker.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let started_in = std::fs::read_to_string(&marker).expect("configured command ran");
    let status = cccc_runtime::status(&group.group_id, &actor.id).expect("actor terminal");
    cccc_runtime::stop(&group.group_id, &actor.id).expect("stop");
    assert_eq!(
        PathBuf::from(started_in.trim()),
        workspace.canonicalize().expect("canonical workspace")
    );
    assert!(status.running);
}
