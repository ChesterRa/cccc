use super::*;

fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("private Home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("Home");
    home.initialize().expect("initialize");
    (temp, home)
}

fn root(home: &HomeLayout) -> PathBuf {
    home.root().join("state/session_nodes")
}

fn active(home: &HomeLayout) -> BTreeMap<String, Attempt> {
    fs::read_json::<Journal>(&root(home).join("deliveries.json"))
        .expect("active journal")
        .attempts
}

#[test]
fn completing_attempts_releases_slots_but_retains_exact_results_and_payload_fences() {
    let (_temp, home) = home();
    for (id, outcome) in [
        ("accepted", Outcome::Accepted),
        (
            "rejected",
            Outcome::Rejected {
                reason: "rejected before admission".into(),
            },
        ),
        ("unavailable", Outcome::Unavailable),
    ] {
        assert!(begin(&home, id, "payload").expect("begin").is_none());
        finish(&home, id, "payload", &outcome).expect("finish");
        assert!(active(&home).is_empty());
        let reopened = HomeLayout::from_path(home.root()).expect("reopen");
        let replay = begin(&reopened, id, "payload")
            .expect("receipt")
            .expect("known outcome");
        assert_eq!(
            serde_json::to_value(replay).expect("serialize replayed delivery outcome"),
            serde_json::to_value(outcome).expect("serialize original delivery outcome")
        );
        assert!(begin(&reopened, id, "different").is_err());
    }
}

#[test]
fn migration_keeps_in_flight_and_unknown_fences_and_bounds_each_admission_batch() {
    let (_temp, home) = home();
    begin(&home, "in-flight", "pending").expect("begin pending");
    begin(&home, "unknown", "uncertain").expect("begin unknown");
    finish(
        &home,
        "unknown",
        "uncertain",
        &Outcome::Ambiguous {
            reason: "unknown".into(),
        },
    )
    .expect("unknown result");
    mutate(&home, |_, state| {
        for index in 0..MIGRATION_BATCH + 1 {
            state.insert(
                format!("legacy-{index}"),
                Attempt {
                    fingerprint: fingerprint("old"),
                    outcome: Outcome::Accepted,
                },
            );
        }
        Ok(())
    })
    .expect("legacy fixture");
    assert!(begin(&home, "new", "new payload").expect("admit").is_none());
    let retained = active(&home);
    assert_eq!(
        retained.len(),
        4,
        "one batch migrated, one legacy completion left"
    );
    for (id, payload) in [("in-flight", "pending"), ("unknown", "uncertain")] {
        assert!(matches!(
            begin(&home, id, payload).expect("fenced"),
            Some(Outcome::Ambiguous { .. })
        ));
        assert!(retained.contains_key(id));
        assert!(!receipt_path(&root(&home), id).exists());
    }
    assert!(matches!(
        begin(&home, "legacy-0", "old").expect("legacy replay"),
        Some(Outcome::Accepted)
    ));
    assert!(begin(&home, "legacy-0", "changed").is_err());
}

#[test]
fn durable_completion_recovers_after_crash_before_active_fence_removal() {
    let (_temp, home) = home();
    begin(&home, "delivery", "payload").expect("pending");
    // Persist the final receipt, deliberately retaining the pre-finish active
    // fence: this is the on-disk state if the process dies between the two writes.
    archive(
        &root(&home),
        "delivery",
        &Attempt {
            fingerprint: fingerprint("payload"),
            outcome: Outcome::Accepted,
        },
    )
    .expect("durable receipt");
    assert!(matches!(
        active(&home)["delivery"].outcome,
        Outcome::Ambiguous { .. }
    ));
    assert!(matches!(
        begin(&home, "delivery", "payload").expect("recover"),
        Some(Outcome::Accepted)
    ));
    assert!(active(&home).is_empty());
    assert!(begin(&home, "delivery", "other").is_err());
}

