use super::*;

fn run(stdout: &str, stderr: &str) -> PhaseRun {
    PhaseRun {
        success: true,
        stdout: stdout.into(),
        stderr: stderr.into(),
    }
}

#[test]
fn installer_results_come_only_from_their_own_lines() {
    let output = run(
        "Downloading CCCC v1.2.3...\nCCCC_INSTALL_STAGED=/old\nnoise CCCC_INSTALL_STAGED=/x\nCCCC_INSTALL_STAGED=/opt/cccc.cccc-install.abc\n",
        "",
    );
    assert_eq!(
        output.value("CCCC_INSTALL_STAGED"),
        Some("/opt/cccc.cccc-install.abc")
    );
    assert_eq!(output.value("CCCC_INSTALL_APPLIED"), None);
    assert_eq!(
        run("CCCC_INSTALL_APPLIED=\n", "").value("CCCC_INSTALL_APPLIED"),
        None
    );
    // A key that merely starts with another is a different key.
    assert_eq!(
        run("CCCC_INSTALL_STAGED_EXTRA=1\n", "").value("CCCC_INSTALL_STAGED"),
        None
    );
}

#[test]
fn installer_failures_report_the_last_reason() {
    let output = run(
        "",
        "CCCC installer: first\nCCCC installer: the CCCC daemon is still running\n\n",
    );
    assert_eq!(output.failure_summary(), "the CCCC daemon is still running");
    assert_eq!(
        run("", "").failure_summary(),
        "the installer gave no reason"
    );
}

#[test]
fn only_release_versions_are_accepted() {
    assert_eq!(
        release_version(" 0.4.59 ").expect("trimmed release"),
        "0.4.59"
    );
    assert_eq!(
        release_version("0.5.0-rc.1").expect("prerelease"),
        "0.5.0-rc.1"
    );
    for invalid in [
        "",
        "v0.4.59",
        "0.4",
        "latest",
        "0.4.59/../x",
        &"9".repeat(65),
    ] {
        let rejected = release_version(invalid).expect_err(invalid);
        assert_eq!(rejected.code, "invalid_version");
    }
}

#[test]
fn launch_specs_replay_the_explicit_web_mode() {
    let spec = LaunchSpec::new(
        Some("0.0.0.0".into()),
        Some(9000),
        Some(cccc_web::WebMode::Normal),
        false,
    );
    assert_eq!(
        spec.web_mode().expect("recorded mode"),
        Some(cccc_web::WebMode::Normal)
    );
    assert_eq!(LaunchSpec::default().web_mode().expect("no mode"), None);
    let unknown = LaunchSpec {
        web_mode: Some("kiosk".into()),
        ..LaunchSpec::default()
    };
    assert!(unknown.web_mode().is_err());
}

#[test]
fn self_check_reports_this_programs_constants() {
    let check = SelfCheck {
        schema: SELF_CHECK_SCHEMA,
        version: crate::PRODUCT_VERSION.into(),
        build: SelfCheckBuild {
            source_id: "a".repeat(64),
        },
        daemon_compatibility: cccc_contracts::RUST_DAEMON_COMPATIBILITY.into(),
        relay_schemas: vec![RELAY_SCHEMA],
    };
    let value = serde_json::to_value(&check).expect("serialize check");
    assert_eq!(value["schema"], 1);
    assert_eq!(value["relay_schemas"], serde_json::json!([RELAY_SCHEMA]));
    // A newer candidate may add fields; this build still reads what it needs.
    let mut newer = value.clone();
    newer["owner_contract"] = serde_json::json!(2);
    let parsed: SelfCheck = serde_json::from_value(newer).expect("newer check");
    assert_eq!(parsed.version, crate::PRODUCT_VERSION);
}

fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("temp dir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    std::fs::create_dir_all(home.root()).expect("home dir");
    (temp, home)
}

fn record(id: &str) -> RelayRecord {
    RelayRecord {
        schema: RELAY_SCHEMA,
        transaction: id.into(),
        from_version: "0.4.58".into(),
        to_version: "0.4.59".into(),
        mode: "restart".into(),
        install_dir: PathBuf::from("/opt/cccc"),
        launch: LaunchSpec::default(),
        outcome: RelayOutcome::Abandoned {
            stage: "stop".into(),
            reason: "two agents could not be confirmed stopped".into(),
        },
    }
}

