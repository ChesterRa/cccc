use super::*;
use crate::{GroupDoc, GroupStore, HomeLayout, inbox, ledger, ledger_archive};
use cccc_contracts::Actor;
use flate2::{Compression, write::GzEncoder};
use serde_json::json;
use std::io::Write;

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: GroupDoc,
    ledger: PathBuf,
    segments: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("snapshot boundaries", "").expect("group");
        group
            .actors
            .extend([Actor::new("reader"), Actor::new("other")]);
        store.save(&group).expect("save");
        let ledger = store.ledger_path(&group.group_id).expect("ledger");
        File::create(&ledger).expect("empty active ledger");
        let segments = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("ledger/segments");
        std::fs::create_dir_all(&segments).expect("segments");
        Self {
            _temp: temp,
            home,
            group,
            ledger,
            segments,
        }
    }

    fn message(&self, mode: &str, number: usize) -> Event {
        let mut event = Event::new("chat.message", &self.group.group_id);
        event.by = "user".into();
        event.data =
            json!({"to":["reader","other"],"message_mode":mode,"text":format!("fixture {number}")})
                .as_object()
                .expect("data")
                .clone();
        event
    }

    fn gzip_page(&self) -> (Vec<String>, Vec<String>) {
        let file = File::create(self.segments.join("ledger.000001.jsonl.gz")).expect("gzip");
        let mut gzip = GzEncoder::new(file, Compression::fast());
        let mut mails = Vec::new();
        let mut pushes = Vec::new();
        for number in 0..128 {
            for (mode, ids) in [("mail", &mut mails), ("send", &mut pushes)] {
                let mut event = self.message(mode, number);
                event
                    .data
                    .insert("text".into(), json!("fixed synthetic payload ".repeat(512)));
                serde_json::to_writer(&mut gzip, &event).expect("event");
                gzip.write_all(b"\n").expect("newline");
                ids.push(event.id);
            }
        }
        gzip.finish().expect("finish gzip");
        (mails, pushes)
    }
}

