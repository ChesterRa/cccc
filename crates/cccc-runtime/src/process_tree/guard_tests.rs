use super::*;
use std::os::unix::process::CommandExt;
use std::process::Child;
use std::time::Instant;

const T: u64 = 1_000_000;

#[test]
fn diagnostic_paths_stay_relative_and_reject_unexpected_names() {
    let ledger = std::path::Path::new("/private/home/daemon/owned-process-groups.json");
    assert_eq!(diagnostic_path(ledger, None).as_deref(), Some("daemon"));
    assert_eq!(
        diagnostic_path(
            ledger,
            Some(std::ffi::OsStr::new("owned-process-groups.json.tmp"))
        )
        .as_deref(),
        Some("daemon/owned-process-groups.json.tmp")
    );
    assert!(diagnostic_path(ledger, Some(std::ffi::OsStr::new("a/b"))).is_none());
    assert!(diagnostic_path(std::path::Path::new("/private/other/owned.json"), None).is_none());
}

fn test_owned_group(pgid: i32, spawned_at: u64) -> OwnedGroup {
    OwnedGroup {
        pgid,
        spawned_at,
        id: uuid::Uuid::new_v4(),
        origin: None,
        evidence: GroupEvidence::Unconfirmed,
    }
}

#[test]
fn empty_active_ledger_is_readable_by_the_previous_version() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut child = KillOnDrop(
        Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("fixture watchdog"),
    );
    let ledger = temp.path().join("owned.json");
    let mut active = Guard {
        ledger: ledger.clone(),
        watch_list: temp.path().join("owned.fixture.watchdog"),
        watchdog: child.0.stdin.take().expect("fixture pipe"),
        groups: Vec::new(),
        retained: Vec::new(),
        intents: Vec::new(),
        sources_clean: true,
        retrying: Default::default(),
    };
    persist(&mut active).expect("persist empty active guard");
    let bytes = std::fs::read(&ledger).expect("read ledger");
    let old: Vec<LedgerEntry> = serde_json::from_slice(&bytes).expect("previous reader");
    assert!(old.is_empty());
    assert!(
        read_ledger_records(&ledger)
            .expect("current reader")
            .is_empty()
    );
}

#[test]
fn empty_offline_ledger_is_legacy_readable_but_an_intent_is_not() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    persist_locked_sources(&ledger, Vec::new(), &[]).expect("persist empty ledger");
    let empty = std::fs::read(&ledger).expect("read empty ledger");
    let old: Vec<LedgerEntry> = serde_json::from_slice(&empty).expect("previous reader");
    assert!(old.is_empty());
    assert!(
        read_ledger_records(&ledger)
            .expect("current reader")
            .is_empty()
    );

    let intent = Record::LaunchIntent(LaunchIntent {
        id: uuid::Uuid::new_v4(),
        source_id: None,
        created_at: 123,
        origin: None,
    });
    persist_locked_sources(&ledger, vec![intent.clone()], &[]).expect("persist intent");
    let nonempty = std::fs::read(&ledger).expect("read intent");
    assert!(serde_json::from_slice::<Vec<LedgerEntry>>(&nonempty).is_err());
    assert_eq!(
        read_ledger_records(&ledger).expect("current reader"),
        [intent]
    );
}

#[test]
fn empty_watch_is_readable_by_the_previous_and_current_versions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let watch = temp.path().join("owned.fixture.watchdog");
    write_watch_list_at(&watch, &[], 123).expect("write empty watch");
    let text = std::fs::read_to_string(watch).expect("read empty watch");
    let mut old_fields = text.split_whitespace();
    assert_eq!(old_fields.next(), Some("123"));
    assert!(
        old_fields.next().is_none(),
        "previous reader expects no records"
    );
    assert!(
        parse_watch_text(&text, b"fixture")
            .expect("current reader")
            .is_empty()
    );
}

#[cfg(target_os = "linux")]
fn captured_test_group(child: &Child, spawned_at: u64) -> OwnedGroup {
    let view = super::super::linux_identity::ProcView::open().expect("local procfs view");
    let identity = view
        .capture_child(child.id() as i32)
        .expect("capture fixture child");
    OwnedGroup {
        pgid: child.id() as i32,
        spawned_at,
        id: uuid::Uuid::new_v4(),
        origin: Some(identity.origin.clone()),
        evidence: GroupEvidence::Captured(identity),
    }
}

#[test]
fn evidence_reader_imports_legacy_without_claiming_a_new_identity() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("owned.json");
    let legacy = br#"[{"pgid":400,"spawned_at":100,"alive_until":150}]"#;
    std::fs::write(&path, legacy).expect("write legacy evidence");
    let records = read_ledger_records(&path).expect("import evidence");
    let [Record::ProcessGroup(group)] = records.as_slice() else {
        panic!("expected one imported process group");
    };
    assert_eq!(
        (group.pgid, group.spawned_at, group.observed_at),
        (400, 100, 130)
    );
    assert_eq!(group.evidence, GroupEvidence::Legacy);
    assert!(group.origin.is_none());
    assert_eq!(std::fs::read(&path).expect("read original"), legacy);
}

#[test]
fn evidence_reader_never_replaces_a_malformed_ledger() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("owned.json");
    for bytes in [b"{".as_slice(), b"[] trailing".as_slice(), b"{}".as_slice()] {
        std::fs::write(&path, bytes).expect("write malformed evidence");
        assert!(read_ledger_records(&path).is_err());
        assert_eq!(std::fs::read(&path).expect("preserved evidence"), bytes);
    }
}

#[test]
fn interrupted_ledger_publications_are_inputs_not_overwritten_scratch() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("owned.json");
    let old_temporary = temp.path().join("owned.json.tmp");
    let old = br#"[{"pgid":400,"spawned_at":100,"observed_at":130}]"#;
    std::fs::write(&old_temporary, old).expect("old interrupted publication");

    let intent = LaunchIntent {
        id: uuid::Uuid::new_v4(),
        source_id: None,
        created_at: 200,
        origin: None,
    };
    let current = Ledger::new(vec![Record::LaunchIntent(intent.clone())]);
    write_ledger_atomically(&path, &serde_json::to_vec(&current).expect("serialize"))
        .expect("publish without overwriting old temporary");
    assert_eq!(std::fs::read(&old_temporary).expect("old evidence"), old);

    let (records, sources) = read_ledger_sources(&path).expect("read both sources");
    assert!(records.contains(&Record::LaunchIntent(intent)));
    assert!(records.iter().any(|record| matches!(
        record,
        Record::ProcessGroup(group) if group.pgid == 400 && group.evidence == GroupEvidence::Legacy
    )));
    assert_eq!(sources, vec![old_temporary]);
}

#[test]
fn interrupted_watch_publications_are_not_overwritten_by_a_later_write() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    let watch = temp.path().join("owned.instance.watchdog");
    let old_temporary = temp.path().join("owned.instance.watchdog.tmp");
    let old = b"130 400:100\n";
    std::fs::write(&old_temporary, old).expect("old interrupted watch");
    write_watch_list(&watch, &[]).expect("publish a later watch");
    assert_eq!(std::fs::read(&old_temporary).expect("old watch"), old);
    let (entries, sources) = read_recovery_watch_lists(&ledger).expect("read both watches");
    assert!(entries.iter().any(|entry| matches!(
        entry,
        Record::ProcessGroup(group) if group.pgid == 400
    )));
    assert!(sources.contains(&old_temporary));
    assert!(sources.contains(&watch));
}

#[test]
fn incomplete_ledger_temporary_blocks_recovery_and_remains_unchanged() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("owned.json");
    let partial = temp.path().join("owned.json.tmp");
    std::fs::write(&path, b"[]").expect("old ledger");
    for bytes in [b"".as_slice(), b"[{\"pgid\":400".as_slice()] {
        std::fs::write(&partial, bytes).expect("interrupted publication");
        assert!(read_ledger_sources(&path).is_err());
        assert_eq!(std::fs::read(&partial).expect("preserved temporary"), bytes);
    }
}

#[test]
fn a_failed_write_removes_only_its_own_unfinished_file() {
    // A full disk must not leave an unreadable file that stops the next owner.
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("owned.json");
    let earlier = temp.path().join("owned.json.tmp");
    std::fs::write(&path, b"[]").expect("committed ledger");
    std::fs::write(&earlier, b"[{\"pgid\":400").expect("earlier interrupted publication");
    let error = publish_atomically(&path, |file| {
        file.write_all(b"[{\"pgid\":")?;
        Err(io::Error::other("disk full"))
    })
    .expect_err("the write fails");
    assert_eq!(error.to_string(), "disk full");
    let mut names = std::fs::read_dir(temp.path())
        .expect("ledger directory")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["owned.json", "owned.json.tmp"]);
    assert_eq!(
        std::fs::read(&earlier).expect("earlier evidence"),
        b"[{\"pgid\":400"
    );
    assert_eq!(std::fs::read(&path).expect("committed ledger"), b"[]");
}

