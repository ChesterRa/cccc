#![cfg(unix)]

use cccc_contracts::RunnerKind;
use cccc_runtime::{HistoryConfig, LaunchSpec, RuntimeError};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn active_history_replays_raw_ansi_in_pages_and_excludes_completed_sessions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let group_id = format!("g_replay_{}_{}", std::process::id(), unique);
    let actor_id = "replay-peer";
    cccc_runtime::start_with_history(
        LaunchSpec {
            group_id: group_id.clone(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: old_session_command(),
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        },
        HistoryConfig {
            path: temp.path().join("terminal").join("old.pty"),
            max_bytes: 1024 * 1024,
            hot_bytes: 1024 * 1024,
            persist: true,
        },
    )
    .expect("start old session");
    wait_for_marker(&group_id, actor_id, "old persisted session");
    let old_end = cccc_runtime::retained_history(&group_id, actor_id)
        .expect("old history")
        .end_cursor;
    cccc_runtime::stop(&group_id, actor_id).expect("stop old session");

    cccc_runtime::start_with_history(
        LaunchSpec {
            group_id: group_id.clone(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: replay_command(),
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        },
        HistoryConfig {
            path: temp.path().join("terminal").join("replay.pty"),
            max_bytes: 1024 * 1024,
            hot_bytes: 1024 * 1024,
            persist: true,
        },
    )
    .expect("start");

    wait_for_marker(&group_id, actor_id, "current screen");

    let mut cursor = 0;
    let mut replayed = String::new();
    loop {
        let page = cccc_runtime::active_history_since(&group_id, actor_id, cursor, 12)
            .expect("active history page");
        assert!(page.end_cursor >= cursor);
        replayed.push_str(&page.data);
        cursor = page.end_cursor;
        if !page.has_more {
            break;
        }
    }

    assert!(replayed.contains("old conversation"), "{replayed:?}");
    assert!(replayed.contains("\u{1b}[2J\u{1b}[H"), "{replayed:?}");
    assert!(replayed.contains("current screen"), "{replayed:?}");
    assert!(!replayed.contains("old persisted session"), "{replayed:?}");
    assert!(cursor > old_end);

    cccc_runtime::stop(&group_id, actor_id).expect("stop");
    assert!(matches!(
        cccc_runtime::active_history_since(&group_id, actor_id, 0, 1024),
        Err(RuntimeError::NotFound(_, _))
    ));
}

fn wait_for_marker(group_id: &str, actor_id: &str, marker: &str) {
    for _ in 0..100 {
        if cccc_runtime::retained_history(group_id, actor_id)
            .is_ok_and(|page| page.data.contains(marker))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("terminal output did not contain {marker:?}");
}

fn replay_command() -> Vec<String> {
    vec![
        "sh".into(),
        "-c".into(),
        r"printf 'old conversation\r\n\033[2J\033[Hcurrent screen'; sleep 5".into(),
    ]
}

fn old_session_command() -> Vec<String> {
    vec![
        "sh".into(),
        "-c".into(),
        "printf '%s' 'old persisted session'; sleep 5".into(),
    ]
}
