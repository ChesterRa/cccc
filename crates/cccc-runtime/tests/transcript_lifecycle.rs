#![cfg(unix)]

use cccc_contracts::RunnerKind;
use cccc_runtime::{HistoryConfig, LaunchSpec};
use std::collections::BTreeMap;
use std::time::Duration;

fn launch(temp: &tempfile::TempDir, group_id: &str, actor_id: &str) -> HistoryConfig {
    let history = HistoryConfig {
        path: temp.path().join("terminal").join("session.pty"),
        max_bytes: 1024 * 1024,
        hot_bytes: 32,
    };
    cccc_runtime::start_with_history(
        LaunchSpec {
            group_id: group_id.into(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: vec![
                "sh".into(),
                "-c".into(),
                "printf 'durable history marker'; sleep 5".into(),
            ],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        },
        history.clone(),
    )
    .expect("start");
    history
}

#[test]
fn stopped_session_history_remains_queryable_and_persisted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_id = format!("g_{}", std::process::id());
    let actor_id = "durable-peer";
    let history = launch(&temp, &group_id, actor_id);

    let mut live = String::new();
    for _ in 0..100 {
        live = cccc_runtime::history(&group_id, actor_id, None, 1024)
            .expect("live history")
            .data;
        if live.contains("durable history marker") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(live.contains("durable history marker"));

    cccc_runtime::stop(&group_id, actor_id).expect("stop");
    let stopped =
        cccc_runtime::history(&group_id, actor_id, None, 1024).expect("completed history");
    assert!(stopped.data.contains("durable history marker"));

    let persisted =
        cccc_runtime::read_latest_page(history.path.parent().expect("actor dir"), None, 1024)
            .expect("persisted history");
    assert!(persisted.data.contains("durable history marker"));
}

#[test]
fn hot_snapshot_is_bounded_while_disk_history_keeps_the_full_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_id = format!("g_hot_{}", std::process::id());
    let actor_id = "hot-peer";
    let history = HistoryConfig {
        path: temp.path().join("terminal").join("hot.pty"),
        max_bytes: 1024 * 1024,
        hot_bytes: 16,
    };
    cccc_runtime::start_with_history(
        LaunchSpec {
            group_id: group_id.clone(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: vec![
                "sh".into(),
                "-c".into(),
                "printf '0123456789abcdefghijklmnopqrstuvwxyz'; sleep 5".into(),
            ],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        },
        history,
    )
    .expect("start");
    std::thread::sleep(Duration::from_millis(100));

    let hot = cccc_runtime::retained_history(&group_id, actor_id).expect("hot");
    let full = cccc_runtime::history(&group_id, actor_id, None, 1024).expect("full");
    assert!(hot.data.len() <= 16);
    assert!(full.data.contains("0123456789abcdefghijklmnopqrstuvwxyz"));
    cccc_runtime::stop(&group_id, actor_id).expect("stop");
}

#[test]
fn restarted_actor_keeps_prior_history_and_continuous_cursors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_id = format!("g_restart_history_{}", std::process::id());
    let actor_id = "restart-peer";
    let actor_dir = temp.path().join("terminal");
    let start = |name: &str, marker: &str| {
        cccc_runtime::start_with_history(
            LaunchSpec {
                group_id: group_id.clone(),
                actor_id: actor_id.into(),
                runner: RunnerKind::Pty,
                command: vec![
                    "sh".into(),
                    "-c".into(),
                    format!("printf '{marker}'; sleep 5"),
                ],
                cwd: temp.path().into(),
                env: BTreeMap::new(),
                cols: 80,
                rows: 24,
            },
            HistoryConfig {
                path: actor_dir.join(format!("{name}.pty")),
                max_bytes: 1024 * 1024,
                hot_bytes: 1024,
            },
        )
        .expect("start")
    };

    start("first", "first");
    std::thread::sleep(Duration::from_millis(100));
    cccc_runtime::stop(&group_id, actor_id).expect("stop first");

    start("second", " second");
    let mut page = cccc_runtime::history(&group_id, actor_id, None, 1024).expect("history");
    for _ in 0..100 {
        if page.data.contains("first second") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        page = cccc_runtime::history(&group_id, actor_id, None, 1024).expect("history");
    }
    assert_eq!(page.data, "first second");
    assert_eq!(page.start_cursor, 0);
    assert_eq!(page.end_cursor, 12);

    let hot = cccc_runtime::retained_history(&group_id, actor_id).expect("hot");
    assert_eq!(hot.data, " second");
    assert_eq!(hot.start_cursor, 5);
    assert_eq!(hot.end_cursor, 12);
    cccc_runtime::stop(&group_id, actor_id).expect("stop second");
}
