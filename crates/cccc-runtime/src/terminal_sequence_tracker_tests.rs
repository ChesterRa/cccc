use super::TerminalSequenceTracker;

#[test]
fn retains_only_an_incomplete_terminal_suffix() {
    let mut tracker = TerminalSequenceTracker::default();
    tracker.process(b"text\x1b[31");
    assert_eq!(tracker.pending(), b"\x1b[31");

    tracker.process(b"mred");
    assert!(tracker.pending().is_empty());

    let encoded = "你".as_bytes();
    tracker.process(&encoded[..2]);
    assert_eq!(tracker.pending(), &encoded[..2]);
    tracker.process(&encoded[2..]);
    assert!(tracker.pending().is_empty());
}

#[test]
fn unsupported_terminal_strings_disable_snapshots() {
    let mut tracker = TerminalSequenceTracker::default();
    tracker.process(b"before\x1bPqgraphics\x1b\\after");
    assert!(!tracker.snapshot_safe());
    assert!(tracker.pending().is_empty());
}

#[test]
fn recreates_kitty_keyboard_flags_and_stack() {
    let mut tracker = TerminalSequenceTracker::default();
    tracker.process(b"\x1b[=1;1u\x1b[>5u\x1b[=2;2u");
    assert_eq!(tracker.active_keyboard_restore(), b"\x1b[=1;1u\x1b[>7u");

    tracker.process(b"\x1b[<u");
    assert_eq!(tracker.active_keyboard_restore(), b"\x1b[=1;1u");
}

#[test]
fn tracks_keyboard_modes_separately_for_main_and_alternate_screens() {
    let mut tracker = TerminalSequenceTracker::default();
    tracker.process(b"\x1b[=1u\x1b[?1049h\x1b[=8u");

    assert!(tracker.alternate_screen());
    assert_eq!(tracker.main_keyboard_restore(), b"\x1b[=1;1u");
    assert_eq!(tracker.active_keyboard_restore(), b"\x1b[=8;1u");
}
