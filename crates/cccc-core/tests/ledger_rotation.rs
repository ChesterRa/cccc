use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout, ledger, ledger_archive};
use std::io::Write;

#[test]
fn follower_preserves_unseen_events_across_rotation_and_refill() {
    let mut failures = Vec::new();
    for mode in ["refill", "multiple", "gzip", "gzip-multiple"] {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("rotation", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger");
        let mut initial = Event::new("chat.message", &group.group_id);
        initial
            .data
            .insert("text".into(), "historical ".repeat(100).into());
        ledger::append(&path, &initial).expect("initial");
        let (mut follower, _) = ledger::LedgerFollower::at_end(&path).expect("follower");
        let mut expected = Vec::new();
        let before = Event::new("chat.message", &group.group_id);
        ledger::append(&path, &before).expect("unseen before rotation");
        expected.push(before.id);
        let first_segment = ledger_archive::compact(&home, &group.group_id, "fixture")
            .expect("rotate")
            .expect("segment");
        if mode.starts_with("gzip") {
            let file =
                std::fs::File::create(first_segment.with_extension("jsonl.gz")).expect("gzip");
            let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            encoder
                .write_all(&std::fs::read(&first_segment).expect("segment"))
                .expect("compress");
            encoder.finish().expect("finish");
            std::fs::remove_file(first_segment).expect("remove compressed source");
        }
        for _ in 0..12 {
            let event = Event::new("chat.message", &group.group_id);
            ledger::append(&path, &event).expect("refill");
            expected.push(event.id);
        }
        if mode.ends_with("multiple") {
            let segment = ledger_archive::compact(&home, &group.group_id, "second fixture")
                .expect("second rotation")
                .expect("second segment");
            if mode == "gzip-multiple" {
                let file = std::fs::File::create(segment.with_extension("jsonl.gz")).expect("gzip");
                let mut encoder =
                    flate2::write::GzEncoder::new(file, flate2::Compression::default());
                encoder
                    .write_all(&std::fs::read(&segment).expect("segment"))
                    .expect("compress");
                encoder.finish().expect("finish");
                std::fs::remove_file(segment).expect("remove compressed source");
            }
        }
        let actual: Vec<_> = follower
            .poll(&path)
            .expect("poll after rotations")
            .into_iter()
            .map(|event| event.id)
            .collect();
        if actual != expected {
            failures.push(format!(
                "{mode}: received {} of {} events; exact sequence differs",
                actual.len(),
                expected.len()
            ));
        }
        assert!(follower.poll(&path).expect("poll again").is_empty());
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn missing_rotation_cursor_fails_without_advancing_and_recovers_after_repair() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("missing cursor", "").expect("group");
    let path = store.ledger_path(&group.group_id).expect("ledger");
    let initial = Event::new("chat.message", &group.group_id);
    ledger::append(&path, &initial).expect("initial");
    let (mut follower, _) = ledger::LedgerFollower::at_end(&path).expect("follower");
    let segment = ledger_archive::compact(&home, &group.group_id, "fixture")
        .expect("compact")
        .expect("segment");
    let original = std::fs::read(&segment).expect("original");
    std::fs::write(&segment, b"").expect("damage isolated archive");
    let next = Event::new("chat.message", &group.group_id);
    ledger::append(&path, &next).expect("next");
    assert_eq!(
        follower
            .poll(&path)
            .expect_err("lost cursor must fail")
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    std::fs::write(&segment, original).expect("restore exact source");
    let actual = follower.poll(&path).expect("recover");
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].id, next.id);
    assert!(follower.poll(&path).expect("poll again").is_empty());
}

#[test]
fn concurrent_queries_appends_and_compaction_preserve_committed_prefixes() {
    use std::sync::{Arc, Barrier};
    use std::thread;

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("concurrent recovery", "").expect("group");
    let path = store.ledger_path(&group.group_id).expect("ledger");
    let initial = ledger::read_all(&path).expect("initial history");
    let messages: Vec<_> = (0..120)
        .map(|_| Event::new("chat.message", &group.group_id))
        .collect();
    let expected: Arc<Vec<_>> = Arc::new(
        initial
            .into_iter()
            .chain(messages.iter().cloned())
            .collect(),
    );
    let start = Arc::new(Barrier::new(4));
    thread::scope(|scope| {
        scope.spawn(|| {
            start.wait();
            for event in &messages {
                ledger::append(&path, event).expect("concurrent append");
                thread::yield_now();
            }
        });
        scope.spawn(|| {
            start.wait();
            for _ in 0..12 {
                ledger_archive::compact(&home, &group.group_id, "concurrent fixture")
                    .expect("compact");
                thread::yield_now();
            }
        });
        for _ in 0..2 {
            scope.spawn(|| {
                start.wait();
                let mut previous = 0;
                for _ in 0..80 {
                    let events = ledger::read_all(&path).expect("query during mutation");
                    assert!(events.len() >= previous, "committed history cannot shrink");
                    assert_eq!(
                        events,
                        expected[..events.len()],
                        "query must return an exact committed prefix"
                    );
                    previous = events.len();
                    thread::yield_now();
                }
            });
        }
    });
    assert_eq!(ledger::read_all(&path).expect("final history"), *expected);
}
