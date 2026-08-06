use super::{HistoryConfig, TranscriptArchive};
use crate::{read_latest_page, read_latest_since};

fn config(root: &std::path::Path, name: &str, max_bytes: usize) -> HistoryConfig {
    HistoryConfig {
        path: root.join(format!("{name}.pty")),
        max_bytes,
        hot_bytes: 8,
        persist: true,
    }
}

#[test]
fn persists_cursor_pages_and_compacts_to_the_bounded_tail() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut archive =
        TranscriptArchive::create(config(temp.path(), "session", 10)).expect("archive");
    archive
        .append(b"abcdefghijklmnopqrstuvwxyz")
        .expect("append");
    archive.flush().expect("flush");

    let page = read_latest_page(temp.path(), None, 100).expect("latest page");
    assert_eq!(page.data, "qrstuvwxyz");
    assert_eq!(page.start_cursor, 16);
    assert_eq!(page.end_cursor, 26);
    assert!(!page.cursor_expired);

    let expired = read_latest_since(temp.path(), 3, 100).expect("expired page");
    assert_eq!(expired.data, "qrstuvwxyz");
    assert_eq!(expired.start_cursor, 16);
    assert!(expired.cursor_expired);
}

#[test]
fn latest_pointer_survives_archive_reopen() {
    let temp = tempfile::tempdir().expect("tempdir");
    {
        let mut archive =
            TranscriptArchive::create(config(temp.path(), "first", 1024)).expect("archive");
        archive.append("first 你好".as_bytes()).expect("append");
        archive.flush().expect("flush");
    }

    let page = read_latest_page(temp.path(), None, 1024).expect("reopen");
    assert_eq!(page.data, "first 你好");
    assert_eq!(page.start_cursor, 0);
    assert_eq!(page.end_cursor, "first 你好".len() as u64);
}

#[test]
fn sessions_share_one_cursor_space_and_page_across_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut first = TranscriptArchive::create(config(temp.path(), "first", 1024)).expect("first");
    first.append(b"first").expect("append first");
    first.flush().expect("flush first");
    drop(first);

    let mut second =
        TranscriptArchive::create(config(temp.path(), "second", 1024)).expect("second");
    assert_eq!(second.end_cursor(), 5);
    second.append(b" second").expect("append second");
    second.flush().expect("flush second");

    let page = read_latest_page(temp.path(), None, 1024).expect("combined page");
    assert_eq!(page.data, "first second");
    assert_eq!(page.start_cursor, 0);
    assert_eq!(page.end_cursor, 12);

    let since = read_latest_since(temp.path(), 5, 1024).expect("second session");
    assert_eq!(since.data, " second");
    assert_eq!(since.start_cursor, 5);
    assert_eq!(since.end_cursor, 12);
}

#[test]
fn clear_removes_history_from_older_session_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut first = TranscriptArchive::create(config(temp.path(), "first", 1024)).expect("first");
    first.append(b"old").expect("append first");
    first.flush().expect("flush first");
    drop(first);

    let mut second =
        TranscriptArchive::create(config(temp.path(), "second", 1024)).expect("second");
    second.clear().expect("clear");
    second.append(b"new").expect("append second");

    let page = read_latest_page(temp.path(), None, 1024).expect("cleared page");
    assert_eq!(page.data, "new");
    assert_eq!(page.start_cursor, 3);
    assert_eq!(page.end_cursor, 6);
    assert!(!temp.path().join("first.pty").exists());
}

#[test]
fn a_new_session_becomes_latest_and_prunes_old_files_to_the_actor_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut first = TranscriptArchive::create(config(temp.path(), "first", 64)).expect("first");
    first.append(&[b'a'; 48]).expect("append first");
    first.flush().expect("flush first");
    drop(first);

    let mut second = TranscriptArchive::create(config(temp.path(), "second", 64)).expect("second");
    second.append(&[b'b'; 48]).expect("append second");
    second.flush().expect("flush second");

    let page = read_latest_page(temp.path(), None, 64).expect("latest");
    assert_eq!(page.data, "b".repeat(48));
    assert_eq!(page.start_cursor, 48);
    assert_eq!(page.end_cursor, 96);
    assert!(!temp.path().join("first.pty").exists());
}
