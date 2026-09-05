//! Read an older page, optionally expanding it through a fixed snapshot end.
use crate::dispatch::OpError;
use crate::ops::terminal_history_source;
use cccc_core::HomeLayout;
use cccc_runtime::HistoryPage;

const MAX_RENDER_BYTES: u64 = 50_000_000;

pub(super) fn read(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    before: Option<u64>,
    limit: usize,
    render_before: Option<u64>,
) -> Result<HistoryPage, OpError> {
    let older = terminal_history_source::page(home, group_id, actor_id, before, limit)
        .map_err(super::runtime_error)?;
    let Some(end) = render_before else {
        return Ok(older);
    };
    // Expiry can advance the retained start beyond a valid pinned end. That
    // is an exhausted snapshot, not a malformed range from the caller.
    let length = end.saturating_sub(older.start_cursor);
    if length > MAX_RENDER_BYTES {
        return Err(OpError::new(
            "invalid_args",
            "render_before must bound a retained history range of at most 50 MB",
        ));
    }
    if before.is_some_and(|cursor| cursor > end)
        || (end < older.end_cursor && !older.cursor_expired)
    {
        return Err(OpError::new(
            "invalid_args",
            "render_before precedes the requested page end",
        ));
    }
    let mut cumulative =
        terminal_history_source::page(home, group_id, actor_id, Some(end), length as usize)
            .map_err(super::runtime_error)?;
    cumulative.cursor_expired |= older.cursor_expired;
    // Both memory and durable readers return an empty expired page at the
    // retained start. Pin its cursors too, including expiry between the reads.
    cumulative.start_cursor = cumulative.start_cursor.min(end);
    cumulative.end_cursor = cumulative.end_cursor.min(end);
    Ok(cumulative)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::{Actor, DaemonRequest};
    use cccc_core::GroupStore;
    use serde_json::json;

    fn fixture(raw: &str) -> (tempfile::TempDir, HomeLayout, String, std::path::PathBuf) {
        let temp = tempfile::tempdir().expect("create test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("create home layout");
        let store = GroupStore::new(home.clone()).expect("complete new in fixture");
        let mut group = store
            .create("history", "")
            .expect("complete create in fixture");
        cccc_core::actors::add(&mut group, Actor::new("peer")).expect("complete add in fixture");
        store.save(&group).expect("complete save in fixture");
        let dir =
            crate::ops::actor_runtime::terminal_history::actor_dir(&home, &group.group_id, "peer")
                .expect("complete actor dir in fixture");
        std::fs::create_dir_all(&dir).expect("create fixture directory");
        let file = dir.join("retained.pty");
        let mut bytes = b"CCCCPTY1".to_vec();
        bytes.extend(0_u64.to_le_bytes());
        bytes.extend(raw.as_bytes());
        std::fs::write(&file, bytes).expect("write test fixture");
        std::fs::write(dir.join("latest"), "retained.pty").expect("write test fixture");
        (temp, home, group.group_id, file)
    }

    #[test]
    fn cumulative_history_renders_newlines_and_split_ansi_once() {
        for (raw, split, expected) in [
            ("a\nb", 2, "a\nb"),
            ("a\u{1b}[31mb", 4, "ab"),
            ("old\rnew", 4, "old\n\nnew"),
            (
                "\u{1b}[1;1Hold frame\u{1b}[1;1H\u{1b}[2Knew frame",
                15,
                "old frame\n\nnew frame",
            ),
        ] {
            let (_temp, home, group, _) = fixture(raw);
            let request = DaemonRequest {
                v: 1,
                op: "terminal_history".into(),
                args: json!({
                    "group_id":group, "actor_id":"peer", "before":split,
                    "limit_bytes":split, "render_before":raw.len(), "strip_ansi":true,
                })
                .as_object()
                .expect("access object fixture")
                .clone(),
            };
            let result =
                super::super::history(&home, &request).expect("complete history in fixture");
            assert_eq!(result["text"], expected);
            assert_eq!(result["start_cursor"], 0);
            assert_eq!(result["end_cursor"], raw.len());
        }
    }

    #[test]
    fn loading_older_history_keeps_the_already_visible_newer_lines() {
        let raw = (0..6000).map(|n| format!("line {n}\n")).collect::<String>();
        let (_temp, home, group, _) = fixture(&raw);
        let recent =
            read(&home, &group, "peer", None, 16_000, None).expect("complete read in fixture");
        let recent_text = crate::ops::terminal_text::render_history(&recent.data, false);
        let all = read(
            &home,
            &group,
            "peer",
            Some(recent.start_cursor),
            raw.len(),
            Some(recent.end_cursor),
        )
        .expect("complete read in fixture");
        let text = crate::ops::terminal_text::render_history(&all.data, false);
        assert_eq!(text, raw.trim_end());
        // The first line of a byte page may be partial; all complete recent
        // lines must remain visible after extending and replacing the page.
        for line in recent_text.lines().skip(1) {
            assert!(text.lines().any(|candidate| candidate == line));
        }
    }

    #[test]
    fn expired_snapshot_stops_paging_without_reading_new_output() {
        let (_temp, home, group, file) = fixture("old output");
        let first = read(&home, &group, "peer", None, 4, None).expect("first page");
        // Retention or terminal clear advances past the browser's pinned range.
        let mut bytes = b"CCCCPTY1".to_vec();
        bytes.extend((first.end_cursor + 10).to_le_bytes());
        bytes.extend(b"new output outside the snapshot");
        std::fs::write(file, bytes).expect("advance retained history");
        let expired = read(
            &home,
            &group,
            "peer",
            Some(first.start_cursor),
            4,
            Some(first.end_cursor),
        )
        .expect("retention expiry is not an invalid request");
        assert!(expired.cursor_expired);
        assert!(!expired.has_more);
        assert!(expired.data.is_empty());
        assert_eq!(expired.end_cursor, first.end_cursor);
        assert!(expired.start_cursor >= first.start_cursor);
    }

    #[test]
    fn snapshot_excludes_new_output_and_rejects_invalid_ranges() {
        use std::io::Write;
        let (_temp, home, group, file) = fixture("a\nb");
        let mut output = std::fs::OpenOptions::new()
            .append(true)
            .open(file)
            .expect("complete open in fixture");
        output
            .write_all(b"\nnew output")
            .expect("write process input");
        let page =
            read(&home, &group, "peer", Some(2), 2, Some(3)).expect("complete read in fixture");
        assert_eq!(page.data, "a\nb");
        assert_eq!(page.end_cursor, 3);
        assert!(read(&home, &group, "peer", Some(2), 2, Some(1)).is_err());
        assert!(
            read(
                &home,
                &group,
                "peer",
                Some(2),
                2,
                Some(MAX_RENDER_BYTES + 1)
            )
            .is_err()
        );
    }
}
