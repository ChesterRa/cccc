use super::{
    authorize_transcript, render_tail, replay, snapshot, tail, tail_render_options, trailing_chars,
    write,
};
use cccc_contracts::{Actor, DaemonRequest, RunnerKind};
use cccc_core::{GroupStore, HomeLayout};
use cccc_runtime::LaunchSpec;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::time::Duration;

fn pty_group(home: &HomeLayout, actor_id: &str) -> String {
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("terminal test", "").expect("group");
    cccc_core::actors::add(&mut group, Actor::new(actor_id)).expect("actor");
    store.save(&group).expect("save group");
    group.group_id
}

#[test]
fn write_preserves_standalone_carriage_return() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group_id = format!("g_terminal_{}", uuid::Uuid::new_v4().simple());
    let actor_id = "peer1";
    cccc_runtime::start(LaunchSpec {
        group_id: group_id.clone(),
        actor_id: actor_id.into(),
        runner: RunnerKind::Pty,
        command: vec![
            "sh".into(),
            "-c".into(),
            "IFS= read -r line; printf 'received:<%s>' \"$line\"; sleep 1".into(),
        ],
        cwd: temp.path().into(),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    })
    .expect("start runtime");
    for data in ["/model", "\r"] {
        let request = DaemonRequest {
            v: 1,
            op: "terminal_write".into(),
            args: json!({"group_id":group_id,"actor_id":actor_id,"data":data})
                .as_object()
                .cloned()
                .expect("args"),
        };
        assert!(write(&home, &request).is_ok());
    }
    std::thread::sleep(Duration::from_millis(100));
    let output = cccc_runtime::history(&group_id, actor_id, None, 1024)
        .expect("history")
        .data;
    assert!(output.contains("received:</model>"), "{output:?}");
    let _ = cccc_runtime::stop(&group_id, actor_id);
}

#[test]
fn write_rejects_empty_data() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let request = DaemonRequest {
        v: 1,
        op: "terminal_write".into(),
        args: Map::from_iter([
            ("group_id".into(), Value::String("g1".into())),
            ("actor_id".into(), Value::String("peer1".into())),
            ("data".into(), Value::String(String::new())),
        ]),
    };
    assert_eq!(
        write(&home, &request)
            .expect_err("empty terminal data must be rejected")
            .code,
        "invalid_args"
    );
}

#[test]
fn terminal_tail_defaults_to_readable_rendering_for_non_web_callers() {
    let request = DaemonRequest {
        v: 1,
        op: "terminal_tail".into(),
        args: Map::new(),
    };
    assert_eq!(tail_render_options(&request), (true, true));
    let request = DaemonRequest {
        args: Map::from_iter([
            ("strip_ansi".into(), Value::Bool(false)),
            ("compact".into(), Value::Bool(false)),
        ]),
        ..request
    };
    assert_eq!(tail_render_options(&request), (false, false));
}

#[test]
fn tail_rendering_handles_unicode_and_ansi_before_truncation() {
    assert_eq!(trailing_chars("prefix你好世界", 4), "你好世界");
    assert_eq!(trailing_chars("short", 20), "short");
    let raw = "abcdefgh\u{1b}[6DXY";
    let expected = trailing_chars(&super::terminal_text::render(raw, false), 4);
    assert_eq!(render_tail(raw, 4, true, false), expected);
    assert_ne!(
        expected,
        super::terminal_text::render(&trailing_chars(raw, 4), false)
    );
}

#[test]
fn terminal_tail_uses_complete_hot_stream_and_preserves_raw_cursor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let actor_id = "tail-peer";
    let group_id = pty_group(&home, actor_id);
    cccc_runtime::start(LaunchSpec {
        group_id: group_id.clone(),
        actor_id: actor_id.into(),
        runner: RunnerKind::Pty,
        command: vec![
            "sh".into(),
            "-c".into(),
            "printf '\\033[1;1Habcdefgh\\033[1;1HXY'; sleep 2".into(),
        ],
        cwd: temp.path().into(),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    })
    .expect("start");
    let mut retained = cccc_runtime::retained_history(&group_id, actor_id).expect("history");
    for _ in 0..50 {
        if retained.data.contains("abcdefgh") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        retained = cccc_runtime::retained_history(&group_id, actor_id).expect("history");
    }
    let request = DaemonRequest {
        v: 1,
        op: "terminal_tail".into(),
        args: json!({
            "group_id": group_id, "actor_id": actor_id, "max_chars": 4,
            "strip_ansi": true, "compact": true,
        })
        .as_object()
        .cloned()
        .expect("args"),
    };
    let result = tail(&home, &request).expect("tail");
    let expected = trailing_chars(&super::terminal_text::render(&retained.data, true), 4);
    assert_eq!(result["text"].as_str(), Some(expected.as_str()));
    assert_eq!(result["end_cursor"].as_u64(), Some(retained.end_cursor));
    let _ = cccc_runtime::stop(&group_id, actor_id);
}