#[test]
fn relay_records_round_trip_and_refuse_another_shape() {
    let (_temp, home) = home();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let transaction = Transaction::create(&home, &id, home.root()).expect("transaction");
    cccc_core::fs::write_secret_json(&transaction.relay_path(), &record(&id)).expect("record");
    let read = transaction.read_relay().expect("read record");
    assert_eq!(read.outcome, record(&id).outcome);

    let mut extra = serde_json::to_value(record(&id)).expect("serialize record");
    extra["unexpected"] = serde_json::json!(true);
    cccc_core::fs::write_secret_json(&transaction.relay_path(), &extra).expect("extra record");
    assert!(transaction.read_relay().is_err());

    let mut newer = record(&id);
    newer.schema = RELAY_SCHEMA + 1;
    cccc_core::fs::write_secret_json(&transaction.relay_path(), &newer).expect("newer record");
    assert!(transaction.read_relay().is_err());

    let other = record(&uuid::Uuid::new_v4().simple().to_string());
    cccc_core::fs::write_secret_json(&transaction.relay_path(), &other).expect("other record");
    assert!(transaction.read_relay().is_err());
}

#[test]
fn transactions_open_only_under_their_own_home() {
    let (temp, home) = home();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let created = Transaction::create(&home, &id, home.root()).expect("transaction");
    let opened = Transaction::open(&home, &created.dir).expect("open own transaction");
    assert_eq!(opened.id, id);

    let other_home = HomeLayout::from_path(temp.path().join("other")).expect("other home");
    std::fs::create_dir_all(other_home.root().join("update")).expect("other update dir");
    assert!(Transaction::open(&other_home, &created.dir).is_err());

    let not_an_id = home.root().join("update/not-a-transaction");
    std::fs::create_dir_all(&not_an_id).expect("odd dir");
    assert!(Transaction::open(&home, &not_an_id).is_err());
}

#[cfg(unix)]
#[test]
fn transaction_directories_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let (_temp, home) = home();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let transaction = Transaction::create(&home, &id, home.root()).expect("transaction");
    for path in [home.root().join("update"), transaction.dir.clone()] {
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "{}", path.display());
    }
}

fn coordinator(home: &HomeLayout) -> Coordinator {
    Coordinator::new(UpdateContext {
        home: home.clone(),
        install_dir: home.root().join("bin"),
        executable: home.root().join("bin/cccc"),
        launch: LaunchSpec::default(),
    })
}

#[test]
fn keeping_agents_is_refused_until_its_evidence_exists() {
    let (_temp, home) = home();
    let rejected = cccc_web::UpdateHost::begin(
        &coordinator(&home),
        UpdateRequest {
            version: "0.4.59".into(),
            mode: UpdateMode::Keep,
        },
    )
    .err()
    .expect("keep is refused");
    assert_eq!(rejected.code, "keep_preflight_incomplete");
}

#[test]
fn one_update_at_a_time_and_only_its_owner_abandons_it() {
    let (_temp, home) = home();
    let coordinator = coordinator(&home);
    *coordinator.inner.lock() = Slot::Busy {
        id: "first".into(),
        abandoned: false,
    };
    let rejected = cccc_web::UpdateHost::begin(
        &coordinator,
        UpdateRequest {
            version: "0.4.59".into(),
            mode: UpdateMode::Restart,
        },
    )
    .err()
    .expect("a second update is refused");
    assert_eq!(rejected.code, "update_in_progress");

    cccc_web::UpdateHost::abandon(&coordinator, "second");
    assert!(matches!(
        &*coordinator.inner.lock(),
        Slot::Busy {
            abandoned: false,
            ..
        }
    ));
    cccc_web::UpdateHost::abandon(&coordinator, "first");
    assert!(matches!(
        &*coordinator.inner.lock(),
        Slot::Busy {
            abandoned: true,
            ..
        }
    ));
}

fn prepared(home: &HomeLayout) -> Prepared {
    let id = uuid::Uuid::new_v4().simple().to_string();
    Prepared {
        transaction: Transaction::create(home, &id, home.root()).expect("transaction"),
        version: "0.4.59".into(),
        mode: UpdateMode::Restart,
        target: BuildIdentity {
            version: "0.4.59".into(),
            source_id: "a".repeat(64),
        },
    }
}