#[test]
fn failed_receipt_persistence_keeps_the_unknown_fence() {
    let (_temp, home) = home();
    begin(&home, "delivery", "payload").expect("pending");
    // A file where the archive directory must be makes real filesystem I/O fail
    // deterministically, without relying on permissions or mocking the writer.
    std::fs::write(root(&home).join("delivery_receipts"), b"not a directory").expect("I/O fault");
    assert!(finish(&home, "delivery", "payload", &Outcome::Accepted).is_err());
    assert!(matches!(
        active(&home)["delivery"].outcome,
        Outcome::Ambiguous { .. }
    ));
    std::fs::remove_file(root(&home).join("delivery_receipts")).expect("restore storage");
    assert!(matches!(
        begin(&home, "delivery", "payload").expect("fenced"),
        Some(Outcome::Ambiguous { .. })
    ));
}

#[test]
fn corrupt_or_conflicting_archives_fail_closed_without_releasing_active_fences() {
    let (_temp, home) = home();
    begin(&home, "delivery", "payload").expect("pending");
    let journal = root(&home).join("deliveries.json");
    let before = std::fs::read(&journal).expect("original unknown fence");
    assert!(matches!(
        active(&home)["delivery"].outcome,
        Outcome::Ambiguous { .. }
    ));
    let path = receipt_path(&root(&home), "delivery");
    std::fs::create_dir_all(path.parent().expect("receipt path has a shard directory"))
        .expect("shard");
    std::fs::write(&path, b"broken json").expect("corrupt receipt");
    assert!(begin(&home, "delivery", "payload").is_err());
    assert_eq!(
        std::fs::read(&journal).expect("begin preserved original journal"),
        before
    );
    assert!(finish(&home, "delivery", "payload", &Outcome::Accepted).is_err());
    assert!(active(&home).contains_key("delivery"));
    assert!(matches!(
        active(&home)["delivery"].outcome,
        Outcome::Ambiguous { .. }
    ));
    assert_eq!(
        std::fs::read(&journal).expect("unchanged unknown fence"),
        before
    );
    fs::write_secret_json(
        &path,
        &Attempt {
            fingerprint: fingerprint("other"),
            outcome: Outcome::Accepted,
        },
    )
    .expect("conflicting receipt");
    assert!(begin(&home, "delivery", "payload").is_err());
    assert_eq!(
        std::fs::read(&journal).expect("begin preserved original journal"),
        before
    );
    assert!(finish(&home, "delivery", "payload", &Outcome::Accepted).is_err());
    assert!(active(&home).contains_key("delivery"));
    assert!(matches!(
        active(&home)["delivery"].outcome,
        Outcome::Ambiguous { .. }
    ));
    assert_eq!(
        std::fs::read(&journal).expect("unchanged unknown fence"),
        before
    );
    std::fs::remove_file(&path).expect("remove faulty receipt");
    let reopened = HomeLayout::from_path(home.root()).expect("reopen after fault");
    assert!(matches!(
        begin(&reopened, "delivery", "payload").expect("still fenced"),
        Some(Outcome::Ambiguous { .. })
    ));
    assert_eq!(
        std::fs::read(&journal).expect("preserved after recovery"),
        before
    );
    assert!(begin(&reopened, "delivery", "different").is_err());
}
#[test]
fn unknown_results_cannot_be_misread_as_completed_receipts() {
    let (_temp, home) = home();
    let path = receipt_path(&root(&home), "delivery");
    fs::write_secret_json(
        &path,
        &Attempt {
            fingerprint: fingerprint("payload"),
            outcome: Outcome::Ambiguous {
                reason: "unknown".into(),
            },
        },
    )
    .expect("invalid archive fixture");
    assert!(begin(&home, "delivery", "payload").is_err());
}

#[test]
fn old_readers_and_unknown_future_formats_fail_closed() {
    let (_temp, home) = home();
    begin(&home, "delivery", "payload").expect("pending");
    finish(&home, "delivery", "payload", &Outcome::Accepted).expect("complete");
    let path = root(&home).join("deliveries.json");
    assert!(
        fs::read_json::<BTreeMap<String, Attempt>>(&path).is_err(),
        "old reader must not ignore receipts and admit a duplicate"
    );
    fs::write_secret_json(
        &path,
        &Journal {
            version: 3,
            attempts: BTreeMap::new(),
        },
    )
    .expect("future format");
    assert!(begin(&home, "new", "new payload").is_err());
    assert_eq!(
        fs::read_json::<Journal>(&path)
            .expect("read preserved future journal")
            .version,
        3
    );
}
