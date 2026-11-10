use cccc_contracts::{Actor, Event};
use cccc_core::{GroupStore, HomeLayout, inbox, ledger, ledger_archive};
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

fn mail(group_id: &str, number: usize) -> Event {
    let mut event = Event::new("chat.message", group_id);
    event.by = "user".into();
    event.data = json!({"to":["reader"],"message_mode":"mail","text":number.to_string()})
        .as_object()
        .expect("message data")
        .clone();
    event
}

/// The rotated segments of a ledger, as a directory listing sorts them. Each name carries the
/// time of its rotation and its sequence number, so a failure shows whether the two agree.
fn segment_names(ledger: &Path) -> Vec<String> {
    let segments = ledger.with_file_name("state").join("ledger/segments");
    let mut names = std::fs::read_dir(segments)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn consuming_an_overflowing_inbox_preserves_order_during_append_and_rotation() {
    const EXISTING: usize = 1025;
    const APPENDED: usize = 128;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("concurrent Inbox", "").expect("group");
    group.actors.push(Actor::new("reader"));
    store.save(&group).expect("save group");
    let path = store.ledger_path(&group.group_id).expect("ledger");
    let segments_of = path.clone();
    let mut expected = Vec::new();
    for number in 0..EXISTING {
        let event = mail(&group.group_id, number);
        ledger::append(&path, &event).expect("existing Mail");
        expected.push(event.id);
    }
    assert_eq!(
        inbox::list_unread(&home, &group, "reader", 1000)
            .expect("warm overflowing page")
            .len(),
        1000
    );

    let barrier = Arc::new(Barrier::new(2));
    let appended = Arc::new(AtomicUsize::new(0));
    let writer = {
        let home = home.clone();
        let group_id = group.group_id.clone();
        let barrier = barrier.clone();
        let appended = appended.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let mut ids = Vec::new();
            for number in EXISTING..EXISTING + APPENDED {
                let event = mail(&group_id, number);
                ledger::append(&path, &event).expect("concurrent Mail");
                appended.fetch_add(1, Ordering::Relaxed);
                ids.push(event.id);
                if ids.len() % 32 == 0 {
                    ledger_archive::compact(&home, &group_id, "Inbox regression")
                        .expect("concurrent compaction");
                }
                std::thread::yield_now();
            }
            ids
        })
    };
    barrier.wait();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut received = Vec::new();
    // What a failure needs to tell apart: a writer that is still writing, and a reader that
    // gets nothing although everything was written.
    let (mut pages, mut last_page, mut empty_pages_in_a_row) = (0_usize, 0_usize, 0_usize);
    let mut last_progress = Instant::now();
    while received.len() < EXISTING + APPENDED {
        assert!(
            Instant::now() < deadline,
            "Inbox stopped making progress: received {} of {}, last {:?}; writer appended {} of \
             {APPENDED}, finished {}; {pages} pages, the last one {last_page} long, \
             {empty_pages_in_a_row} empty in a row, {:?} since the last non-empty page; \
             segments {:?}",
            received.len(),
            EXISTING + APPENDED,
            received.last(),
            appended.load(Ordering::Relaxed),
            writer.is_finished(),
            last_progress.elapsed(),
            segment_names(&segments_of),
        );
        let page = inbox::consume_unread(&home, &group, "reader", "reader", 37)
            .expect("consume during writes and rotation");
        pages += 1;
        last_page = page.messages.len();
        if last_page == 0 {
            empty_pages_in_a_row += 1;
        } else {
            empty_pages_in_a_row = 0;
            last_progress = Instant::now();
        }
        received.extend(page.messages.into_iter().map(|event| event.id));
        std::thread::yield_now();
    }
    expected.extend(writer.join().expect("writer"));
    assert_eq!(
        received, expected,
        "each Mail is consumed once, in ledger order"
    );
    assert!(
        inbox::list_unread(&home, &group, "reader", 10)
            .expect("final page")
            .is_empty()
    );
}
