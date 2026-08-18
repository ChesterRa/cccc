use super::*;
use cccc_core::GroupStore;
use serde_json::{Map, Value, json};
use std::io::Write;

#[test]
fn pending_wal_rebuilds_marker_without_duplicate_event() {
    use sha2::Digest;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("deepseek wal", "").expect("group");
    let key = "deepseek.update:event-1:0";
    crate::ops::local_headless::append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("event-1"))]),
        Some(key),
    )
    .expect("initial event");
    let state = store.state_dir(&group.group_id).expect("state");
    let events = state.join("headless/events.jsonl");
    let marker_dir = state.join("headless/events.dedupe");
    let mut digest = sha2::Sha256::new();
    digest.update(key.as_bytes());
    let marker = marker_dir.join(format!("{:x}.marker", digest.finalize()));
    let event: Value = serde_json::from_str(
        std::fs::read_to_string(&events)
            .expect("events")
            .lines()
            .next()
            .expect("event line"),
    )
    .expect("event json");
    std::fs::remove_file(marker).expect("simulate missing marker");
    std::fs::write(
        marker_dir.join("pending.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "key": key,
            "event_id": event["id"],
            "offset": 0,
            "line_len": std::fs::read_to_string(&events)
                .expect("events")
                .lines()
                .next()
                .expect("event line")
                .len(),
            "line": std::fs::read_to_string(&events)
                .expect("events")
                .lines()
                .next()
                .expect("event line"),
            "event": event
        }))
        .expect("pending"),
    )
    .expect("pending write");
    crate::ops::local_headless::append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("event-1"))]),
        Some(key),
    )
    .expect("recover pending");
    assert_eq!(
        std::fs::read_to_string(events)
            .expect("events")
            .lines()
            .count(),
        1
    );
}

#[test]
fn pending_wal_recovers_at_offset_after_ready_log_growth() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("deepseek wal large", "").expect("group");
    let key0 = "deepseek.update:event-0:0";
    crate::ops::local_headless::append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("event-0"))]),
        Some(key0),
    )
    .expect("initial event");

    let state = store.state_dir(&group.group_id).expect("state");
    let events = state.join("headless/events.jsonl");
    let marker_dir = state.join("headless/events.dedupe");
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&events)
            .expect("events");
        for _ in 0..30_000 {
            file.write_all(b"{\"filler\":1}\n").expect("filler");
        }
        file.sync_all().expect("filler sync");
    }

    let key = "deepseek.update:event-2:0";
    let event = json!({
        "id": "event-2-output",
        "ts": "2026-01-01T00:00:00Z",
        "group_id": group.group_id.clone(),
        "actor_id": "deepseek",
        "type": "headless.message.delta",
        "dedupe_key": key,
        "data": {"event_id": "event-2"}
    });
    let line = serde_json::to_vec(&event).expect("line");
    let offset = std::fs::metadata(&events).expect("metadata").len();
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&events)
            .expect("events");
        file.write_all(&line).expect("event");
        file.write_all(b"\n").expect("newline");
        file.sync_all().expect("event sync");
    }
    std::fs::write(
        marker_dir.join("pending.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "key": key,
            "event_id": event["id"],
            "offset": offset,
            "line_len": line.len(),
            "line": String::from_utf8(line).expect("utf8"),
            "event": event
        }))
        .expect("pending"),
    )
    .expect("pending write");

    crate::ops::local_headless::append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("event-2"))]),
        Some(key),
    )
    .expect("recover large pending");
    let matching = std::fs::read_to_string(events)
        .expect("events")
        .lines()
        .filter(|line| line.contains("deepseek.update:event-2:0"))
        .count();
    assert_eq!(matching, 1);
    assert!(!marker_dir.join("pending.json").exists());
}

#[test]
fn pending_wal_is_recovered_before_non_dedupe_writer() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("deepseek writer recovery", "").expect("group");
    let state = store.state_dir(&group.group_id).expect("state");
    let events = state.join("headless/events.jsonl");
    let marker_dir = state.join("headless/events.dedupe");
    std::fs::create_dir_all(&marker_dir).expect("marker dir");
    std::fs::File::create(&events).expect("events");
    let key = "deepseek.update:event-reserved:0";
    let event = json!({
        "id": "reserved-output",
        "ts": "2026-01-01T00:00:00Z",
        "group_id": group.group_id.clone(),
        "actor_id": "deepseek",
        "type": "headless.message.delta",
        "dedupe_key": key,
        "data": {"event_id": "event-reserved"}
    });
    let line = serde_json::to_vec(&event).expect("line");
    std::fs::write(
        marker_dir.join("pending.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "key": key,
            "event_id": event["id"],
            "offset": 0,
            "line_len": line.len(),
            "line": String::from_utf8(line).expect("utf8"),
            "event": event
        }))
        .expect("pending"),
    )
    .expect("pending write");

    crate::ops::local_headless::append_event(
        &home,
        &group.group_id,
        "deepseek",
        "headless.permission.responded",
        Map::from_iter([("event_id".into(), json!("permission-1"))]),
    )
    .expect("non-dedupe writer recovery");
    let lines = std::fs::read_to_string(events)
        .expect("events")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("event json"))
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["dedupe_key"], key);
    assert_eq!(lines[1]["type"], "headless.permission.responded");
    assert!(!marker_dir.join("pending.json").exists());
}

#[test]
fn invalid_marker_on_ready_large_log_fails_closed() {
    use sha2::Digest;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("deepseek invalid marker", "").expect("group");
    let key = "deepseek.update:event-invalid:0";
    crate::ops::local_headless::append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("event-invalid"))]),
        Some(key),
    )
    .expect("initial event");
    let state = store.state_dir(&group.group_id).expect("state");
    let events = state.join("headless/events.jsonl");
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&events)
            .expect("events");
        for _ in 0..30_000 {
            file.write_all(b"{\"filler\":1}\n").expect("filler");
        }
        file.sync_all().expect("filler sync");
    }
    let mut digest = sha2::Sha256::new();
    digest.update(key.as_bytes());
    let marker = state
        .join("headless/events.dedupe")
        .join(format!("{:x}.marker", digest.finalize()));
    std::fs::write(&marker, b"corrupt\n").expect("corrupt marker");
    let error = crate::ops::local_headless::append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("event-invalid"))]),
        Some(key),
    )
    .expect_err("invalid marker must fail closed");
    assert!(error.to_string().contains("marker is invalid"));
    let matching = std::fs::read_to_string(events)
        .expect("events")
        .lines()
        .filter(|line| line.contains(key))
        .count();
    assert_eq!(matching, 1);
}