#[test]
fn a_complete_file_whose_rename_fails_stays_as_evidence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("owned.json");
    // A nonempty directory in the ledger's place makes the rename fail.
    std::fs::create_dir(&path).expect("blocking directory");
    std::fs::write(path.join("keep"), b"").expect("nonempty directory");
    write_ledger_atomically(&path, b"[]").expect_err("rename fails");
    let kept = std::fs::read_dir(temp.path())
        .expect("ledger directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|candidate| candidate != &path)
        .collect::<Vec<_>>();
    assert_eq!(kept.len(), 1);
    assert_eq!(
        std::fs::read(&kept[0]).expect("complete publication"),
        b"[]"
    );
}

fn unconfirmed_record(pgid: i32, origin: Option<Origin>) -> Record {
    Record::ProcessGroup(GroupRecord {
        id: uuid::Uuid::new_v4(),
        source_id: None,
        pgid,
        spawned_at: 100,
        observed_at: 130,
        origin,
        evidence: GroupEvidence::Unconfirmed,
    })
}

#[test]
fn without_identity_only_an_ended_leader_and_group_release_a_record() {
    // The policy for platforms without a stronger identity, such as macOS.
    let mut ended = Command::new("/bin/sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .expect("ended fixture");
    let ended_pid = ended.id() as i32;
    ended.kill().expect("end fixture");
    ended.wait().expect("reap fixture");
    assert!(!keep_without_identity(&unconfirmed_record(ended_pid, None)));

    // A leader outside the group its number names: the group is gone, the leader is not.
    let outside = KillOnDrop(
        Command::new("/bin/sleep")
            .arg("60")
            .spawn()
            .expect("leader outside its group"),
    );
    let outside_pid = outside.0.id() as i32;
    assert!(group_absent(outside_pid));
    assert!(keep_without_identity(&unconfirmed_record(
        outside_pid,
        None
    )));

    let running = KillOnDrop(
        Command::new("/bin/sleep")
            .arg("60")
            .process_group(0)
            .spawn()
            .expect("running group"),
    );
    assert!(keep_without_identity(&unconfirmed_record(
        running.0.id() as i32,
        None
    )));

    let elsewhere = Origin {
        boot_id: "00000000-0000-4000-8000-000000000000".into(),
        pid_namespace: 1,
        time_namespace: None,
    };
    assert!(keep_without_identity(&unconfirmed_record(
        ended_pid,
        Some(elsewhere)
    )));
    assert!(keep_without_identity(&Record::LaunchIntent(LaunchIntent {
        id: uuid::Uuid::new_v4(),
        source_id: None,
        created_at: 100,
        origin: None,
    })));
}

#[test]
fn replaying_the_same_legacy_source_does_not_duplicate_its_record() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("owned.json");
    let partial = temp.path().join("owned.json.tmp");
    std::fs::write(
        &partial,
        br#"[{"pgid":400,"spawned_at":100,"observed_at":130}]"#,
    )
    .expect("previous source");
    let (first, _) = read_ledger_sources(&path).expect("first read");
    write_ledger_atomically(
        &path,
        &serde_json::to_vec(&Ledger::new(first.clone())).expect("serialize first ledger"),
    )
    .expect("absorb before simulated interruption");
    let (mut replayed, _) = read_ledger_sources(&path).expect("read after interruption");
    normalize_source_records(&mut replayed).expect("normalize");
    assert_eq!(replayed, first);
}

#[test]
fn conflicting_snapshots_keep_both_evidence_items() {
    let id = uuid::Uuid::new_v4();
    let first = Record::LaunchIntent(LaunchIntent {
        id,
        source_id: None,
        created_at: 100,
        origin: None,
    });
    let second = Record::ProcessGroup(GroupRecord {
        id,
        source_id: None,
        pgid: 400,
        spawned_at: 100,
        observed_at: 101,
        origin: None,
        evidence: GroupEvidence::Unconfirmed,
    });
    let mut records = vec![first.clone(), first.clone(), second];
    let mut repeated = records.clone();
    normalize_source_records(&mut records).expect("normalize");
    normalize_source_records(&mut repeated).expect("repeat normalization");
    assert_eq!(records, repeated);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0], first);
    assert_ne!(records[0].id(), records[1].id());
    let bytes = serde_json::to_vec(&Ledger::new(records.clone())).expect("serialize");
    assert_eq!(
        Ledger::from_bytes(&bytes).expect("valid ledger").records,
        records
    );
}

#[test]
fn replay_after_interrupted_conflict_merge_does_not_invent_more_records() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    let old_temp = temp.path().join("owned.json.tmp");
    let id = uuid::Uuid::new_v4();
    let intent = Record::LaunchIntent(LaunchIntent {
        id,
        source_id: None,
        created_at: 100,
        origin: None,
    });
    let group = Record::ProcessGroup(GroupRecord {
        id,
        source_id: None,
        pgid: 400,
        spawned_at: 100,
        observed_at: 101,
        origin: None,
        evidence: GroupEvidence::Unconfirmed,
    });
    std::fs::write(
        &ledger,
        serde_json::to_vec(&Ledger::new(vec![intent])).expect("serialize intent"),
    )
    .expect("write initial ledger");
    std::fs::write(
        &old_temp,
        serde_json::to_vec(&Ledger::new(vec![group])).expect("serialize group"),
    )
    .expect("write interrupted source");
    let (expected, _) = locked_sources(&ledger).expect("first merge");
    assert_eq!(expected.len(), 2);
    for _ in 0..3 {
        // Model a crash after publishing the combined ledger but before
        // deleting the absorbed old temporary source.
        write_ledger_atomically(
            &ledger,
            &serde_json::to_vec(&Ledger::new(expected.clone())).expect("serialize merged ledger"),
        )
        .expect("publish merged ledger");
        let (again, _) = locked_sources(&ledger).expect("replayed merge");
        assert_eq!(
            again, expected,
            "replaying the source must not create another item"
        );
    }
    // Recovery may legitimately clear the first conflicting record before the
    // old source is removed. The surviving normalized record still identifies
    // that source even though its original ID is no longer occupied.
    let survivor = vec![expected[1].clone()];
    write_ledger_atomically(
        &ledger,
        &serde_json::to_vec(&Ledger::new(survivor.clone())).expect("serialize surviving evidence"),
    )
    .expect("publish surviving evidence");
    let (again, _) = locked_sources(&ledger).expect("replay after one record cleared");
    assert_eq!(again, survivor);
}

#[test]
fn protected_intent_survives_heartbeat_and_unstarted_cleanup() {
    const MODE: &str = "CCCC_TEST_GUARD_INTENT";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::protected_intent_survives_heartbeat_and_unstarted_cleanup",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    enable(&ledger).expect("enable guard");
    let intent = begin_intent()
        .expect("publish intent")
        .expect("guard active");
    assert!(
        !recovery_pending(),
        "a launch still in progress is not abandoned"
    );
    assert!(list_active(&ledger).expect("review").records.is_empty());
    {
        let mut current = guard();
        persist(current.as_mut().expect("guard")).expect("heartbeat publication");
    }
    let records = read_ledger_records(&ledger).expect("read published intent");
    assert!(records.contains(&Record::LaunchIntent(intent.entry.clone())));
    cancel_unstarted_intent(intent.entry.id).expect("confirmed not started");
    assert!(
        read_ledger_records(&ledger)
            .expect("read after cancel")
            .is_empty()
    );
}

#[test]
fn acceptance_is_fenced_to_one_reviewed_version_and_preserves_other_records() {
    const MODE: &str = "CCCC_TEST_GUARD_ACCEPT";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::acceptance_is_fenced_to_one_reviewed_version_and_preserves_other_records",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    enable(&ledger).expect("enable guard");
    for _ in 0..2 {
        let intent = begin_intent().expect("intent").expect("active");
        retain_uncertain_intent(intent.entry.id).expect("retain");
    }
    let snapshot = list_active(&ledger).expect("list pending");
    assert_eq!(snapshot.records.len(), 2);
    let first = &snapshot.records[0];
    assert_eq!(
        accept_active(&ledger, &first.id, "stale"),
        Err(RecoveryError::Changed)
    );
    assert_eq!(
        list_active(&ledger).expect("still pending").records.len(),
        2
    );
    let other = temp.path().join("different-owner.json");
    assert!(list_active(&other).is_err());
    assert_eq!(
        accept_active(&other, &first.id, &first.version),
        Err(RecoveryError::Unavailable)
    );
    accept_active(&ledger, &first.id, &first.version).expect("accept exactly one");
    assert_eq!(
        accept_active(&ledger, &first.id, &first.version),
        Err(RecoveryError::Changed)
    );
    let remaining = list_active(&ledger).expect("remaining record");
    assert_eq!(remaining.records.len(), 1);
    assert_eq!(remaining.records[0].id, snapshot.records[1].id);
    assert!(recovery_pending());
    let second = &remaining.records[0];
    accept_active(&ledger, &second.id, &second.version).expect("accept second");
    assert!(!recovery_pending());
    assert!(
        read_ledger_records(&ledger)
            .expect("read ledger")
            .is_empty()
    );
}

