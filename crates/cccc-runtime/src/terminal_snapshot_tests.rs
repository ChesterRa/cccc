use super::TerminalStateMirror;
use retach::screen::Screen;

fn visible_text(screen: &Screen) -> String {
    screen
        .visible_rows()
        .map(|row| row.iter().map(|cell| cell.c).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn snapshot_rebuilds_the_current_screen_at_an_exact_raw_cursor() {
    let mut mirror = TerminalStateMirror::new(12, 3);
    mirror.process(b"old line\r\nsecond\r\n\x1b[2J\x1b[Hlatest");

    let snapshot = mirror.snapshot(42).expect("snapshot");
    let mut restored = Screen::new(snapshot.cols, snapshot.rows, 512);
    restored.process(&snapshot.data);

    assert_eq!(snapshot.cursor, 42);
    assert_eq!((snapshot.cols, snapshot.rows), (12, 3));
    assert!(visible_text(&restored).contains("latest"));
    assert!(!visible_text(&restored).contains("old line"));
}

#[test]
fn cursor_position_query_at_home_generates_a_response() {
    let mut mirror = TerminalStateMirror::new(80, 24);

    assert_eq!(mirror.process(b"\x1b[6n"), [b"\x1b[1;1R".to_vec()]);
}

#[test]
fn cursor_position_query_reports_the_mirrored_position() {
    let mut mirror = TerminalStateMirror::new(80, 24);

    assert_eq!(
        mirror.process(b"\x1b[5;10H\x1b[6n"),
        [b"\x1b[5;10R".to_vec()]
    );
}

#[test]
fn private_cursor_position_query_reports_the_mirrored_position() {
    let mut mirror = TerminalStateMirror::new(80, 24);

    assert_eq!(
        mirror.process(b"\x1b[5;10H\x1b[?6n"),
        [b"\x1b[?5;10R".to_vec()]
    );
}

#[test]
fn split_cursor_position_query_generates_one_response() {
    let mut mirror = TerminalStateMirror::new(80, 24);

    assert!(mirror.process(b"\x1b[6").is_empty());
    assert_eq!(mirror.process(b"n"), [b"\x1b[1;1R".to_vec()]);
    assert!(mirror.process(b"plain output").is_empty());
}

#[test]
fn terminal_queries_continue_after_snapshots_become_unsafe() {
    let mut mirror = TerminalStateMirror::new(80, 24);

    assert_eq!(
        mirror.process(b"\x1bPqgraphics\x1b\\\x1b[5;10H\x1b[6n"),
        [b"\x1b[5;10R".to_vec()]
    );
    assert!(mirror.snapshot(0).is_none());
}

#[test]
fn color_queries_are_answered_without_a_browser() {
    let mut mirror = TerminalStateMirror::new(80, 24);

    assert_eq!(
        mirror.process(b"\x1b]10;?;?\x1b\\"),
        [
            b"\x1b]10;rgb:e2e2/e8e8/f0f0\x1b\\".to_vec(),
            b"\x1b]11;rgb:0f0f/1717/2a2a\x1b\\".to_vec(),
        ]
    );
}

#[test]
fn mixed_terminal_queries_preserve_response_order() {
    let mut mirror = TerminalStateMirror::new(80, 24);

    assert_eq!(
        mirror.process(b"\x1b[6n\x1b]11;?\x1b\\\x1b[5n"),
        [
            b"\x1b[1;1R".to_vec(),
            b"\x1b]11;rgb:0f0f/1717/2a2a\x1b\\".to_vec(),
            b"\x1b[0n".to_vec(),
        ]
    );
}

#[test]
fn snapshot_injects_recent_lines_into_native_scrollback() {
    let mut mirror = TerminalStateMirror::new(12, 3);
    mirror.process(b"line one\r\nline two\r\nline three\r\nline four\r\nlatest");

    let snapshot = mirror.snapshot(55).expect("snapshot");
    let mut restored = Screen::new(snapshot.cols, snapshot.rows, 512);
    restored.process(&snapshot.data);
    let history = restored
        .get_history()
        .into_iter()
        .map(|line| String::from_utf8_lossy(&line).into_owned())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(history.contains("line one"));
    assert!(history.contains("line two"));
    assert!(visible_text(&restored).contains("latest"));
}

#[test]
fn snapshot_preserves_a_split_csi_sequence_for_the_live_suffix() {
    let prefix = b"base\x1b[31";
    let suffix = b"mRED\x1b[0m";
    let mut mirror = TerminalStateMirror::new(20, 2);
    mirror.process(prefix);
    let snapshot = mirror.snapshot(prefix.len() as u64).expect("snapshot");

    let mut restored = Screen::new(20, 2, 512);
    restored.process(&snapshot.data);
    restored.process(suffix);
    let mut expected = Screen::new(20, 2, 512);
    expected.process(prefix);
    expected.process(suffix);

    assert_eq!(visible_text(&restored), visible_text(&expected));
}

#[test]
fn snapshot_preserves_a_split_utf8_codepoint_for_the_live_suffix() {
    let encoded = "你".as_bytes();
    let mut mirror = TerminalStateMirror::new(10, 2);
    mirror.process(&encoded[..2]);
    let snapshot = mirror.snapshot(2).expect("snapshot");

    let mut restored = Screen::new(10, 2, 512);
    restored.process(&snapshot.data);
    restored.process(&encoded[2..]);

    assert!(visible_text(&restored).contains('你'));
}

#[test]
fn unsupported_graphics_fall_back_to_raw_replay() {
    let mut mirror = TerminalStateMirror::new(80, 24);
    mirror.process(b"\x1bPqgraphics\x1b\\");
    assert!(mirror.snapshot(12).is_none());
}

#[test]
fn resize_is_reflected_in_snapshot_metadata() {
    let mut mirror = TerminalStateMirror::new(80, 24);
    mirror.resize(132, 40);
    let snapshot = mirror.snapshot(0).expect("snapshot");
    assert_eq!((snapshot.cols, snapshot.rows), (132, 40));
}

#[test]
fn oversized_terminal_disables_the_mirror_instead_of_allocating_it() {
    let mirror = TerminalStateMirror::new(u16::MAX, u16::MAX);
    assert!(mirror.snapshot(0).is_none());

    let mut mirror = TerminalStateMirror::new(80, 24);
    mirror.resize(u16::MAX, u16::MAX);
    assert_eq!(mirror.size(), (u16::MAX, u16::MAX));
    assert!(mirror.snapshot(0).is_none());
}

#[test]
fn oversized_terminal_still_answers_all_server_owned_queries() {
    let mut mirror = TerminalStateMirror::new(u16::MAX, u16::MAX);
    assert_eq!(
        mirror.process(b"\x1b[6n\x1b[?6n\x1b]10;?\x1b\\"),
        [
            b"\x1b[1;1R".to_vec(),
            b"\x1b[?1;1R".to_vec(),
            b"\x1b]10;rgb:e2e2/e8e8/f0f0\x1b\\".to_vec(),
        ]
    );
}