#[cfg(unix)]
#[test]
fn many_archives_fit_low_descriptor_limit() {
    const TEST: &str = "ledger::snapshot::tests::many_archives_fit_low_descriptor_limit";
    const CHILD: &str = "CCCC_TEST_INBOX_LOW_FD_CHILD";
    if std::env::var(CHILD).as_deref() != Ok("1") {
        let output = std::process::Command::new("sh")
            .args(["-c", "ulimit -n 128; exec \"$@\"", "cccc-inbox-fd-test"])
            .arg(std::env::current_exe().expect("test executable"))
            .args(["--exact", TEST, "--nocapture"])
            .env(CHILD, "1")
            .output()
            .expect("isolated low-FD child");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let f = Fixture::new();
    let mut ids = Vec::new();
    for number in 0..80 {
        let event = f.message("mail", number);
        let mut file =
            File::create(f.segments.join(format!("ledger.{number:06}.jsonl"))).expect("segment");
        serde_json::to_writer(&mut file, &event).expect("event");
        file.write_all(b"\n").expect("newline");
        ids.push(event.id);
    }
    let page = inbox::list_unread(&f.home, &f.group, "reader", 100).expect("Inbox under FD limit");
    assert_eq!(
        page.iter().map(|event| &event.id).collect::<Vec<_>>(),
        ids.iter().collect::<Vec<_>>()
    );
    // Also exercise the startup streaming reader with more segments than the
    // descriptor limit, independently of Inbox's publication snapshot.
    for number in 80..160 {
        let event = f.message("mail", number);
        let mut file =
            File::create(f.segments.join(format!("ledger.{number:06}.jsonl"))).expect("segment");
        serde_json::to_writer(&mut file, &event).expect("event");
        file.write_all(b"\n").expect("newline");
        ids.push(event.id);
    }
    let mut visited = Vec::new();
    ledger::visit_snapshot_oldest_first(&f.ledger, |event| {
        visited.push(event.id);
        ControlFlow::Continue(())
    })
    .expect("startup-style streaming read under FD limit");
    assert_eq!(visited, ids);
}

#[test]
fn compressed_inbox_pages_decode_each_archive_once() {
    let f = Fixture::new();
    let (ids, _) = f.gzip_page();
    let actors = ["reader".to_owned(), "other".to_owned()];
    inbox::list_unread_many(&f.home, &f.group, &actors, 128).expect("warm projection");
    let before = GZIP_READERS.get();
    let pages = inbox::list_unread_many(&f.home, &f.group, &actors, 128).expect("warm Inbox pages");
    for actor in actors {
        assert_eq!(
            pages[&actor]
                .iter()
                .map(|event| &event.id)
                .collect::<Vec<_>>(),
            ids.iter().collect::<Vec<_>>()
        );
    }
    assert_eq!(
        GZIP_READERS.get() - before,
        1,
        "one decoder for the shared archived page, including duplicate recipients"
    );
}

#[test]
fn compressed_delivery_recovery_decodes_each_archive_once() {
    let f = Fixture::new();
    let (_, ids) = f.gzip_page();
    inbox::pending_deliveries(&f.home, &f.group, "reader", 128).expect("warm recovery projection");
    let before = GZIP_READERS.get();
    let page = inbox::pending_deliveries(&f.home, &f.group, "reader", 128).expect("recovery page");
    assert_eq!(
        page.iter().map(|event| &event.id).collect::<Vec<_>>(),
        ids.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        GZIP_READERS.get() - before,
        1,
        "one decoder for archived pending deliveries"
    );
}

#[test]
fn broken_archive_returns_an_error_and_does_not_poison_the_next_query() {
    let f = Fixture::new();
    let (ids, _) = f.gzip_page();
    inbox::list_unread(&f.home, &f.group, "reader", 128).expect("warm page");
    let broken = f.segments.join("ledger.000002.jsonl.gz");
    std::fs::write(&broken, b"not a gzip stream").expect("broken fixture");
    assert!(inbox::list_unread(&f.home, &f.group, "reader", 128).is_err());
    std::fs::remove_file(broken).expect("remove broken fixture");
    let page = inbox::list_unread(&f.home, &f.group, "reader", 128).expect("recovered page");
    assert_eq!(
        page.iter().map(|event| &event.id).collect::<Vec<_>>(),
        ids.iter().collect::<Vec<_>>()
    );
}

#[test]
fn snapshot_keeps_the_committed_active_prefix_after_rotation() {
    let f = Fixture::new();
    let before = f.message("mail", 0);
    ledger::append(&f.ledger, &before).expect("initial");
    let snapshot = Snapshot::open(&f.ledger).expect("snapshot");
    ledger_archive::compact(&f.home, &f.group.group_id, "snapshot fixture").expect("rotate");
    ledger::append(&f.ledger, &f.message("mail", 1)).expect("new active file");
    let mut read = Vec::new();
    snapshot
        .visit(&[], |event, _| {
            read.push(event.id);
            ControlFlow::Continue(())
        })
        .expect("pinned prefix");
    assert_eq!(read, [before.id]);
}

#[test]
fn batch_reads_preserve_request_order_gaps_and_duplicates_across_sources() {
    let f = Fixture::new();
    let mut ids = Vec::new();
    for segment in 0..3 {
        let path = f.segments.join(format!("ledger.{segment:06}.jsonl.gz"));
        let mut gzip = GzEncoder::new(File::create(path).expect("segment"), Compression::fast());
        for number in 0..5 {
            let event = f.message("mail", segment * 5 + number);
            serde_json::to_writer(&mut gzip, &event).expect("event");
            gzip.write_all(b"\n").expect("newline");
            ids.push(event.id);
        }
        gzip.finish().expect("finish gzip");
    }
    let event = f.message("mail", 15);
    let mut plain = File::create(f.segments.join("ledger.000003.jsonl")).expect("plain segment");
    serde_json::to_writer(&mut plain, &event).expect("event");
    plain.write_all(b"\n").expect("newline");
    ids.push(event.id);
    let event = f.message("mail", 16);
    ledger::append(&f.ledger, &event).expect("active event");
    ids.push(event.id);
    let snapshot = Snapshot::open(&f.ledger).expect("snapshot");
    let mut locations = Vec::new();
    snapshot
        .visit(&[], |_, location| {
            locations.push(location);
            ControlFlow::Continue(())
        })
        .expect("locations");
    let order = [16, 14, 0, 4, 15, 12, 14, 0];
    let requested: Vec<_> = order.iter().map(|index| locations[*index]).collect();
    let before = GZIP_READERS.get();
    let page = snapshot.events(&requested).expect("mixed page");
    assert_eq!(
        page.iter().map(|event| &event.id).collect::<Vec<_>>(),
        order.iter().map(|index| &ids[*index]).collect::<Vec<_>>()
    );
    assert_eq!(GZIP_READERS.get() - before, 2, "only selected gzip sources");
    assert!(snapshot.events(&[]).expect("empty page").is_empty());
}