#[test]
fn terminal_snapshot_returns_a_rendered_screen_at_the_raw_end_cursor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let actor_id = "snapshot-peer";
    let group_id = pty_group(&home, actor_id);
    cccc_runtime::start(LaunchSpec {
        group_id: group_id.clone(),
        actor_id: actor_id.into(),
        runner: RunnerKind::Pty,
        command: vec![
            "sh".into(),
            "-c".into(),
            "printf 'old\\033[1;1H\\033[2Kcurrent'; sleep 2".into(),
        ],
        cwd: temp.path().into(),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    })
    .expect("start");
    std::thread::sleep(Duration::from_millis(100));
    let raw = cccc_runtime::retained_history(&group_id, actor_id).expect("raw");
    let request = DaemonRequest {
        v: 1,
        op: "terminal_snapshot".into(),
        args: json!({"group_id":group_id,"actor_id":actor_id})
            .as_object()
            .cloned()
            .expect("args"),
    };

    let result = snapshot(&home, &request).expect("snapshot");

    assert!(
        result["data"]
            .as_str()
            .unwrap_or_default()
            .ends_with("current")
    );
    assert_eq!(result["end_cursor"].as_u64(), Some(raw.end_cursor));
    assert!(result.get("history").is_none());
    let _ = cccc_runtime::stop(&group_id, actor_id);
}

#[test]
fn terminal_replay_preserves_current_session_raw_ansi() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let actor_id = "history-peer";
    let group_id = pty_group(&home, actor_id);
    cccc_runtime::start(LaunchSpec {
        group_id: group_id.clone(),
        actor_id: actor_id.into(),
        runner: RunnerKind::Pty,
        command: vec![
            "sh".into(),
            "-c".into(),
            "printf 'old conversation\\r\\n\\033[2J\\033[Hcurrent screen'; sleep 2".into(),
        ],
        cwd: temp.path().into(),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    })
    .expect("start");
    for _ in 0..50 {
        let retained = cccc_runtime::retained_history(&group_id, actor_id).expect("history");
        if retained.data.contains("current screen") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let request = DaemonRequest {
        v: 1,
        op: "terminal_replay".into(),
        args: json!({
            "group_id": group_id,
            "actor_id": actor_id,
            "limit_bytes": 512 * 1024,
            "strip_ansi": false,
        })
        .as_object()
        .cloned()
        .expect("args"),
    };

    let result = replay(&home, &request).expect("raw terminal replay");
    let raw = result["history"]["data"].as_str().unwrap_or_default();

    assert!(raw.contains("old conversation"), "raw history was {raw:?}");
    assert!(raw.contains("\u{1b}[2J"), "raw history was {raw:?}");
    assert!(raw.contains("current screen"), "raw history was {raw:?}");
    assert_eq!(result["history"]["start_cursor"], 0);
    assert_eq!(result["history"]["cursor_expired"], false);
    let _ = cccc_runtime::stop(&group_id, actor_id);
}

#[test]
fn peer_cannot_read_another_actors_terminal_with_default_visibility() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("terminal access", "").expect("group");
    cccc_core::actors::add(&mut group, Actor::new("lead")).expect("lead");
    cccc_core::actors::add(&mut group, Actor::new("peer")).expect("peer");
    store.save(&group).expect("save group");
    cccc_runtime::start(LaunchSpec {
        group_id: group.group_id.clone(),
        actor_id: "lead".into(),
        runner: RunnerKind::Pty,
        command: vec!["sh".into(), "-c".into(), "printf private; sleep 2".into()],
        cwd: temp.path().into(),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    })
    .expect("start");

    let request = DaemonRequest {
        v: 1,
        op: "terminal_tail".into(),
        args: json!({
            "group_id":group.group_id,"actor_id":"lead","by":"peer","strip_ansi":false
        })
        .as_object()
        .cloned()
        .expect("args"),
    };
    let error = tail(&home, &request).expect_err("peer transcript access must be denied");

    assert_eq!(error.code, "permission_denied");
    let _ = cccc_runtime::stop(&group.group_id, "lead");
}

#[test]
fn transcript_visibility_allows_self_foreman_and_explicit_all() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("terminal policy", "").expect("group");
    cccc_core::actors::add(&mut group, Actor::new("lead")).expect("lead");
    cccc_core::actors::add(&mut group, Actor::new("peer")).expect("peer");
    store.save(&group).expect("save group");
    let group_id = group.group_id.clone();
    let request = |by: &str| DaemonRequest {
        v: 1,
        op: "terminal_tail".into(),
        args: json!({"group_id":group_id.clone(),"actor_id":"peer","by":by})
            .as_object()
            .cloned()
            .expect("args"),
    };

    assert!(authorize_transcript(&home, &request("peer"), &group_id, "peer").is_ok());
    assert!(authorize_transcript(&home, &request("lead"), &group_id, "peer").is_ok());

    group.extra.insert(
        "settings".into(),
        json!({"terminal_transcript_visibility":"all"}),
    );
    store.save(&group).expect("save all visibility");
    let peer_reads_lead = DaemonRequest {
        args: json!({"group_id":group_id.clone(),"actor_id":"lead","by":"peer"})
            .as_object()
            .cloned()
            .expect("args"),
        ..request("peer")
    };
    assert!(authorize_transcript(&home, &peer_reads_lead, &group_id, "lead").is_ok());
}