#[test]
fn acceptance_is_blocked_when_a_prior_publication_is_unconfirmed() {
    use std::os::unix::fs::PermissionsExt;
    const MODE: &str = "CCCC_TEST_GUARD_ACCEPT_UNCONFIRMED";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::acceptance_is_blocked_when_a_prior_publication_is_unconfirmed",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let guarded = temp.path().join("guard");
    std::fs::create_dir(&guarded).expect("guard directory");
    let ledger = guarded.join("owned.json");
    enable(&ledger).expect("enable guard");
    let intent = begin_intent().expect("intent").expect("active");
    retain_uncertain_intent(intent.entry.id).expect("retain");
    let item = list_active(&ledger)
        .expect("list pending")
        .records
        .remove(0);

    std::fs::set_permissions(&guarded, std::fs::Permissions::from_mode(0o500))
        .expect("deny publication");
    assert!(begin_intent().is_err());
    std::fs::set_permissions(&guarded, std::fs::Permissions::from_mode(0o700))
        .expect("restore directory");
    let snapshot = list_active(&ledger).expect("list pending after failure");
    assert!(!snapshot.records[0].acceptance_supported);
    assert!(!snapshot.diagnostics.is_empty());
    assert_eq!(
        accept_active(&ledger, &item.id, &item.version),
        Err(RecoveryError::Unavailable)
    );
    assert!(recovery_pending());
}

#[test]
fn offline_accept_absorbs_sources_before_removing_only_the_reviewed_item() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    let old_temp = temp.path().join("owned.json.tmp");
    let old_watch = temp.path().join("owned.old.watchdog");
    std::fs::write(&ledger, b"[]").expect("empty ledger");
    std::fs::write(
        &old_temp,
        br#"[{"pgid":400,"spawned_at":100,"observed_at":130}]"#,
    )
    .expect("legacy temporary");
    std::fs::write(&old_watch, b"140 500:110\n").expect("legacy watch");
    let snapshot = list_locked_ledger(&ledger).expect("list evidence");
    assert_eq!(snapshot.records.len(), 2);
    let first = &snapshot.records[0];
    assert_eq!(
        accept_locked_ledger(&ledger, &first.id, "stale"),
        Err(RecoveryError::Changed)
    );
    assert!(old_temp.exists() && old_watch.exists());
    accept_locked_ledger(&ledger, &first.id, &first.version).expect("accept one source item");
    assert!(!old_temp.exists() && !old_watch.exists());
    let remaining = list_locked_ledger(&ledger).expect("list remaining");
    assert_eq!(remaining.records.len(), 1);
    assert_eq!(remaining.records[0].id, snapshot.records[1].id);
    assert_eq!(
        accept_locked_ledger(&ledger, &first.id, &first.version),
        Err(RecoveryError::Changed)
    );
}

#[test]
fn offline_review_can_accept_one_exact_readable_malformed_artifact() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    let partial = temp.path().join("owned.json.tmp");
    std::fs::write(&ledger, b"[]").expect("empty ledger");
    std::fs::write(&partial, b"{").expect("partial evidence");
    let snapshot = list_locked_ledger(&ledger).expect("diagnostic snapshot");
    assert_eq!(snapshot.records.len(), 1);
    let item = &snapshot.records[0];
    assert_eq!(item.kind, "opaque_evidence");
    assert_eq!(item.reason, "evidence_invalid");
    assert!(item.acceptance_supported);
    assert_eq!(
        retry_locked_ledger(&ledger, &item.id, &item.version),
        Ok(RetryResult::StillPending)
    );
    assert_eq!(std::fs::read(&partial).expect("preserved"), b"{");
    std::fs::write(&partial, b"{changed").expect("change source");
    assert_eq!(
        accept_locked_ledger(&ledger, &item.id, &item.version),
        Err(RecoveryError::Changed)
    );
    let changed = list_locked_ledger(&ledger).expect("refresh");
    let changed = &changed.records[0];
    assert_eq!(changed.id, item.id);
    assert_ne!(changed.version, item.version);
    accept_locked_ledger(&ledger, &changed.id, &changed.version).expect("accept exact artifact");
    assert!(!partial.exists());
    assert_eq!(std::fs::read(&ledger).expect("other file unchanged"), b"[]");
}

#[test]
fn offline_review_cannot_accept_evidence_it_cannot_read() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    std::fs::write(&ledger, b"private").expect("fixture");
    std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o000)).expect("deny read");
    if File::open(&ledger).is_ok() {
        std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o600))
            .expect("restore permissions");
        return;
    }
    let result = list_locked_ledger(&ledger).expect("diagnostic snapshot");
    std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o600))
        .expect("restore permissions");
    assert!(result.records.is_empty());
    assert_eq!(result.diagnostics[0].reason, "evidence_unreadable");
    assert_eq!(result.diagnostics[0].hint, "inspect_permissions");
    assert_eq!(std::fs::read(&ledger).expect("preserved"), b"private");
}

#[test]
fn enable_absorbs_watch_only_and_interrupted_ledger_evidence_without_signalling() {
    const MODE: &str = "CCCC_TEST_GUARD_ABSORB_SOURCES";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::enable_absorbs_watch_only_and_interrupted_ledger_evidence_without_signalling",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    let old_watch = temp.path().join("owned.old.watchdog");
    let old_temp = temp.path().join("owned.json.tmp");
    let mut child = KillOnDrop(orphan_leader());
    let pid = child.0.id() as i32;
    let started = now();
    std::fs::write(&ledger, b"[]").expect("stale empty ledger");
    std::fs::write(&old_watch, format!("{started} {pid}:{started}\n")).expect("watch only");
    std::fs::write(
        &old_temp,
        format!("[{{\"pgid\":{pid},\"spawned_at\":{started},\"observed_at\":{started}}}]"),
    )
    .expect("interrupted ledger");
    enable(&ledger).expect("absorb previous sources");
    assert!(
        child.0.try_wait().expect("poll child").is_none(),
        "legacy evidence must not signal"
    );
    let records = read_ledger_records(&ledger).expect("persisted evidence");
    assert_eq!(
        records.iter().filter(|record| matches!(
            record,
            Record::ProcessGroup(group) if group.pgid == pid && group.evidence == GroupEvidence::Legacy
        )).count(),
        2,
        "each source remains independently represented"
    );
    assert!(!old_watch.exists());
    assert!(!old_temp.exists());
    child.0.kill().expect("fixture cleanup");
    child.0.wait().expect("reap fixture");
    refresh_active(&ledger).expect("absent group no longer pending");
}

