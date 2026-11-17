use super::*;

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    intent: Intent,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture directory");
        let home = HomeLayout::from_path(temp.path().join("home 空格")).expect("home");
        home.initialize().expect("initialize home");
        let id = uuid::Uuid::new_v4().simple().to_string();
        fs::create_dir_all(home.root().join("update").join(&id)).expect("transaction directory");
        let install = temp.path().join("install 空格");
        fs::create_dir(&install).expect("install directory");
        let intent = Intent::new(
            &home,
            &id,
            BuildIdentity {
                version: "0.4.59".into(),
                source_id: "1".repeat(64),
            },
            install,
        )
        .expect("intent");
        Self {
            _temp: temp,
            home,
            intent,
        }
    }

    fn arm(&self) {
        arm(&self.home, &self.intent).expect("arm update");
    }
}

#[test]
fn a_normal_start_with_no_intent_creates_no_update_state() {
    let temp = tempfile::tempdir().expect("fixture directory");
    let home = HomeLayout::from_path(temp.path().join("not-created")).expect("home");
    assert_eq!(active(&home).expect("read missing intent"), None);
    check_start(&home, None, &BuildIdentity::current()).expect("ordinary start");
    assert!(!home.root().exists());
    assert!(check_start(&home, Some(temp.path()), &BuildIdentity::current()).is_err());
    assert!(!home.root().exists());
}

#[test]
fn only_the_selected_candidate_with_the_explicit_transaction_may_start() {
    let f = Fixture::new();
    f.arm();
    arm(&f.home, &f.intent).expect("idempotent same intent");
    let error = check_start(&f.home, None, &f.intent.target).expect_err("ordinary start refused");
    assert!(error.to_string().contains("update_in_progress"));
    assert!(error.to_string().contains("update --recover"));
    check_start(&f.home, Some(&f.intent.transaction_dir), &f.intent.target)
        .expect("matching candidate");

    for actual in [
        BuildIdentity {
            version: "0.4.58".into(),
            ..f.intent.target.clone()
        },
        BuildIdentity {
            source_id: "2".repeat(64),
            ..f.intent.target.clone()
        },
        BuildIdentity {
            source_id: String::new(),
            ..f.intent.target.clone()
        },
    ] {
        assert!(check_start(&f.home, Some(&f.intent.transaction_dir), &actual).is_err());
    }
    let other = Fixture::new();
    assert!(
        check_start(
            &f.home,
            Some(&other.intent.transaction_dir),
            &f.intent.target
        )
        .is_err()
    );
    assert!(disarm(&f.home, &other.intent.transaction_id).is_err());
    assert_eq!(active(&f.home).expect("original intent"), Some(f.intent));
}

#[test]
fn an_unresponsive_daemon_still_holding_its_lock_prevents_file_changes() {
    let f = Fixture::new();
    f.arm();
    let lock_path = f.home.daemon_dir().join("ccccd.lock");
    let daemon = crate::fs::try_exclusive_lock(&lock_path)
        .expect("lock")
        .expect("daemon acquired lock");
    assert!(!f.home.daemon_dir().join("ccccd.addr.json").exists());
    let error = claim_stopped(&f.home, &f.intent.transaction_id).expect_err("lock is busy");
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert!(error.to_string().contains("daemon_running"));
    drop(daemon);

    let installer = claim_stopped(&f.home, &f.intent.transaction_id).expect("hold stopped home");
    assert!(
        crate::fs::try_exclusive_lock(&lock_path)
            .expect("probe held lock")
            .is_none()
    );
    // Closing the descriptor for exec must not lift the persistent startup fence.
    drop(installer);
    assert!(check_start(&f.home, None, &f.intent.target).is_err());
    check_start(&f.home, Some(&f.intent.transaction_dir), &f.intent.target)
        .expect("explicit handoff after descriptor closes");
    disarm(&f.home, &f.intent.transaction_id).expect("confirmed update");
    check_start(&f.home, None, &BuildIdentity::current()).expect("normal starts restored");
    assert!(check_start(&f.home, Some(&f.intent.transaction_dir), &f.intent.target).is_err());
}

#[test]
fn a_dead_hosts_intent_is_not_evicted_by_a_new_update_or_normal_start() {
    let f = Fixture::new();
    f.arm();
    let mut dead = f.intent.clone();
    // The same PID with a different native creation identity is an old holder,
    // not permission to silently discard its incomplete file transaction.
    dead.holder.started = "a-previous-process".into();
    crate::fs::write_json(&active_path(&f.home), &dead).expect("old holder record");
    assert!(check_start(&f.home, None, &dead.target).is_err());
    assert!(arm(&f.home, &f.intent).is_err());
    let held = claim_stopped(&f.home, &dead.transaction_id).expect("explicit recovery permitted");
    disarm(&f.home, &dead.transaction_id).expect("old files restored");
    drop(held);
    assert_eq!(active(&f.home).expect("cleared"), None);
}

#[test]
fn a_different_live_host_cannot_take_over_or_disarm_the_update() {
    let f = Fixture::new();
    f.arm();
    let mut foreign = f.intent.clone();
    foreign.holder = NativeHost::parent(std::process::id()).expect("live fixture parent");
    crate::fs::write_json(&active_path(&f.home), &foreign).expect("foreign live holder");
    assert!(claim_stopped(&f.home, &foreign.transaction_id).is_err());
    assert!(disarm(&f.home, &foreign.transaction_id).is_err());
    assert_eq!(active(&f.home).expect("still active"), Some(foreign));
}