#[tokio::test]
async fn an_update_given_up_after_its_handover_frees_the_host() {
    let (_temp, home) = home();
    let coordinator = coordinator(&home);
    let prepared = prepared(&home);
    let dir = prepared.transaction.dir.clone();
    *coordinator.inner.lock() = Slot::Prepared(prepared);
    // The Web handed it over, then the handover failed before anything stopped.
    let prepared = coordinator.take_prepared().expect("handed over");
    coordinator.undo(prepared).await;
    assert!(matches!(&*coordinator.inner.lock(), Slot::Idle));
    assert!(!dir.exists());
    // Nothing is left to wait for: closing returns at once.
    tokio::time::timeout(Duration::from_secs(5), coordinator.close())
        .await
        .expect("close does not wait");
}

#[cfg(unix)]
#[tokio::test]
async fn closing_the_host_stops_a_running_installer_before_undoing_it() {
    let (temp, home) = home();
    let coordinator = coordinator(&home);
    let install_dir = coordinator.context().install_dir.clone();
    std::fs::create_dir_all(&install_dir).expect("install dir");
    let lock = install_dir.join(".cccc-install.lock");
    let id = uuid::Uuid::new_v4().simple().to_string();
    let transaction = Transaction::create(&home, &id, home.root()).expect("transaction");
    let started = temp.path().join("started");
    let late = temp.path().join("late");
    let staged = temp.path().join("staged");
    let quote = |path: &Path| shell_words::quote(&path.to_string_lossy()).into_owned();
    // An installer stalled in prepare, as on a slow download. Like the real
    // one, it holds the install lock in the name of its transaction and stages
    // a file; a child it left in the background would write `late` later. Its
    // abort releases only a lock its own transaction holds.
    write_script(
        &transaction,
        &format!(
            "lock={lock}\n\
             case \"$CCCC_INSTALL_PHASE\" in\n\
             prepare)\n\
             mkdir \"$lock\" && printf '%s\\n' \"$CCCC_INSTALL_TXN\" > \"$lock/txn\" || exit 1\n\
             : > {staged}\n\
             ( sleep 1; : > {late} ) &\n\
             : > {started}\n\
             sleep 30 ;;\n\
             abort)\n\
             [ \"$(cat \"$lock/txn\")\" = \"$CCCC_INSTALL_TXN\" ] || exit 1\n\
             rm -rf \"$lock\" {staged}\n\
             echo CCCC_INSTALL_ABORTED={id} ;;\n\
             esac",
            lock = quote(&lock),
            staged = quote(&staged),
            late = quote(&late),
            started = quote(&started)
        ),
    );
    *coordinator.inner.lock() = Slot::Busy {
        id: id.clone(),
        abandoned: false,
    };
    // Run the way `begin` runs a preparation, with that prepare phase as the work.
    let inner = Arc::clone(&coordinator.inner);
    let task_id = id.clone();
    let phase_transaction = transaction.clone();
    let preparation = tokio::spawn(async move {
        let context = inner.context.clone();
        let installer = async {
            let run = run_phase(
                &context,
                &phase_transaction,
                "prepare",
                Some("0.4.59"),
                Duration::from_secs(60),
            )
            .await;
            Err::<Prepared, PrepareFailure>((
                Some(phase_transaction.clone()),
                rejection(
                    "installer_finished",
                    format!(
                        "the stalled installer finished: {:?}",
                        run.map(|run| run.success)
                            .map_err(|error| error.to_string())
                    ),
                ),
            ))
        };
        let result = unless_abandoned(&inner, &task_id, installer).await;
        settle_preparation(&inner, &task_id, result).await
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while !started.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the installer started");
    assert!(lock.join("txn").is_file() && staged.is_file());
    tokio::time::timeout(Duration::from_secs(10), coordinator.close())
        .await
        .expect("close does not wait for the installer");
    assert!(matches!(&*coordinator.inner.lock(), Slot::Idle));
    // The same transaction's abort ran once the installer was stopped.
    assert!(!lock.exists());
    assert!(!staged.exists());
    assert!(!transaction.dir.exists());
    let rejection = preparation
        .await
        .expect("preparation task")
        .expect_err("abandoned");
    assert_eq!(rejection.code, "update_abandoned");
    // Its background child would have written this a second after it started.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        !late.exists(),
        "a child of the stopped installer kept running"
    );
}

#[tokio::test]
async fn abandoning_a_preparation_stops_it_and_lets_the_next_one_in() {
    let (_temp, home) = home();
    let coordinator = coordinator(&home);
    let id = uuid::Uuid::new_v4().simple().to_string();
    let transaction = Transaction::create(&home, &id, home.root()).expect("transaction");
    *coordinator.inner.lock() = Slot::Busy {
        id: id.clone(),
        abandoned: false,
    };
    let inner = Arc::clone(&coordinator.inner);
    let task_id = id.clone();
    let preparation = tokio::spawn(async move {
        let result = unless_abandoned(&inner, &task_id, std::future::pending()).await;
        settle_preparation(&inner, &task_id, result).await
    });
    tokio::task::yield_now().await;
    cccc_web::UpdateHost::abandon(&coordinator, &id);
    tokio::time::timeout(Duration::from_secs(5), preparation)
        .await
        .expect("stops at once")
        .expect("preparation task")
        .expect_err("abandoned");
    assert!(matches!(&*coordinator.inner.lock(), Slot::Idle));
    assert!(!transaction.dir.exists());
}

#[test]
fn installer_results_count_only_with_their_status_and_value() {
    let (_temp, home) = home();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let transaction = Transaction::create(&home, &id, home.root()).expect("transaction");
    let aborted = run(&format!("CCCC_INSTALL_ABORTED={id}\n"), "");
    assert!(aborted.aborted(&transaction));
    let other = run("CCCC_INSTALL_ABORTED=someone-else\n", "");
    assert!(!other.aborted(&transaction));
    let failed = PhaseRun {
        success: false,
        ..run(
            &format!("CCCC_INSTALL_ABORTED={id}\n"),
            "CCCC installer: no\n",
        )
    };
    assert!(!failed.aborted(&transaction));
    assert_eq!(failed.problem(), "no");
    assert_eq!(
        other.problem(),
        "the installer did not report the expected result"
    );
    assert!(run("CCCC_INSTALL_APPLIED=0.4.59\n", "").reported("CCCC_INSTALL_APPLIED", "0.4.59"));
    assert!(!run("CCCC_INSTALL_APPLIED=0.4.60\n", "").reported("CCCC_INSTALL_APPLIED", "0.4.59"));
}

#[test]
fn only_an_installer_that_implements_phases_is_accepted() {
    let script = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh"),
    )
    .expect("installer")
    .replace("@CCCC_VERSION@", "0.4.59");
    check_installer(script.as_bytes(), "0.4.59").expect("the current installer");
    // An installer from before phases would install everything in one go.
    let older = script.replace(&format!("{INSTALLER_PHASE_API}\n"), "");
    assert_ne!(older, script);
    let error = check_installer(older.as_bytes(), "0.4.59").expect_err("no phases");
    assert!(format!("{error:#}").contains("cannot update a running CCCC in steps"));
    check_installer(script.as_bytes(), "0.4.60").expect_err("another release");
    check_installer(b"echo hi", "0.4.59").expect_err("not a shell installer");
}