#[test]
fn failed_pre_spawn_intent_write_refuses_to_run_the_command() {
    use std::os::unix::fs::PermissionsExt;
    const MODE: &str = "CCCC_TEST_GUARD_INTENT_WRITE_FAILURE";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::failed_pre_spawn_intent_write_refuses_to_run_the_command",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let guarded = temp.path().join("guard");
    std::fs::create_dir(&guarded).expect("guard directory");
    let ledger = guarded.join("owned.json");
    enable(&ledger).expect("enable guard");
    std::fs::set_permissions(&guarded, std::fs::Permissions::from_mode(0o500))
        .expect("deny publication");
    let marker = temp.path().join("started");
    let result = crate::OwnedProcessTree::spawn_registered(Command::new("touch").arg(&marker), 0);
    std::fs::set_permissions(&guarded, std::fs::Permissions::from_mode(0o700))
        .expect("restore directory");
    assert!(matches!(result, Err(crate::CommandError::NotStarted(_))));
    assert!(
        !marker.exists(),
        "command ran despite failed pre-spawn evidence"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn failed_post_spawn_publication_reports_started_and_keeps_unknown_intent() {
    use std::os::unix::fs::PermissionsExt;
    const MODE: &str = "CCCC_TEST_GUARD_POST_SPAWN_FAILURE";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::failed_post_spawn_publication_reports_started_and_keeps_unknown_intent",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let guarded = temp.path().join("guard");
    std::fs::create_dir(&guarded).expect("guard directory");
    let ledger = guarded.join("owned.json");
    enable(&ledger).expect("enable guard");
    let mut registry = super::super::registry();
    let intent = begin_intent()
        .expect("publish intent")
        .expect("guard active");
    let intent_id = intent.entry.id;
    let (mut child, resource) = super::super::spawn::standard(
        Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        0,
    )
    .expect("fixture child started");
    let _child_cleanup = match FixtureProcess::pin(child.id() as i32, std::process::id()) {
        Ok(pinned) => pinned,
        Err(_) => {
            // Closing the pipe lets our fixture exit without signalling an
            // identity that could not be verified.
            drop(child.stdin.take());
            let _ = child.wait();
            return;
        }
    };
    std::fs::set_permissions(&guarded, std::fs::Permissions::from_mode(0o500))
        .expect("deny post-spawn publication");
    let result = super::super::OwnedProcessTree::finish_started(
        child,
        resource,
        &mut registry,
        Some(intent),
    );
    std::fs::set_permissions(&guarded, std::fs::Permissions::from_mode(0o700))
        .expect("restore directory");
    drop(registry);
    assert!(matches!(result, Err(crate::CommandError::Started(_))));
    assert!(recovery_pending(), "unconfirmed launch must block recovery");
    assert!(
        read_ledger_records(&ledger)
            .expect("read previous intent")
            .iter()
            .any(|record| {
                matches!(record, Record::LaunchIntent(entry) if entry.id == intent_id)
            })
    );
}

#[cfg(target_os = "linux")]
#[test]
fn failed_publication_stops_a_child_that_left_its_original_group() {
    use std::time::{Duration, Instant};

    const MODE: &str = "CCCC_TEST_GUARD_MOVED_CHILD";
    match std::env::var(MODE).as_deref() {
        Err(_) => {
            let output = Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "process_tree::guard::tests::failed_publication_stops_a_child_that_left_its_original_group",
                    "--nocapture",
                ])
                .env(MODE, "parent")
                .output()
                .expect("isolated helper");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        Ok("target") => {
            let group = std::env::var("CCCC_TEST_GUARD_TARGET_GROUP")
                .expect("group")
                .parse::<i32>()
                .expect("numeric group");
            rustix::process::setpgid(
                None,
                Some(rustix::process::Pid::from_raw(group).expect("positive group")),
            )
            .expect("move fixture out of its original group");
            std::fs::write(
                std::env::var_os("CCCC_TEST_GUARD_READY").expect("marker"),
                b"ready",
            )
            .expect("signal readiness");
            // A broken cleanup eventually unblocks on its own, so the test
            // cannot hang indefinitely or signal an unrelated process.
            std::thread::sleep(Duration::from_secs(3));
            return;
        }
        Ok("parent") => {}
        _ => panic!("unexpected fixture mode"),
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let guarded = temp.path().join("guard");
    std::fs::create_dir(&guarded).expect("guard directory");
    let ledger = guarded.join("owned.json");
    enable(&ledger).expect("enable guard");
    let mut registry = super::super::registry();
    let intent = begin_intent()
        .expect("publish intent")
        .expect("guard active");
    let marker = temp.path().join("ready");
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "--exact",
            "process_tree::guard::tests::failed_publication_stops_a_child_that_left_its_original_group",
            "--nocapture",
        ])
        .env(MODE, "target")
        .env(
            "CCCC_TEST_GUARD_TARGET_GROUP",
            rustix::process::getpgrp().as_raw_pid().to_string(),
        )
        .env("CCCC_TEST_GUARD_READY", &marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let (mut child, resource) =
        super::super::spawn::standard(&mut command, 0).expect("fixture child started");
    let _child_cleanup = match FixtureProcess::pin(child.id() as i32, std::process::id()) {
        Ok(pinned) => pinned,
        Err(_) => {
            // On a kernel without pidfds, let this finite fixture exit rather
            // than signal a numeric identity we could not pin.
            let _ = child.wait();
            return;
        }
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "fixture did not move groups");
        std::thread::sleep(Duration::from_millis(5));
    }
    let pid = rustix::process::Pid::from_raw(child.id() as i32).expect("fixture pid");
    assert_eq!(
        rustix::process::getpgid(Some(pid)).expect("child group"),
        rustix::process::getpgrp()
    );
    assert_eq!(
        rustix::process::test_kill_process_group(pid),
        Err(rustix::io::Errno::SRCH),
        "the original fixture group must be empty"
    );

    // A regular file in place of the guard directory makes publication fail
    // even when the test runs with privileges that bypass chmod restrictions.
    let parked = temp.path().join("parked-guard");
    std::fs::rename(&guarded, &parked).expect("park fixture guard directory");
    std::fs::write(&guarded, b"not a directory").expect("block fixture publication");
    let started = Instant::now();
    let result = super::super::OwnedProcessTree::finish_started(
        child,
        resource,
        &mut registry,
        Some(intent),
    );
    let elapsed = started.elapsed();
    std::fs::remove_file(&guarded).expect("remove fixture blocker");
    std::fs::rename(&parked, &guarded).expect("restore fixture directory");
    drop(registry);
    assert!(matches!(result, Err(crate::CommandError::Started(_))));
    assert!(
        elapsed < Duration::from_millis(1500),
        "cleanup waited for the still-running child: {elapsed:?}"
    );
    assert!(recovery_pending(), "unconfirmed launch remains visible");
}

#[test]
fn ordinary_spawn_failure_clears_its_confirmed_unstarted_intent() {
    const MODE: &str = "CCCC_TEST_GUARD_NO_EXECUTABLE";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::ordinary_spawn_failure_clears_its_confirmed_unstarted_intent",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    enable(&ledger).expect("enable guard");
    let result = crate::OwnedProcessTree::spawn_registered(
        &mut Command::new(temp.path().join("missing-executable")),
        0,
    );
    assert!(matches!(result, Err(crate::CommandError::NotStarted(_))));
    assert!(
        read_ledger_records(&ledger)
            .expect("read evidence")
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn missing_pty_program_does_not_leave_an_unknown_launch() {
    const MODE: &str = "CCCC_TEST_GUARD_MISSING_PTY";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::missing_pty_program_does_not_leave_an_unknown_launch",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    enable(&ledger).expect("enable guard");
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("pty");
    let command = portable_pty::CommandBuilder::new(temp.path().join("missing-executable"));
    let result = crate::OwnedProcessTree::spawn_pty(|| {
        pair.slave
            .spawn_command(command)
            .map_err(crate::session::native_pty_spawn_error)
    });
    assert!(result.is_err());
    assert!(
        !recovery_pending(),
        "a confirmed missing program cannot leave a pending launch"
    );
    assert!(read_ledger_records(&ledger).expect("ledger").is_empty());
}

#[test]
fn pty_child_setup_error_does_not_leave_an_unknown_launch() {
    const MODE: &str = "CCCC_TEST_GUARD_PTY_CHDIR";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::pty_child_setup_error_does_not_leave_an_unknown_launch",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    enable(&ledger).expect("enable guard");
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("pty");
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    let marker = temp.path().join("child-executed");
    command.args(["-c", "touch \"$1\"", "--"]);
    command.arg(&marker);
    command.cwd(temp.path().join("missing-directory"));
    // portable-pty replaces a missing cwd with HOME, so make the fixture's
    // child-only HOME missing as well to reach std's pre-exec chdir failure.
    command.env("HOME", temp.path().join("missing-home"));
    let result = crate::OwnedProcessTree::spawn_pty(|| {
        pair.slave
            .spawn_command(command)
            .map_err(crate::session::native_pty_spawn_error)
    });
    assert!(
        result.is_err(),
        "the child must fail before executing the shell"
    );
    assert!(
        !marker.exists(),
        "the target program must not have executed"
    );
    assert!(!recovery_pending());
    assert!(read_ledger_records(&ledger).expect("ledger").is_empty());
}

#[test]
fn panicking_pty_spawn_keeps_the_intent_visible_to_the_live_owner() {
    const MODE: &str = "CCCC_TEST_GUARD_PANICKING_PTY";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::panicking_pty_spawn_keeps_the_intent_visible_to_the_live_owner",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    enable(&ledger).expect("enable guard");
    let result = std::panic::catch_unwind(|| {
        let _ = crate::OwnedProcessTree::spawn_pty(|| panic!("injected spawn panic"));
    });
    assert!(
        result.is_err(),
        "the panic must not be converted to NotStarted"
    );
    assert!(
        recovery_pending(),
        "an uncertain launch must remain visible"
    );
    assert!(matches!(
        read_ledger_records(&ledger)
            .expect("persisted intent")
            .as_slice(),
        [Record::LaunchIntent(_)]
    ));
    let snapshot = list_active(&ledger).expect("review pending launch");
    assert_eq!(snapshot.records.len(), 1);
    let item = &snapshot.records[0];
    accept_active(&ledger, &item.id, &item.version).expect("accept reviewed intent");
    assert!(!recovery_pending());
    assert!(read_ledger_records(&ledger).expect("ledger").is_empty());
}

#[test]
fn abandoning_intent_does_not_wait_for_guard_mutex() {
    const MODE: &str = "CCCC_TEST_GUARD_INTENT_DROP";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::abandoning_intent_does_not_wait_for_guard_mutex",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = temp.path().join("owned.json");
    enable(&ledger).expect("enable guard");
    let intent = begin_intent().expect("begin").expect("active");
    let lock = guard();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        drop(intent);
        tx.send(()).expect("report drop");
    });
    let completed_while_locked = rx.recv_timeout(Duration::from_millis(500));
    drop(lock);
    worker.join().expect("drop worker");
    assert!(
        completed_while_locked.is_ok(),
        "drop waited for guard mutex"
    );
    assert!(recovery_pending());
    assert_eq!(
        list_active(&ledger)
            .expect("review pending launch")
            .records
            .len(),
        1
    );
}

