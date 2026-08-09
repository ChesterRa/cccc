use super::OutputBuffer;

#[test]
fn history_uses_absolute_cursors() {
    let mut output = OutputBuffer::default();
    output.push(b"hello");
    output.push(b" world");
    let page = output.page(None, 5);
    assert_eq!(page.data, "world");
    assert_eq!(page.start_cursor, 6);
    assert_eq!(page.end_cursor, 11);
}

#[test]
fn history_can_continue_from_a_previous_session_cursor() {
    let mut output = OutputBuffer::with_capacity_at(1024, 42);
    output.push(b"next");
    let page = output.retained_page();
    assert_eq!(page.data, "next");
    assert_eq!(page.start_cursor, 42);
    assert_eq!(page.end_cursor, 46);
}

#[test]
fn retained_page_returns_the_complete_buffer() {
    let mut output = OutputBuffer::default();
    output.push("prefix 你好".as_bytes());
    output.push(b" suffix");
    let page = output.retained_page();
    assert_eq!(page.data, "prefix 你好 suffix");
    assert_eq!(page.start_cursor, 0);
    assert_eq!(page.end_cursor, "prefix 你好 suffix".len() as u64);
    assert!(!page.has_more);
    assert!(!page.cursor_expired);
}

#[test]
fn retained_tail_page_is_bounded_and_keeps_an_incomplete_suffix_for_later() {
    let mut output = OutputBuffer::default();
    output.push(b"prefix-");
    let encoded = "你".as_bytes();
    output.push(&encoded[..2]);

    let tail = output.retained_tail_page(4);

    assert_eq!(tail.data, "x-");
    assert_eq!(tail.start_cursor, 5);
    assert_eq!(tail.end_cursor, 7);
    assert!(tail.has_more);

    output.push(&encoded[2..]);
    let remainder = output.page_since(tail.end_cursor, 64);
    assert_eq!(remainder.data, "你");
    assert_eq!(remainder.end_cursor, 10);
}

#[test]
fn trimming_retained_output_keeps_the_latest_bytes_and_absolute_cursor() {
    let mut output = OutputBuffer::with_capacity_at(1024, 40);
    output.push(b"0123456789");

    output.trim_to(4);
    let retained = output.retained_page();

    assert_eq!(retained.data, "6789");
    assert_eq!(retained.start_cursor, 46);
    assert_eq!(retained.end_cursor, 50);
    assert_eq!(output.retained_bytes(), 4);
}

#[test]
fn history_since_extends_a_page_to_the_next_utf8_boundary() {
    let mut output = OutputBuffer::default();
    output.push("ab你cd".as_bytes());
    let page = output.page_since(0, 3);
    assert_eq!(page.data, "ab你");
    assert_eq!(page.start_cursor, 0);
    assert_eq!(page.end_cursor, 5);
    assert!(page.has_more);
}

#[test]
fn forward_pages_wait_for_an_incomplete_utf8_suffix() {
    let mut output = OutputBuffer::default();
    let encoded = "你".as_bytes();
    output.push(&encoded[..2]);
    let partial = output.retained_page();
    assert!(partial.data.is_empty());
    assert_eq!(partial.end_cursor, 0);
    assert!(partial.has_more);
    output.push(&encoded[2..]);
    let completed = output.page_since(partial.end_cursor, 64);
    assert_eq!(completed.data, "你");
    assert_eq!(completed.end_cursor, 3);
    assert!(!completed.has_more);
}

#[test]
fn history_since_only_returns_new_output() {
    let mut output = OutputBuffer::default();
    output.push(b"hello");
    let first = output.page_since(u64::MAX, 1024);
    assert!(first.data.is_empty());
    assert_eq!(first.end_cursor, 5);
    output.push(b" world");
    let next = output.page_since(first.end_cursor, 1024);
    assert_eq!(next.data, " world");
    assert_eq!(next.start_cursor, 5);
    assert_eq!(next.end_cursor, 11);
    assert!(!next.has_more);
}

#[test]
fn bounded_forward_pages_ignore_output_after_the_snapshot_end() {
    let mut output = OutputBuffer::default();
    output.push(b"snapshot");
    let snapshot_end = output.retained_page().end_cursor;
    output.push(b"-live-output");

    let first = output.page_since_until(0, snapshot_end, 4);
    assert_eq!(first.data, "snap");
    assert!(first.has_more);

    let second = output.page_since_until(first.end_cursor, snapshot_end, 64);
    assert_eq!(second.data, "shot");
    assert_eq!(second.end_cursor, snapshot_end);
    assert!(!second.has_more);
}

#[test]
fn bounded_forward_pages_stop_before_an_incomplete_utf8_suffix() {
    let mut output = OutputBuffer::default();
    output.push(b"ready ");
    let encoded = "你".as_bytes();
    output.push(&encoded[..2]);
    let snapshot_end = output.retained_page().end_cursor;
    assert_eq!(snapshot_end, 6);

    let page = output.page_since_until(0, snapshot_end, 64);
    assert_eq!(page.data, "ready ");
    assert_eq!(page.end_cursor, snapshot_end);
    assert!(!page.has_more);

    output.push(&encoded[2..]);
    assert_eq!(output.page_since(snapshot_end, 64).data, "你");
}

#[test]
fn tracks_bracketed_paste_across_output_chunks() {
    let mut output = OutputBuffer::default();
    output.push(b"\x1b[?20");
    output.push(b"04hready");
    assert!(output.bracketed_paste_enabled());
    output.push(b"\x1b[?2004l");
    assert!(!output.bracketed_paste_enabled());
}

#[test]
fn partial_utf8_pages_preserve_cursor_byte_length() {
    let mut output = OutputBuffer::default();
    output.push("你".as_bytes());
    let page = output.page(None, 2);
    assert_eq!(page.data, "??");
    assert_eq!(page.data.len() as u64, page.end_cursor - page.start_cursor);
}