fn write_script(transaction: &Transaction, body: &str) {
    std::fs::write(transaction.script(), format!("#!/bin/sh\n{body}\n")).expect("script");
}

#[cfg(unix)]
#[tokio::test]
async fn an_installer_phase_leaves_nothing_running() {
    let (temp, home) = home();
    let context = coordinator(&home).context().clone();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let transaction = Transaction::create(&home, &id, home.root()).expect("transaction");
    let started = temp.path().join("started");
    let late = temp.path().join("late");
    let quote = |path: &Path| shell_words::quote(&path.to_string_lossy()).into_owned();

    // Timed out: the shell and the child it started in the background both end.
    write_script(
        &transaction,
        &format!(
            "( sleep 0.5; : > {late} ) &\n: > {started}\nsleep 30",
            late = quote(&late),
            started = quote(&started)
        ),
    );
    let timed_out = run_phase(
        &context,
        &transaction,
        "abort",
        None,
        Duration::from_millis(300),
    )
    .await
    .err()
    .expect("the phase times out");
    assert!(format!("{timed_out:#}").contains("did not finish abort"));
    assert!(started.exists(), "the background child was started");
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!late.exists(), "a child of a timed-out phase kept running");

    // Finished: nothing it left in the background outlives it either.
    std::fs::remove_file(&started).expect("reset");
    write_script(
        &transaction,
        &format!(
            "( sleep 0.5; : > {late} ) &\n: > {started}\necho CCCC_INSTALL_ABORTED={id}",
            late = quote(&late),
            started = quote(&started)
        ),
    );
    let finished = run_phase(
        &context,
        &transaction,
        "abort",
        None,
        Duration::from_secs(10),
    )
    .await
    .expect("the phase finishes");
    assert!(finished.aborted(&transaction));
    assert!(started.exists(), "the background child was started");
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!late.exists(), "a child of a finished phase kept running");
}

