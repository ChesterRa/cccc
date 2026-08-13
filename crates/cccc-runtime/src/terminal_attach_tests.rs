use super::{AttachmentRegistry, TerminalAttachMode, TerminalAttachOptions, TerminalAttachment};
use crate::RuntimeError;
use crate::session_history::SessionHistory;
use crate::terminal_initial_output::TerminalInitialOutputKind;
use crate::transcript_archive::HistoryConfig;

fn attachment(
    history: &SessionHistory,
    registry: &AttachmentRegistry,
    mode: TerminalAttachMode,
    takeover: bool,
) -> TerminalAttachment {
    TerminalAttachment::new(
        "g_attach".into(),
        "peer".into(),
        TerminalAttachOptions {
            mode,
            takeover,
            since: None,
            prefer_snapshot: false,
        },
        registry.clone(),
        history.clone(),
    )
    .expect("attachment")
}

#[tokio::test]
async fn negotiated_snapshot_uses_the_raw_cursor_as_its_live_fence() {
    let history = SessionHistory::new(None).expect("history");
    let raw = b"old\r\n\x1b[2J\x1b[Hlatest";
    history.push(raw).expect("screen output");
    let registry = AttachmentRegistry::default();
    let mut client = TerminalAttachment::new(
        "g_attach".into(),
        "peer".into(),
        TerminalAttachOptions {
            mode: TerminalAttachMode::Viewer,
            takeover: false,
            since: None,
            prefer_snapshot: true,
        },
        registry,
        history.clone(),
    )
    .expect("attachment");

    let snapshot = client.take_initial_output();
    assert_eq!(snapshot.kind, TerminalInitialOutputKind::Snapshot);
    assert_eq!(snapshot.start_cursor, raw.len() as u64);
    assert_eq!(snapshot.end_cursor, raw.len() as u64);
    assert!(!snapshot.data.is_empty());

    history.push(b" live").expect("live output");
    let live = client
        .next_output(64)
        .await
        .expect("next output")
        .expect("live page");
    assert_eq!(live.start_cursor, raw.len() as u64);
    assert_eq!(live.data, b" live");
}

#[test]
fn clearing_history_also_removes_old_output_from_future_snapshots() {
    let history = SessionHistory::new(None).expect("history");
    history.push(b"secret-old-output").expect("old output");
    history.clear().expect("clear");
    history.push(b"new-output").expect("new output");
    let mut client = TerminalAttachment::new(
        "g_attach".into(),
        "peer".into(),
        TerminalAttachOptions {
            mode: TerminalAttachMode::Viewer,
            takeover: false,
            since: None,
            prefer_snapshot: true,
        },
        AttachmentRegistry::default(),
        history,
    )
    .expect("attachment");

    let initial = client.take_initial_output();
    assert_eq!(initial.kind, TerminalInitialOutputKind::Snapshot);
    assert!(
        initial
            .data
            .windows(b"new-output".len())
            .any(|chunk| chunk == b"new-output")
    );
    assert!(
        !initial
            .data
            .windows(b"secret-old-output".len())
            .any(|chunk| chunk == b"secret-old-output")
    );
}

#[test]
fn expired_negotiated_cursor_uses_a_snapshot_at_the_current_raw_cursor() {
    let history = SessionHistory::new(Some(HistoryConfig {
        path: "unused.pty".into(),
        max_bytes: 4,
        hot_bytes: 4,
        persist: false,
    }))
    .expect("history");
    history.push(b"12345678").expect("output");
    let mut client = TerminalAttachment::new(
        "g_attach".into(),
        "peer".into(),
        TerminalAttachOptions {
            mode: TerminalAttachMode::Viewer,
            takeover: false,
            since: Some(0),
            prefer_snapshot: true,
        },
        AttachmentRegistry::default(),
        history,
    )
    .expect("attachment");

    let initial = client.take_initial_output();
    assert_eq!(initial.kind, TerminalInitialOutputKind::Snapshot);
    assert_eq!(initial.start_cursor, 8);
    assert_eq!(initial.end_cursor, 8);
}

#[test]
fn unsafe_snapshot_state_falls_back_to_the_retained_raw_replay() {
    let history = SessionHistory::new(None).expect("history");
    let raw = b"before\x1bPqgraphics\x1b\\after";
    history.push(raw).expect("output");
    let mut client = TerminalAttachment::new(
        "g_attach".into(),
        "peer".into(),
        TerminalAttachOptions {
            mode: TerminalAttachMode::Viewer,
            takeover: false,
            since: None,
            prefer_snapshot: true,
        },
        AttachmentRegistry::default(),
        history,
    )
    .expect("attachment");

    let initial = client.take_initial_output();
    assert_eq!(initial.kind, TerminalInitialOutputKind::Replay);
    assert_eq!(initial.start_cursor, 0);
    assert_eq!(initial.end_cursor, raw.len() as u64);
    assert_eq!(initial.data, raw);
}

#[tokio::test]
async fn snapshot_and_live_output_are_contiguous_and_raw() {
    let history = SessionHistory::new(None).expect("history");
    history.push(&[b'a', 0xff]).expect("snapshot output");
    let registry = AttachmentRegistry::default();
    let mut client = attachment(&history, &registry, TerminalAttachMode::Viewer, false);
    history.push(&[0xfe, b'b']).expect("live output");

    let replay = client.take_replay();
    let live = client
        .next_output(64)
        .await
        .expect("next output")
        .expect("live page");
    assert_eq!(replay.data, [b'a', 0xff]);
    assert_eq!(live.data, [0xfe, b'b']);
    assert_eq!(replay.end_cursor, live.start_cursor);
}

#[tokio::test]
async fn slow_attachment_reports_an_expired_cursor() {
    let history = SessionHistory::new(Some(HistoryConfig {
        path: "unused.pty".into(),
        max_bytes: 4,
        hot_bytes: 4,
        persist: false,
    }))
    .expect("history");
    let registry = AttachmentRegistry::default();
    let mut client = attachment(&history, &registry, TerminalAttachMode::Viewer, false);
    client.take_replay();
    history.push(b"12345678").expect("output");

    assert!(matches!(
        client.next_output(64).await,
        Err(RuntimeError::OutputLagged {
            requested: 0,
            retained_start: 4,
        })
    ));
}

#[tokio::test]
async fn sealed_history_finishes_a_caught_up_attachment() {
    let history = SessionHistory::new(None).expect("history");
    let registry = AttachmentRegistry::default();
    let mut client = attachment(&history, &registry, TerminalAttachMode::Viewer, false);
    client.take_replay();
    history.seal_output().expect("seal");

    assert!(client.next_output(64).await.expect("next").is_none());
}

#[test]
fn writer_takeover_and_disconnect_promotion_match_python_semantics() {
    let history = SessionHistory::new(None).expect("history");
    let registry = AttachmentRegistry::default();
    let first = attachment(&history, &registry, TerminalAttachMode::Control, false);
    let second = attachment(&history, &registry, TerminalAttachMode::Control, false);
    let viewer = attachment(&history, &registry, TerminalAttachMode::Viewer, false);
    assert!(first.terminal_writable());
    assert!(!second.terminal_writable());
    assert!(!viewer.terminal_writable());

    let takeover = attachment(&history, &registry, TerminalAttachMode::Control, true);
    assert!(takeover.terminal_writable());
    assert!(takeover.writer_replaced());
    assert!(!registry.is_writer(first.attachment_id).expect("first"));
    assert!(
        registry
            .is_writer(takeover.attachment_id)
            .expect("takeover")
    );

    drop(takeover);
    assert!(registry.is_writer(first.attachment_id).expect("promoted"));
}