#[test]
fn concurrent_updates_cannot_overwrite_the_first_intent() {
    let f = Fixture::new();
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    fs::create_dir(f.home.root().join("update").join(&id)).expect("second transaction");
    let second = Intent::new(
        &f.home,
        &id,
        f.intent.target.clone(),
        f.intent.install_dir.clone(),
    )
    .expect("second intent");
    let home = f.home.clone();
    let first = f.intent.clone();
    let a = std::thread::spawn(move || arm(&home, &first));
    let home = f.home.clone();
    let b = std::thread::spawn(move || arm(&home, &second));
    assert_ne!(
        a.join().expect("first updater").is_ok(),
        b.join().expect("second updater").is_ok()
    );
    let winner = active(&f.home).expect("read winner").expect("one intent");
    assert!([f.intent.transaction_id, id].contains(&winner.transaction_id));
}

#[test]
fn damaged_or_oversized_records_do_not_lift_the_startup_fence() {
    let f = Fixture::new();
    f.arm();
    for content in [
        b"{\"schema\":1}".to_vec(),
        vec![b' '; MAX_RECORD_BYTES as usize + 1],
    ] {
        fs::write(active_path(&f.home), &content).expect("damaged record");
        let error = active(&f.home).expect_err("damaged intent must remain fenced");
        assert!(error.to_string().contains("update_in_progress"));
        assert!(error.to_string().contains("cannot read or validate"));
        assert!(
            error
                .to_string()
                .contains(&active_path(&f.home).display().to_string())
        );
        assert!(
            error
                .to_string()
                .contains("after checking the installation")
        );
        assert!(error.to_string().contains("manually to allow startup"));
        assert!(check_start(&f.home, None, &f.intent.target).is_err());
        assert!(disarm(&f.home, &f.intent.transaction_id).is_err());
        assert_eq!(
            fs::read(active_path(&f.home)).expect("intent kept"),
            content
        );
        assert!(!f.home.daemon_dir().join("ccccd.lock").exists());
    }
}

#[test]
fn missing_update_directories_keep_the_fence_and_name_the_manual_recovery_path() {
    for (missing_transaction, expected_kind) in [
        (true, io::ErrorKind::NotFound),
        (false, io::ErrorKind::InvalidData),
    ] {
        let f = Fixture::new();
        f.arm();
        let original = fs::read(active_path(&f.home)).expect("published intent");
        fs::remove_dir(if missing_transaction {
            &f.intent.transaction_dir
        } else {
            &f.intent.install_dir
        })
        .expect("simulate removed directory");

        let error = check_start(&f.home, None, &f.intent.target)
            .expect_err("missing directory cannot lift the fence");
        assert_eq!(error.kind(), expected_kind);
        let message = error.to_string();
        assert!(message.contains("update_in_progress"));
        assert!(message.contains(&active_path(&f.home).display().to_string()));
        assert!(
            message.contains("checking the installation and the interrupted update transaction")
        );
        assert!(message.contains("manually to allow startup"));
        assert!(claim_stopped(&f.home, &f.intent.transaction_id).is_err());
        assert!(disarm(&f.home, &f.intent.transaction_id).is_err());
        assert_eq!(
            fs::read(active_path(&f.home)).expect("intent kept"),
            original
        );
        assert!(!f.home.daemon_dir().join("ccccd.lock").exists());
    }
}

#[test]
fn invalid_transaction_ids_and_targets_are_rejected_before_publication() {
    let f = Fixture::new();
    for id in [
        "..",
        "../escape",
        "",
        "00000000000000000000000000000000",
        "not-a-uuid",
    ] {
        assert!(
            Intent::new(
                &f.home,
                id,
                f.intent.target.clone(),
                f.intent.install_dir.clone()
            )
            .is_err()
        );
    }
    let mut broken = f.intent.clone();
    broken.target.source_id = "not-a-source-id".into();
    assert!(arm(&f.home, &broken).is_err());
    assert_eq!(active(&f.home).expect("not armed"), None);
}

#[cfg(unix)]
#[test]
fn transaction_symlinks_cannot_redirect_the_home_binding() {
    let f = Fixture::new();
    let other = tempfile::tempdir().expect("different transaction directory");
    fs::remove_dir(&f.intent.transaction_dir).expect("remove empty transaction");
    std::os::unix::fs::symlink(other.path(), &f.intent.transaction_dir)
        .expect("transaction symlink");
    assert!(arm(&f.home, &f.intent).is_err());
}

#[cfg(unix)]
#[test]
fn recovery_commands_quote_paths_and_select_the_existing_old_executable() {
    let f = Fixture::new();
    let parts = shell_words::split(&f.intent.recovery_command()).expect("parse command");
    assert_eq!(
        parts[0],
        f.intent.install_dir.join("cccc").to_string_lossy()
    );
    assert_eq!(&parts[1..3], &["update", "--recover"]);
    assert_eq!(parts[3], f.intent.transaction_dir.to_string_lossy());
    let backup = f
        .intent
        .install_dir
        .join(format!(".cccc-backup.{}", f.intent.transaction_id));
    fs::create_dir(&backup).expect("installer backup");
    fs::write(backup.join("cccc"), b"old executable fixture").expect("old executable");
    assert_eq!(
        shell_words::split(&f.intent.recovery_command()).expect("parse backup command")[0],
        backup.join("cccc").to_string_lossy()
    );
}
