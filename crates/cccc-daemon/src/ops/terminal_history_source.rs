use cccc_core::HomeLayout;
use cccc_runtime::{HistoryPage, RuntimeError};

use super::actor_runtime::terminal_history;

const MAX_RETAINED_HISTORY_BYTES: usize = 50_000_000;

fn empty_history() -> HistoryPage {
    HistoryPage {
        data: String::new(),
        start_cursor: 0,
        end_cursor: 0,
        has_more: false,
        cursor_expired: false,
    }
}

fn absent_history_is_empty(
    result: Result<HistoryPage, RuntimeError>,
) -> Result<HistoryPage, RuntimeError> {
    match result {
        Err(RuntimeError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(empty_history())
        }
        result => result,
    }
}

pub(super) fn page(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    before: Option<u64>,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    match cccc_runtime::history(group_id, actor_id, before, limit) {
        Err(RuntimeError::NotFound(_, _)) => {
            absent_history_is_empty(cccc_runtime::read_latest_page(
                &terminal_history::actor_dir(home, group_id, actor_id)?,
                before,
                limit,
            ))
        }
        result => result,
    }
}

pub(super) fn retained(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    match cccc_runtime::retained_history_tail(group_id, actor_id, limit) {
        Err(RuntimeError::NotFound(_, _)) => {
            absent_history_is_empty(cccc_runtime::read_latest_page(
                &terminal_history::actor_dir(home, group_id, actor_id)?,
                None,
                limit,
            ))
        }
        result => result,
    }
}

pub(super) fn retained_full(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> Result<HistoryPage, RuntimeError> {
    match cccc_runtime::retained_history(group_id, actor_id) {
        Err(RuntimeError::NotFound(_, _)) => {
            absent_history_is_empty(cccc_runtime::read_latest_page(
                &terminal_history::actor_dir(home, group_id, actor_id)?,
                None,
                MAX_RETAINED_HISTORY_BYTES,
            ))
        }
        result => result,
    }
}

pub(super) fn since(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    after: u64,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    match cccc_runtime::history_since(group_id, actor_id, after, limit) {
        Err(RuntimeError::NotFound(_, _)) => {
            absent_history_is_empty(cccc_runtime::read_latest_since(
                &terminal_history::actor_dir(home, group_id, actor_id)?,
                after,
                limit,
            ))
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::GroupStore;
    use std::io::Write;

    #[test]
    fn reads_the_latest_persisted_session_without_a_live_runtime() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("history", "").expect("group");
        let actor_dir =
            terminal_history::actor_dir(&home, &group.group_id, "peer-1").expect("actor dir");
        std::fs::create_dir_all(&actor_dir).expect("create actor dir");
        let path = actor_dir.join("persisted.pty");
        let mut file = std::fs::File::create(&path).expect("file");
        file.write_all(b"CCCCPTY1").expect("magic");
        file.write_all(&42_u64.to_le_bytes()).expect("cursor");
        file.write_all(b"persisted output").expect("data");
        std::fs::write(actor_dir.join("latest"), b"persisted.pty").expect("latest");

        let page = page(&home, &group.group_id, "peer-1", None, 1024).expect("history");

        assert_eq!(page.data, "persisted output");
        assert_eq!(page.start_cursor, 42);
        assert_eq!(page.end_cursor, 58);
    }
}
