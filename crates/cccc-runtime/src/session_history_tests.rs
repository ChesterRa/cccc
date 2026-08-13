use crate::RuntimeError;
use crate::session_history::InitialHistory;
use crate::session_history::SessionHistory;
use crate::transcript_archive::HistoryConfig;

fn config(root: &std::path::Path) -> HistoryConfig {
    HistoryConfig {
        path: root.join("session.pty"),
        max_bytes: 1024,
        hot_bytes: 1024,
        persist: true,
    }
}

#[test]
fn clear_keeps_archive_and_hot_buffer_aligned() {
    let temp = tempfile::tempdir().expect("tempdir");
    let history = SessionHistory::new(Some(config(temp.path()))).expect("history");
    history.push(b"old").expect("old");
    history.clear().expect("clear");
    history.push(b"new").expect("new");

    assert_eq!(history.page(None, 1024).expect("archive").data, "new");
    assert_eq!(history.retained_page().expect("hot").data, "new");
}

#[cfg(unix)]
#[test]
fn archive_failure_falls_back_to_the_hot_buffer() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("history");
    let history = SessionHistory::new(Some(config(&root))).expect("history");
    std::fs::remove_dir_all(&root).expect("remove archive directory");

    assert!(history.push(b"first").is_err());
    history.push(b" second").expect("hot-buffer fallback");

    assert_eq!(
        history.page(None, 1024).expect("fallback page").data,
        "first second"
    );
}

#[test]
fn persistence_can_be_disabled_without_losing_hot_history() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("memory-only");
    let mut history_config = config(&root);
    history_config.persist = false;
    let history = SessionHistory::new(Some(history_config)).expect("history");

    history.push(b"memory only").expect("push");

    assert_eq!(
        history.page(None, 1024).expect("history page").data,
        "memory only"
    );
    assert!(!root.exists());
}

#[test]
fn archive_creation_failure_does_not_block_in_memory_history() {
    let temp = tempfile::tempdir().expect("tempdir");
    let blocker = temp.path().join("not-a-directory");
    std::fs::write(&blocker, b"file").expect("blocker");
    let history = SessionHistory::new(Some(config(&blocker))).expect("memory fallback");

    history.push(b"still available").expect("push");

    assert_eq!(
        history.retained_page().expect("hot history").data,
        "still available"
    );
}

#[test]
fn archive_creation_fallback_keeps_the_replacement_cursor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let blocker = temp.path().join("not-a-directory");
    std::fs::write(&blocker, b"file").expect("blocker");
    let history =
        SessionHistory::new_at(Some(config(&blocker)), 42).expect("memory fallback with cursor");

    history.push(b"replacement").expect("push");

    let page = history.page_since(42, 1024).expect("replacement page");
    assert_eq!(page.data, "replacement");
    assert_eq!(page.start_cursor, 42);
    assert!(!page.cursor_expired);
}

#[test]
fn terminal_mirror_resize_commits_only_after_the_pty_resize() {
    let history = SessionHistory::new(None).expect("history");
    history.push(b"visible output").expect("push");

    let error = history
        .resize_terminal_with(120, 40, || {
            Err(RuntimeError::Io(std::io::Error::other(
                "synthetic PTY resize failure",
            )))
        })
        .expect_err("resize must fail");
    assert!(error.to_string().contains("synthetic PTY resize failure"));
    assert_eq!(snapshot_size(&history), (80, 24));

    history
        .resize_terminal_with(120, 40, || Ok(()))
        .expect("resize");
    assert_eq!(snapshot_size(&history), (120, 40));
}

fn snapshot_size(history: &SessionHistory) -> (u16, u16) {
    match history.subscribe(None, true).expect("subscription").initial {
        InitialHistory::Snapshot(snapshot) => (snapshot.cols, snapshot.rows),
        InitialHistory::Replay(_) => panic!("expected terminal snapshot"),
    }
}