#[cfg(target_os = "linux")]
#[test]
fn owner_death_after_spawn_before_registration_leaves_the_intent() {
    const MODE: &str = "CCCC_TEST_GUARD_PREPUBLISH_CRASH";
    const DIRECTORY: &str = "CCCC_TEST_GUARD_PREPUBLISH_DIR";
    if std::env::var_os(MODE).is_some() {
        let dir = PathBuf::from(std::env::var_os(DIRECTORY).expect("fixture dir"));
        enable(&dir.join("owned.json")).expect("enable guard");
        let _intent = begin_intent()
            .expect("publish intent")
            .expect("guard active");
        let (mut child, _resource) = super::super::spawn::standard(
            Command::new("sleep")
                .arg("60")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
            0,
        )
        .expect("fixture child started before registration");
        std::fs::write(dir.join("child.pid"), child.id().to_string()).expect("publish child PID");
        let mut command = [0_u8; 1];
        std::io::Read::read_exact(&mut std::io::stdin(), &mut command).expect("parent control");
        if command == [b'C'] {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        std::thread::sleep(Duration::from_secs(60));
        return;
    }
    // Without a pidfd the parent cannot safely fence cleanup of the orphaned
    // fixture, so do not create it under a restrictive kernel policy.
    if !fixture_pidfds_available() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    struct OwnerCleanup(Child);
    impl Drop for OwnerCleanup {
        fn drop(&mut self) {
            if let Some(mut stdin) = self.0.stdin.take() {
                let _ = std::io::Write::write_all(&mut stdin, b"C");
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if self.0.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut helper = OwnerCleanup(
        Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::owner_death_after_spawn_before_registration_leaves_the_intent",
                "--nocapture",
            ])
            .env(MODE, "1")
            .env(DIRECTORY, temp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("fixture owner"),
    );
    let marker = temp.path().join("child.pid");
    let deadline = Instant::now() + Duration::from_secs(10);
    let pid: i32 = loop {
        if let Ok(text) = std::fs::read_to_string(&marker)
            && let Ok(pid) = text.parse()
        {
            break pid;
        }
        assert!(Instant::now() < deadline, "fixture child did not start");
        std::thread::sleep(Duration::from_millis(10));
    };
    let _child = FixtureProcess::pin(pid, helper.0.id())
        .expect("pin the fixture owner's child before owner death");
    std::io::Write::write_all(helper.0.stdin.as_mut().expect("control pipe"), b"G")
        .expect("allow the controlled crash");
    helper.0.kill().expect("stop fixture owner");
    helper.0.wait().expect("reap fixture owner");
    let records = read_ledger_records(&temp.path().join("owned.json")).expect("read evidence");
    assert!(
        records
            .iter()
            .any(|record| matches!(record, Record::LaunchIntent(_))),
        "owner died before registering a started child but left no intent"
    );
}

#[test]
fn web_ledger_retains_legacy_group_without_touching_daemon_files() {
    let temp = tempfile::tempdir().expect("temp");
    let web = temp.path().join("web");
    let daemon = temp.path().join("daemon");
    std::fs::create_dir_all(&web).expect("web directory");
    std::fs::create_dir_all(&daemon).expect("daemon directory");
    let ledger = web.join("owned-process-groups.json");
    let daemon_ledger = daemon.join("owned-process-groups.json");
    let daemon_watch = daemon.join("owned-process-groups.other.watchdog");
    let web_watch = web.join("owned-process-groups.old.watchdog");
    let daemon_group = KillOnDrop(orphan_leader());
    let daemon_entry = LedgerEntry {
        pgid: daemon_group.0.id() as i32,
        spawned_at: now(),
        observed_at: now(),
    };
    std::fs::write(
        &daemon_ledger,
        serde_json::to_vec(&[daemon_entry]).expect("daemon ledger"),
    )
    .expect("persist daemon ledger");
    let daemon_list = format!(
        "{} {}:{}\n",
        now(),
        daemon_entry.pgid,
        daemon_entry.spawned_at
    );
    std::fs::write(&daemon_watch, &daemon_list).expect("daemon watch");
    std::fs::write(&web_watch, format!("{}\n", now())).expect("web watch");
    let mut leader = KillOnDrop(orphan_leader());
    let entry = LedgerEntry {
        pgid: leader.0.id() as i32,
        spawned_at: now(),
        observed_at: now(),
    };
    std::fs::write(&ledger, serde_json::to_vec(&[entry]).expect("ledger")).expect("persist ledger");

    assert!(
        recover_ledger(&ledger).is_err(),
        "legacy live group remains pending"
    );
    assert!(leader.0.try_wait().expect("poll old Web group").is_none());
    assert!(ledger.exists());
    assert!(!web_watch.exists());
    assert!(
        running(daemon_entry.pgid),
        "daemon-owned group must survive"
    );
    let daemon_records = read_ledger_records(&daemon_ledger).expect("daemon ledger survives");
    assert!(
        matches!(daemon_records.as_slice(), [Record::ProcessGroup(group)]
        if group.pgid == daemon_entry.pgid && group.spawned_at == daemon_entry.spawned_at)
    );
    assert_eq!(
        std::fs::read(&daemon_watch).expect("daemon watch survives"),
        daemon_list.as_bytes()
    );
    leader.0.kill().expect("fixture cleanup");
    leader.0.wait().expect("reap fixture");
    recover_ledger(&ledger).expect("retry after group disappears");
    assert!(!ledger.exists());
}

#[test]
fn web_recovery_includes_a_watch_list_newer_than_the_ledger() {
    let temp = tempfile::tempdir().expect("temp");
    let ledger = temp.path().join("owned-process-groups.json");
    let watch = temp.path().join("owned-process-groups.old.watchdog");
    std::fs::write(&ledger, b"[]").expect("old empty ledger");
    let mut leader = KillOnDrop(orphan_leader());
    let pid = leader.0.id() as i32;
    let started = now();
    std::fs::write(&watch, format!("{started} {pid}:{started}\n")).expect("newer watch list");

    assert!(
        recover_ledger(&ledger).is_err(),
        "watch evidence remains pending"
    );
    assert!(leader.0.try_wait().expect("poll fixture").is_none());
    assert!(
        read_ledger_records(&ledger)
            .expect("read evidence")
            .iter()
            .any(|record| { matches!(record, Record::ProcessGroup(group) if group.pgid == pid) })
    );
    assert!(!watch.exists());
    leader.0.kill().expect("fixture cleanup");
    leader.0.wait().expect("reap fixture");
    recover_ledger(&ledger).expect("retry after group disappears");
    assert!(!ledger.exists());
}

#[test]
fn invalid_web_watch_list_blocks_recovery_without_erasing_evidence() {
    let temp = tempfile::tempdir().expect("temp");
    let ledger = temp.path().join("owned-process-groups.json");
    let watch = temp.path().join("owned-process-groups.old.watchdog.tmp");
    std::fs::write(&ledger, b"[]").expect("empty ledger");
    for contents in [String::new(), format!("{} 500:123", now())] {
        std::fs::write(&watch, contents).expect("interrupted write");
        let error = recover_ledger(&ledger).expect_err("unreadable evidence");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            error
                .to_string()
                .contains("owned-process-groups.old.watchdog.tmp")
        );
        assert!(
            !error
                .to_string()
                .contains(&temp.path().display().to_string())
        );
        assert!(ledger.exists());
        assert!(watch.exists());
    }
}

#[test]
fn invalid_web_ledger_is_not_removed_or_treated_as_clean() {
    let temp = tempfile::tempdir().expect("temp");
    let ledger = temp.path().join("owned-process-groups.json");
    let watch = temp.path().join("owned-process-groups.old.watchdog");
    std::fs::write(&ledger, b"not json").expect("invalid ledger");
    std::fs::write(&watch, b"fixture").expect("watch");
    let error = recover_ledger(&ledger).expect_err("invalid ledger");
    assert!(error.to_string().contains("owned-process-groups.json"));
    assert!(
        !error
            .to_string()
            .contains(&temp.path().display().to_string())
    );
    assert!(ledger.exists());
    assert!(watch.exists());
}

#[test]
fn legacy_cleanup_records_survive_restart_format_change() {
    const MODE: &str = "CCCC_GUARD_LEGACY_RESTART_TEST";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::legacy_cleanup_records_survive_restart_format_change",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated restart");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("temp");
    let ledger = temp.path().join("owned.json");
    let mut leader = orphan_leader();
    let pgid = leader.id() as i32;
    std::fs::write(
        &ledger,
        serde_json::to_vec(&serde_json::json!([{
            "pgid": pgid,
            "spawned_at": now(),
            "alive_until": now() + 20
        }]))
        .expect("old record"),
    )
    .expect("persist old record");
    enable(&ledger).expect("restart with old ledger");
    let running = leader.try_wait().expect("poll old leader").is_none();
    let retained = read_ledger_records(&ledger).expect("migrated evidence");
    assert!(retained.iter().any(|record| matches!(
        record,
        Record::ProcessGroup(group) if group.pgid == pgid && group.evidence == GroupEvidence::Legacy
    )));
    assert!(recovery_pending());
    leader.kill().expect("fixture cleanup");
    leader.wait().expect("reap fixture");
    assert!(running, "legacy evidence must not authorize a signal");
    refresh_active(&ledger).expect("absent old group clears");
}

#[test]
fn ledger_migration_removes_only_the_original_future_allowance() {
    let temp = tempfile::tempdir().expect("temp");
    let ledger = temp.path().join("owned.json");
    std::fs::write(
        &ledger,
        serde_json::to_vec(&serde_json::json!([
            {"pgid": 500, "spawned_at": T, "observed_at": T + 600},
            {"pgid": 500, "spawned_at": T, "alive_until": T + 620},
            {"pgid": 501, "spawned_at": 0, "alive_until": 10}
        ]))
        .expect("mixed ledger"),
    )
    .expect("persist fixture");
    let entries: Vec<LedgerEntry> =
        serde_json::from_slice(&std::fs::read(&ledger).expect("ledger"))
            .expect("current and previous formats");
    assert_eq!(entries[0].pgid, 500);
    assert_eq!(entries[0].spawned_at, T);
    assert_eq!(entries[0].observed_at, T + 600);
    assert_eq!(entries[1], entries[0]);
    assert_eq!(entries[2].observed_at, 0);
    let encoded = serde_json::to_value(&entries).expect("current representation");
    assert!(encoded[1].get("alive_until").is_none());
    assert_eq!(encoded[1]["observed_at"], T + 600);
}

#[test]
fn unreadable_cleanup_records_block_replacement() {
    let temp = tempfile::tempdir().expect("temp");
    let ledger = temp.path().join("owned.json");
    let watch_list = temp.path().join("owned.previous.watchdog");
    let original = br#"[{"pgid":500,"spawned_at":1000,"unknown_observation":2000}]"#;
    std::fs::write(&ledger, original).expect("unknown ledger format");
    std::fs::write(&watch_list, "1000 500\n").expect("previous watchdog");
    assert_eq!(
        read_ledger_records(&ledger)
            .expect_err("required observation")
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        enable(&ledger)
            .expect_err("must not replace unreadable state")
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        std::fs::read(&ledger).expect("old ledger retained"),
        original
    );
    assert_eq!(
        std::fs::read_to_string(&watch_list).expect("old watchdog retained"),
        "1000 500\n"
    );
    assert!(
        read_ledger_records(&temp.path().join("missing.json"))
            .expect("first startup has no ledger")
            .is_empty()
    );
}

#[test]
fn externally_reaped_child_is_removed_from_guard_records() {
    const MODE: &str = "CCCC_TEST_GUARD_EXTERNAL_REAP";
    if std::env::var_os(MODE).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::externally_reaped_child_is_removed_from_guard_records",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .expect("isolated helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().expect("temporary ledger");
    let ledger = temp.path().join("owned.json");
    crate::protect_owned_process_groups(&ledger).expect("protect helper");
    let (mut child, owner) =
        crate::OwnedProcessTree::spawn(Command::new("sh").args(["-c", "exit 0"]))
            .expect("fixture child");
    child.wait().expect("external reap");
    let error = owner
        .try_wait(|| child.try_wait())
        .expect_err("ownership lost");
    assert_eq!(error.raw_os_error(), Some(nix::libc::ECHILD));
    let entries = read_ledger_records(&ledger).expect("read owned ledger");
    let listed = guard().as_ref().expect("guard").groups.len();
    publish(&[]).expect("clear fixture records before assertions");
    assert!(
        entries.is_empty(),
        "externally reaped identity remains in durable ledger"
    );
    assert_eq!(
        listed, 0,
        "externally reaped identity remains in watch list"
    );
}

#[test]
fn a_fresh_watch_list_does_not_authorize_a_recycled_group() {
    let temp = tempfile::tempdir().expect("temporary watch list");
    let mut unrelated = orphan_leader();
    let list = temp.path().join("groups.watchdog");
    #[cfg(target_os = "linux")]
    let group = {
        let mut group = captured_test_group(&unrelated, now() - 3_600);
        if let GroupEvidence::Captured(identity) = &mut group.evidence {
            identity.start_ticks += 1;
        }
        group
    };
    #[cfg(not(target_os = "linux"))]
    let group = test_owned_group(unrelated.id() as i32, now() - 3_600);
    write_watch_list(&list, &[group]).expect("fresh list containing an obsolete identity");
    run_watchdog(&list);
    let survived = unrelated.try_wait().expect("poll fixture").is_none();
    if survived {
        unrelated.kill().expect("fixture cleanup");
    }
    unrelated.wait().expect("reap fixture");
    assert!(
        survived,
        "watchdog signalled a process with another birth time"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_watchdog_refuses_unconfirmed_or_foreign_origin_records() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut child = KillOnDrop(orphan_leader());
    let original = captured_test_group(&child.0, now());
    let watch = temp.path().join("owned.watchdog");

    let mut cases = Vec::new();
    let mut unconfirmed = original.clone();
    unconfirmed.evidence = GroupEvidence::Unconfirmed;
    cases.push(unconfirmed);
    for which in 0..3 {
        let mut group = original.clone();
        let GroupEvidence::Captured(identity) = &mut group.evidence else {
            panic!("fixture identity");
        };
        match which {
            0 => identity.origin.boot_id = uuid::Uuid::new_v4().to_string(),
            1 => identity.origin.pid_namespace += 1,
            _ => identity.origin.time_namespace = Some(u64::MAX),
        }
        group.origin = Some(identity.origin.clone());
        cases.push(group);
    }
    for group in cases {
        write_watch_list(&watch, &[group]).expect("write watch");
        assert!(run_watchdog(&watch).success());
        assert!(
            child.0.try_wait().expect("poll fixture").is_none(),
            "an unconfirmed or foreign record signalled the fixture"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_watch_evidence_roundtrips_the_recorded_strength() {
    let temp = tempfile::tempdir().expect("tempdir");
    let child = KillOnDrop(orphan_leader());
    let group = captured_test_group(&child.0, now());
    let ledger = temp.path().join("owned.json");
    let watch = temp.path().join("owned.fixture.watchdog");
    let observed_at = now();
    write_watch_list_at(&watch, std::slice::from_ref(&group), observed_at).expect("write");
    let (records, paths) = read_recovery_watch_lists(&ledger).expect("read");
    assert_eq!(paths, [watch]);
    let [Record::ProcessGroup(record)] = records.as_slice() else {
        panic!("expected one versioned group record");
    };
    assert_eq!(record.id, group.id);
    assert_eq!(record.pgid, group.pgid);
    assert_eq!(record.origin, group.origin);
    assert_eq!(record.evidence, group.evidence);
}

#[cfg(target_os = "linux")]
fn record_from_group(group: &OwnedGroup) -> Record {
    Record::ProcessGroup(GroupRecord {
        id: group.id,
        source_id: None,
        pgid: group.pgid,
        spawned_at: group.spawned_at,
        observed_at: now(),
        origin: group.origin.clone(),
        evidence: group.evidence.clone(),
    })
}

#[cfg(target_os = "linux")]
#[test]
fn recovery_retains_foreign_and_conflicting_evidence_without_signalling() {
    let mut child = KillOnDrop(orphan_leader());
    let group = captured_test_group(&child.0, now());
    let mut foreign = record_from_group(&group);
    let Record::ProcessGroup(foreign_group) = &mut foreign else {
        unreachable!()
    };
    let GroupEvidence::Captured(identity) = &mut foreign_group.evidence else {
        unreachable!()
    };
    identity.origin.pid_namespace += 1;
    foreign_group.origin = Some(identity.origin.clone());

    let mut conflict = record_from_group(&group);
    let Record::ProcessGroup(conflict_group) = &mut conflict else {
        unreachable!()
    };
    let GroupEvidence::Captured(identity) = &mut conflict_group.evidence else {
        unreachable!()
    };
    identity.start_ticks += 1;
    let records = vec![foreign, conflict];
    assert_eq!(reconcile_records(records.clone()), records);
    assert!(child.0.try_wait().expect("fixture still running").is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn migrated_leader_helper() {
    let Some(directory) = std::env::var_os("CCCC_TEST_MIGRATING_LEADER") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !directory.join("go").exists() {
        assert!(Instant::now() < deadline, "parent did not release fixture");
        std::thread::sleep(Duration::from_millis(5));
    }
    let parent_group: i32 = std::env::var("CCCC_TEST_PARENT_GROUP")
        .expect("parent group")
        .parse()
        .expect("numeric group");
    rustix::process::setpgid(None, rustix::process::Pid::from_raw(parent_group))
        .expect("move fixture to parent's group");
    std::fs::write(directory.join("moved"), b"ready").expect("notify parent");
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn empty_original_group_does_not_clear_a_live_leader_that_moved() {
    let temp = tempfile::tempdir().expect("fixture directory");
    let mut child = KillOnDrop(
        Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_tree::guard::tests::migrated_leader_helper",
                "--nocapture",
            ])
            .env("CCCC_TEST_MIGRATING_LEADER", temp.path())
            .env(
                "CCCC_TEST_PARENT_GROUP",
                rustix::process::getpgrp()
                    .as_raw_nonzero()
                    .get()
                    .to_string(),
            )
            .process_group(0)
            .spawn()
            .expect("spawn fixture"),
    );
    let group = captured_test_group(&child.0, now());
    std::fs::write(temp.path().join("go"), b"go").expect("release fixture");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !temp.path().join("moved").exists() {
        assert!(
            child.0.try_wait().expect("poll fixture").is_none(),
            "fixture exited before moving"
        );
        assert!(Instant::now() < deadline, "fixture did not move");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(group_absent(group.pgid), "original group must be empty");
    let record = record_from_group(&group);
    assert_eq!(reconcile_records(vec![record.clone()]), vec![record]);
    assert!(
        child.0.try_wait().expect("poll fixture").is_none(),
        "migrated leader must not be signalled"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn recovery_signals_the_verified_fixture_and_confirms_group_absence() {
    let mut child = KillOnDrop(orphan_leader());
    let group = captured_test_group(&child.0, now());
    let mut retained = reconcile_records(vec![record_from_group(&group)]);
    let deadline = Instant::now() + Duration::from_secs(3);
    let stopped_by_recovery = loop {
        if child.0.try_wait().expect("poll fixture").is_some() {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(stopped_by_recovery, "verified fixture was not signalled");
    if !retained.is_empty() {
        retained = reconcile_records(retained);
    }
    assert!(
        retained.is_empty(),
        "verified group should be gone: {retained:?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn verified_previous_boot_clears_without_signalling_current_fixture() {
    let mut child = KillOnDrop(orphan_leader());
    let group = captured_test_group(&child.0, now());
    let mut record = record_from_group(&group);
    let Record::ProcessGroup(group) = &mut record else {
        unreachable!()
    };
    let GroupEvidence::Captured(identity) = &mut group.evidence else {
        unreachable!()
    };
    identity.origin.boot_id = uuid::Uuid::new_v4().to_string();
    group.origin = Some(identity.origin.clone());
    assert!(reconcile_records(vec![record]).is_empty());
    assert!(child.0.try_wait().expect("fixture still running").is_none());
}

fn orphan_leader() -> Child {
    Command::new("sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .expect("spawn group leader")
}

/// A self-created helper's child, pinned before the helper can exit. Cleanup
/// through the fd cannot signal an unrelated process after numeric PID reuse.
struct FixtureProcess {
    #[cfg(target_os = "linux")]
    fd: std::os::fd::OwnedFd,
}

impl FixtureProcess {
    fn pin(pid: i32, parent: u32) -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            use rustix::process::{Pid, PidfdFlags, pidfd_open};
            let pid_value =
                Pid::from_raw(pid).ok_or_else(|| io::Error::other("invalid fixture PID"))?;
            let fd = pidfd_open(pid_value, PidfdFlags::empty())?;
            let view = super::super::linux_identity::ProcView::open()?;
            if view.pidfd_process(&fd)? != pid
                || view.parent_pid(pid)? != parent as i32
                || view.pidfd_process(&fd)? != pid
            {
                return Err(io::Error::other("fixture is not the helper's child"));
            }
            super::super::linux_identity::require_running_pidfd(&fd)?;
            Ok(Self { fd })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (pid, parent);
            Ok(Self {})
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for FixtureProcess {
    fn drop(&mut self) {
        let _ = rustix::process::pidfd_send_signal(&self.fd, rustix::process::Signal::KILL);
    }
}

fn fixture_pidfds_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        use rustix::process::{Pid, PidfdFlags, pidfd_open};
        super::super::linux_identity::ProcView::open().is_ok()
            && Pid::from_raw(std::process::id() as i32)
                .is_some_and(|pid| pidfd_open(pid, PidfdFlags::empty()).is_ok())
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// An orphan's new parent may reap it late; a zombie has still exited.
fn exited_within(pid: i32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !running(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

const STALL_MODE: &str = "CCCC_GUARD_STALL_TEST";
const STALL_DIR: &str = "CCCC_GUARD_STALL_DIR";

#[test]
fn a_stalled_watchdog_never_blocks_publication_or_forced_exit() {
    if !fixture_pidfds_available() {
        eprintln!("skipped: cannot pin fixture watchdog on this kernel");
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let mut helper = KillOnDrop(
        Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "process_tree::guard::tests::stalled_watchdog_helper",
                "--exact",
                "--nocapture",
            ])
            .env(STALL_MODE, "1")
            .env(STALL_DIR, temp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn owner helper"),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let watchdog = loop {
        if let Ok(text) = std::fs::read_to_string(temp.path().join("watchdog.pid"))
            && text.ends_with('\n')
            && let Ok(pid) = text.trim().parse::<i32>()
        {
            break pid;
        }
        assert!(helper.0.try_wait().expect("poll helper").is_none());
        assert!(
            Instant::now() < deadline,
            "helper did not identify watchdog"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let _watchdog = FixtureProcess::pin(watchdog, helper.0.id()).expect("pin fixture watchdog");
    helper
        .0
        .stdin
        .as_mut()
        .expect("helper stdin")
        .write_all(b"P")
        .expect("release helper");
    drop(helper.0.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = helper.0.try_wait().expect("poll helper") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    drop(helper);
    assert!(
        status.is_some_and(|status| status.success()),
        "publication or forced exit blocked on a stalled watchdog"
    );
    assert!(
        temp.path().join("done").exists(),
        "owner helper did not finish"
    );
    assert!(
        exited_within(watchdog, Duration::from_secs(5)),
        "fixture watchdog did not exit"
    );
}

#[test]
fn stalled_watchdog_helper() {
    if std::env::var_os(STALL_MODE).is_none() {
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var_os(STALL_DIR).expect("stall dir"));
    crate::protect_owned_process_groups(&dir.join("ledger.json")).expect("protect");
    let output = Command::new("pgrep")
        .args(["-P", &std::process::id().to_string(), "-x", "sh"])
        .output()
        .expect("pgrep");
    let watchdog = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    std::fs::write(dir.join("watchdog.pid"), format!("{watchdog}\n")).expect("publish watchdog");
    let mut proceed = [0_u8; 1];
    if std::io::stdin().read_exact(&mut proceed).is_err() || proceed != *b"P" {
        return;
    }
    assert!(
        Command::new("kill")
            .args(["-STOP", &watchdog])
            .status()
            .expect("stop watchdog")
            .success()
    );
    // Far more than a pipe buffer; these PGIDs exceed every pid_max. Publication
    // must not depend on the watchdog reading anything.
    let groups = (0..20_000)
        .map(|index| test_owned_group(900_000_000 + index, now()))
        .collect::<Vec<_>>();
    for _ in 0..20 {
        publish(&groups).expect("publish");
    }
    publish(&[]).expect("publish");
    crate::force_terminate_owned().expect("force terminate");
    std::fs::write(dir.join("done"), b"done").expect("publish done");
    // Resume while it is still our unreaped child. Once this helper exits,
    // its pipe closes and the watchdog can finish without a numeric-PID kill.
    let pid = rustix::process::Pid::from_raw(watchdog.parse().expect("watchdog pid"))
        .expect("positive watchdog pid");
    rustix::process::waitid(
        rustix::process::WaitId::Pid(pid),
        rustix::process::WaitIdOptions::EXITED
            | rustix::process::WaitIdOptions::NOHANG
            | rustix::process::WaitIdOptions::NOWAIT,
    )
    .expect("watchdog remains our child");
    rustix::process::kill_process(pid, rustix::process::Signal::CONT)
        .expect("resume owned watchdog");
}

fn run_watchdog(watch_list: &Path) -> std::process::ExitStatus {
    watchdog_command(watch_list)
        .stdin(Stdio::null())
        .status()
        .expect("run watchdog")
}

#[test]
fn the_watchdog_terminates_groups_in_a_fresh_watch_list() {
    let temp = tempfile::tempdir().expect("tempdir");
    let list = temp.path().join("groups.watchdog");
    let mut leader = orphan_leader();
    #[cfg(target_os = "linux")]
    let group = captured_test_group(&leader, now());
    #[cfg(not(target_os = "linux"))]
    let group = test_owned_group(leader.id() as i32, now());
    write_watch_list(&list, &[group]).expect("watch list");
    assert!(run_watchdog(&list).success());
    // The leader is this test's child: poll its exit rather than signal 0,
    // which a zombie still answers.
    let deadline = Instant::now() + Duration::from_secs(5);
    while leader.try_wait().expect("poll leader").is_none() {
        assert!(
            Instant::now() < deadline,
            "the watchdog left a listed group running"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_watchdog_leaves_a_stale_watch_list_to_verified_reconciliation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let list = temp.path().join("groups.watchdog");
    let mut unrelated = orphan_leader();
    let written_long_ago = now() - WATCH_LIST_MAX_AGE.as_secs() - 60;
    std::fs::write(&list, format!("{written_long_ago} {}\n", unrelated.id())).expect("list");
    run_watchdog(&list);
    assert!(
        unrelated.try_wait().expect("poll").is_none(),
        "a stale watch list must not be trusted"
    );
    unrelated.kill().expect("kill unrelated");
    unrelated.wait().expect("reap unrelated");
}

const LATEST_MODE: &str = "CCCC_GUARD_LATEST_TEST";

#[test]
fn a_group_published_while_the_watchdog_is_paused_still_ends_with_its_owner() {
    if !fixture_pidfds_available() {
        eprintln!("skipped: cannot pin fixture processes on this kernel");
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let mut owner = KillOnDrop(
        Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "process_tree::guard::tests::latest_list_owner_helper",
                "--exact",
                "--nocapture",
            ])
            .env(LATEST_MODE, "1")
            .env(STALL_DIR, temp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn owner helper"),
    );
    let pids_path = temp.path().join("pids");
    let deadline = Instant::now() + Duration::from_secs(10);
    let pids = loop {
        if let Ok(text) = std::fs::read_to_string(&pids_path)
            && text.ends_with('\n')
        {
            break text
                .split_whitespace()
                .map(|pid| pid.parse::<i32>().expect("pid"))
                .collect::<Vec<_>>();
        }
        assert!(Instant::now() < deadline, "owner helper did not publish");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(pids.len(), 3, "helper must report its own children");
    let (first, latest, watchdog) = (pids[0], pids[1], pids[2]);
    let _first = FixtureProcess::pin(first, owner.0.id()).expect("pin first fixture");
    let _latest = FixtureProcess::pin(latest, owner.0.id()).expect("pin second fixture");
    let _watchdog = FixtureProcess::pin(watchdog, owner.0.id()).expect("pin fixture watchdog");
    assert!(
        running(first) && running(latest),
        "fixture groups exited before the owner"
    );

    owner.0.kill().expect("kill owner");
    owner.0.wait().expect("reap owner");

    // The watchdog must act now, without a later owner start.
    let first_ended = exited_within(first, Duration::from_secs(10));
    let latest_ended = exited_within(latest, Duration::from_secs(10));
    // On platforms without pidfds, these self-created sleeps are bounded and
    // expire naturally on failure; do not signal a possibly recycled number.
    #[cfg(not(target_os = "linux"))]
    for (pid, ended) in [(first, first_ended), (latest, latest_ended)] {
        if !ended {
            let _ = exited_within(pid, Duration::from_secs(35));
        }
    }
    assert!(first_ended, "an earlier group survived its owner");
    assert!(
        latest_ended,
        "the group published while the watchdog was paused survived"
    );
}

#[test]
fn latest_list_owner_helper() {
    if std::env::var_os(LATEST_MODE).is_none() {
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var_os(STALL_DIR).expect("dir"));
    crate::protect_owned_process_groups(&dir.join("ledger.json")).expect("protect");
    let spawn = || {
        crate::OwnedProcessTree::spawn(
            Command::new("sleep")
                .arg("30")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .expect("spawn owned group")
    };
    let (first, _first_tree) = spawn();
    let output = Command::new("pgrep")
        .args(["-P", &std::process::id().to_string(), "-x", "sh"])
        .output()
        .expect("pgrep");
    let watchdog = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    assert!(
        Command::new("kill")
            .args(["-STOP", &watchdog])
            .status()
            .expect("stop watchdog")
            .success()
    );
    let (latest, _latest_tree) = spawn();
    std::fs::write(
        dir.join("pids"),
        format!("{} {} {watchdog}\n", first.id(), latest.id()),
    )
    .expect("publish pids");
    std::thread::sleep(Duration::from_secs(300));
}

const INSTANCE_MODE: &str = "CCCC_GUARD_INSTANCE_TEST";
const INSTANCE_OUT: &str = "CCCC_GUARD_INSTANCE_OUT";

#[test]
fn a_late_watchdog_of_a_replaced_instance_never_touches_the_new_instance() {
    if !fixture_pidfds_available() {
        eprintln!("skipped: cannot pin fixture processes on this kernel");
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let mut first = KillOnDrop(spawn_instance(temp.path(), "first"));
    let (first_group, first_list) = instance_output(temp.path(), "first");
    let _first_group = FixtureProcess::pin(first_group, first.0.id()).expect("pin first fixture");
    assert!(
        running(first_group),
        "fixture group exited before the owner"
    );
    first.0.kill().expect("kill first instance");
    first.0.wait().expect("reap first instance");
    assert!(
        ended(first_group),
        "the first instance's watchdog left its group"
    );

    let second = KillOnDrop(spawn_instance(temp.path(), "second"));
    let (second_group, second_list) = instance_output(temp.path(), "second");
    let _second_group =
        FixtureProcess::pin(second_group, second.0.id()).expect("pin second fixture");
    assert_ne!(first_list, second_list);
    assert!(
        !first_list.exists(),
        "the replaced instance's list must be removed"
    );

    // The first instance's watchdog reads its list only now.
    run_watchdog(&first_list);

    let survived = running(second_group);
    drop(second);
    // Its own watchdog reads the list before the directory goes away.
    let second_ended = ended(second_group);
    assert!(
        survived,
        "a late watchdog terminated the new instance's group"
    );
    assert!(
        second_ended,
        "the second instance's watchdog left its group"
    );
}

fn ended(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while running(pid) {
        if Instant::now() >= deadline {
            // The fixture sleep has a finite lifetime. On non-Linux hosts,
            // let it finish rather than signalling a potentially reused PID.
            #[cfg(not(target_os = "linux"))]
            let _ = exited_within(pid, Duration::from_secs(35));
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

#[test]
fn instance_helper() {
    let Some(name) = std::env::var_os(INSTANCE_MODE) else {
        return;
    };
    let dir = std::path::PathBuf::from(std::env::var_os(INSTANCE_OUT).expect("dir"));
    crate::protect_owned_process_groups(&dir.join("owned.json")).expect("protect");
    let (group, _tree) = crate::OwnedProcessTree::spawn(
        Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .expect("spawn owned group");
    let list = std::fs::read_dir(&dir)
        .expect("dir")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "watchdog"))
        .expect("own watch list");
    std::fs::write(
        dir.join(name).with_extension("out"),
        format!("{} {}\n", group.id(), list.display()),
    )
    .expect("publish instance");
    std::thread::sleep(Duration::from_secs(300));
}

fn spawn_instance(dir: &Path, name: &str) -> Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "process_tree::guard::tests::instance_helper",
            "--exact",
            "--nocapture",
        ])
        .env(INSTANCE_MODE, name)
        .env(INSTANCE_OUT, dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn instance helper")
}

fn instance_output(dir: &Path, name: &str) -> (i32, std::path::PathBuf) {
    let path = dir.join(name).with_extension("out");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Some((group, list)) = text.trim_end().split_once(' ')
            && text.ends_with('\n')
        {
            return (group.parse().expect("group pid"), list.into());
        }
        assert!(Instant::now() < deadline, "{name} instance did not publish");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A terminated child of a still-running instance stays a zombie; only a process
/// that can still run counts.
fn running(pid: i32) -> bool {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    let state = String::from_utf8_lossy(&output.stdout);
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}

/// Stops an instance helper even when an assertion fails first.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