#[cfg(unix)]
#[tokio::test]
async fn an_unconfirmed_cleanup_keeps_its_transaction() {
    let (_temp, home) = home();
    let context = coordinator(&home).context().clone();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let transaction = Transaction::create(&home, &id, home.root()).expect("transaction");

    write_script(
        &transaction,
        "echo 'CCCC installer: still busy' >&2\nexit 1",
    );
    let error = undo(&context, &transaction)
        .await
        .expect_err("abort failed");
    assert_eq!(format!("{error:#}"), "still busy");
    assert!(transaction.script().is_file());

    write_script(&transaction, "echo CCCC_INSTALL_ABORTED=someone-else");
    undo(&context, &transaction)
        .await
        .expect_err("another transaction's result");
    assert!(transaction.script().is_file());

    write_script(&transaction, &format!("echo CCCC_INSTALL_ABORTED={id}"));
    undo(&context, &transaction).await.expect("aborted");
    assert!(!transaction.dir.exists());

    // Without an installer, nothing ran, so there is nothing to undo.
    let id = uuid::Uuid::new_v4().simple().to_string();
    let transaction = Transaction::create(&home, &id, home.root()).expect("transaction");
    undo(&context, &transaction).await.expect("nothing ran");
    assert!(!transaction.dir.exists());
}

#[test]
fn an_unfinished_cleanup_holds_off_the_next_update() {
    let (_temp, home) = home();
    let coordinator = coordinator(&home);
    let id = uuid::Uuid::new_v4().simple().to_string();
    let transaction = Transaction::create(&home, &id, home.root()).expect("transaction");
    std::fs::write(home.root().join("update/last.json"), "{}").expect("last result");
    let rejected = cccc_web::UpdateHost::begin(
        &coordinator,
        UpdateRequest {
            version: "0.4.59".into(),
            mode: UpdateMode::Restart,
        },
    )
    .err()
    .expect("refused until the cleanup is finished");
    assert_eq!(rejected.code, "update_cleanup_pending");
    // This process created it and still runs, so it has to stop first.
    assert!(
        rejected.message.contains(&format!(
            "stop CCCC, then finish it with `{} update --recover {}`",
            home.root().join("bin/cccc").display(),
            transaction.dir.display()
        )),
        "{}",
        rejected.message
    );
    assert!(matches!(&*coordinator.inner.lock(), Slot::Idle));
}

/// An installation and a transaction driven by the real installer's phases.
#[cfg(target_os = "linux")]
mod recovery {
    use super::*;

    const TARGET: &str = "0.4.59";

    struct Fixture {
        _temp: tempfile::TempDir,
        home: HomeLayout,
        install_dir: PathBuf,
        transaction: Transaction,
    }

    fn installer() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh")
    }

    fn source_id(letter: char) -> String {
        std::iter::repeat_n(letter, 64).collect()
    }

    /// The installed program: answers its self-check as `version`/`source`.
    fn install_program(install_dir: &Path, version: &str, source: &str) {
        use std::os::unix::fs::PermissionsExt;
        let program = install_dir.join("cccc");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\n[ \"$1 $2\" = 'update --self-check' ] || exit 1\n\
                 echo '{{\"schema\":1,\"version\":\"{version}\",\"build\":{{\"source_id\":\"{source}\"}},\"daemon_compatibility\":\"x\",\"relay_schemas\":[1]}}'\n"
            ),
        )
        .expect("program");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .expect("program mode");
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().expect("temp dir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        std::fs::create_dir_all(home.daemon_dir()).expect("daemon dir");
        let install_dir = temp.path().join("bin");
        std::fs::create_dir_all(&install_dir).expect("install dir");
        let install_dir = std::fs::canonicalize(install_dir).expect("canonical install dir");
        std::fs::write(install_dir.join(".cccc-standalone"), "standalone-v1\n").expect("marker");
        install_program(&install_dir, TARGET, &source_id('a'));
        let id = uuid::Uuid::new_v4().simple().to_string();
        let transaction = Transaction::create(&home, &id, &install_dir).expect("transaction");
        std::fs::copy(installer(), transaction.script()).expect("cached installer");
        Fixture {
            _temp: temp,
            home,
            install_dir,
            transaction,
        }
    }

    impl Fixture {
        fn arm(&self, source: &str) {
            let intent = attended_update::Intent::new(
                &self.home,
                &self.transaction.id,
                BuildIdentity {
                    version: TARGET.into(),
                    source_id: source.into(),
                },
                self.install_dir.clone(),
            )
            .expect("intent");
            attended_update::arm(&self.home, &intent).expect("arm");
        }

        /// The installer's own record of a phase it reached, holding its lock.
        fn reach(&self, phase: &str) {
            let lock = self.install_dir.join(".cccc-install.lock");
            std::fs::create_dir_all(&lock).expect("lock");
            std::fs::write(
                lock.join("txn"),
                format!("{}\n", self.transaction.dir.display()),
            )
            .expect("lock owner");
            std::fs::write(
                self.transaction.dir.join("installer.state"),
                format!("phase={phase}\nversion={TARGET}\n"),
            )
            .expect("state");
        }

        /// Hand the transaction to another creator, as if that CCCC made it.
        fn created_by(&self, creator: cccc_core::local_sessions::NativeHost) {
            let path = self.transaction.dir.join("transaction.json");
            let mut record: serde_json::Value =
                cccc_core::fs::read_json(&path).expect("transaction record");
            record["creator"] = serde_json::to_value(creator).expect("creator");
            cccc_core::fs::write_secret_json(&path, &record).expect("rewrite record");
        }

        fn armed(&self) -> bool {
            attended_update::active(&self.home)
                .expect("intent readable")
                .is_some()
        }

        fn last(&self) -> serde_json::Value {
            cccc_core::fs::read_json(&self.home.root().join("update/last.json")).expect("last")
        }
    }

    #[tokio::test]
    async fn a_committed_update_keeps_the_version_the_user_confirmed() {
        let fixture = fixture();
        fixture.arm(&source_id('a'));
        // Confirm deleted the backup and was interrupted before it finished.
        fixture.reach("applied");
        recover(&fixture.home, &fixture.transaction.dir)
            .await
            .expect("recovered");
        assert!(!fixture.armed());
        assert!(!fixture.transaction.dir.exists());
        assert!(!fixture.install_dir.join(".cccc-install.lock").exists());
        let last = fixture.last();
        assert_eq!(last["result"], "kept");
        assert_eq!(last["to_version"], TARGET);
        assert_eq!(last["agents"], "not_observed");
    }

    #[tokio::test]
    async fn a_committed_update_is_kept_only_for_the_build_it_installed() {
        let fixture = fixture();
        fixture.arm(&source_id('b'));
        fixture.reach("applied");
        let error = recover(&fixture.home, &fixture.transaction.dir)
            .await
            .expect_err("another build is installed");
        assert!(format!("{error:#}").contains("is not the CCCC 0.4.59 it installed"));
        assert!(fixture.armed());
        assert_eq!(
            std::fs::read_to_string(fixture.transaction.dir.join("installer.state"))
                .expect("state kept"),
            format!("phase=applied\nversion={TARGET}\n")
        );
    }

    /// What a finished confirm leaves: no backup, no lock, the applied state.
    fn confirmed(fixture: &Fixture) {
        std::fs::write(
            fixture.transaction.dir.join("installer.state"),
            format!("phase=applied\nversion={TARGET}\n"),
        )
        .expect("state");
    }

    #[tokio::test]
    async fn a_finished_confirm_whose_intent_stayed_is_kept_not_undone() {
        let fixture = fixture();
        fixture.arm(&source_id('a'));
        // The host stopped after confirm, before it withdrew the intent.
        confirmed(&fixture);
        recover(&fixture.home, &fixture.transaction.dir)
            .await
            .expect("recovered");
        assert!(!fixture.armed());
        assert!(!fixture.transaction.dir.exists());
        assert_eq!(fixture.last()["result"], "kept");
    }

    #[tokio::test]
    async fn leftovers_of_a_finished_update_are_removed_without_undoing_it() {
        let fixture = fixture();
        // Confirmed and withdrawn, but its directory was left behind.
        confirmed(&fixture);
        let program = std::fs::read(fixture.install_dir.join("cccc")).expect("program");
        recover(&fixture.home, &fixture.transaction.dir)
            .await
            .expect("cleaned up");
        assert!(!fixture.transaction.dir.exists());
        assert!(!fixture.install_dir.join(".cccc-install.lock").exists());
        assert_eq!(
            std::fs::read(fixture.install_dir.join("cccc")).expect("program kept"),
            program
        );
    }

    #[tokio::test]
    async fn a_lost_transaction_directory_still_names_the_way_out() {
        let fixture = fixture();
        fixture.arm(&source_id('a'));
        std::fs::remove_dir_all(&fixture.transaction.dir).expect("lose the transaction");
        let error = recover(&fixture.home, &fixture.transaction.dir)
            .await
            .expect_err("nothing to recover from");
        let message = format!("{error:#}");
        assert!(message.contains("active.json"), "{message}");
        assert!(message.contains("manually to allow startup"), "{message}");
        assert!(fixture.home.root().join("update/active.json").exists());
    }

    #[tokio::test]
    async fn an_update_not_yet_applied_is_undone() {
        let fixture = fixture();
        fixture.arm(&source_id('a'));
        fixture.reach("prepared");
        let staged = fixture
            .install_dir
            .join(format!("cccc.cccc-install.{}", fixture.transaction.id));
        std::fs::write(&staged, "staged").expect("staged");
        recover(&fixture.home, &fixture.transaction.dir)
            .await
            .expect("undone");
        assert!(!staged.exists());
        assert!(!fixture.armed());
        assert!(!fixture.transaction.dir.exists());
        assert_eq!(fixture.last()["result"], "undone");
    }

    #[tokio::test]
    async fn a_transaction_without_its_installer_is_left_alone() {
        let fixture = fixture();
        fixture.arm(&source_id('a'));
        fixture.reach("applied");
        std::fs::remove_file(fixture.transaction.script()).expect("lose the installer");
        let error = recover(&fixture.home, &fixture.transaction.dir)
            .await
            .expect_err("damaged");
        assert!(format!("{error:#}").contains("is damaged"));
        assert!(fixture.armed());
        assert!(fixture.install_dir.join(".cccc-install.lock").exists());
    }

    #[tokio::test]
    async fn an_update_another_running_cccc_is_preparing_is_left_to_it() {
        let fixture = fixture();
        fixture.reach("prepared");
        let mut other = cccc_runtime::background_command("sleep")
            .arg("30")
            .spawn()
            .expect("another CCCC");
        fixture.created_by(
            cccc_core::local_sessions::NativeHost::capture(other.id()).expect("its identity"),
        );
        let error = recover(&fixture.home, &fixture.transaction.dir)
            .await
            .expect_err("its creator still runs");
        assert!(format!("{error:#}").contains("whose exit is not confirmed"));
        assert!(fixture.install_dir.join(".cccc-install.lock").exists());
        other.kill().expect("stop it");
        other.wait().expect("reap it");
        recover(&fixture.home, &fixture.transaction.dir)
            .await
            .expect("cleaned up once its creator exited");
        assert!(!fixture.install_dir.join(".cccc-install.lock").exists());
        assert!(!fixture.transaction.dir.exists());
    }

    #[tokio::test]
    async fn an_update_that_never_reserved_startup_is_cleaned_up_in_its_installation() {
        let fixture = fixture();
        fixture.reach("prepared");
        let staged = fixture
            .install_dir
            .join(format!("cccc.cccc-install.{}", fixture.transaction.id));
        std::fs::write(&staged, "staged").expect("staged");
        recover(&fixture.home, &fixture.transaction.dir)
            .await
            .expect("cleaned up");
        assert!(!staged.exists());
        assert!(!fixture.install_dir.join(".cccc-install.lock").exists());
        assert!(!fixture.transaction.dir.exists());
    }
}
