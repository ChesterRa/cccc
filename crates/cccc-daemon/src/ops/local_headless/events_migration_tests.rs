use super::append_event_with_dedupe;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, json};
use std::io::Write;

#[test]
fn large_legacy_log_builds_dedupe_index_without_loading_the_whole_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store
        .create("large legacy headless log", "")
        .expect("group");
    let directory = store
        .state_dir(&group.group_id)
        .expect("state")
        .join("headless");
    std::fs::create_dir_all(&directory).expect("headless dir");
    let events = directory.join("events.jsonl");
    let mut file = std::fs::File::create(&events).expect("events");
    for index in 0..5_000 {
        let dedupe_key = (index == 2_500).then_some("legacy-key");
        writeln!(
            file,
            "{}",
            json!({
                "id":format!("legacy-{index}"),
                "group_id":group.group_id,
                "actor_id":"codex",
                "type":"headless.message.delta",
                "dedupe_key":dedupe_key,
                "data":{"delta":"x".repeat(64)}
            })
        )
        .expect("legacy event");
    }
    file.sync_all().expect("sync legacy log");
    let original_lines = std::fs::read_to_string(&events)
        .expect("events")
        .lines()
        .count();
    assert!(std::fs::metadata(&events).expect("metadata").len() > 256 * 1024);
    assert!(original_lines > 4_096);

    append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("legacy-source"))]),
        Some("legacy-key"),
    )
    .expect("migrate and dedupe legacy key");
    assert_eq!(
        std::fs::read_to_string(&events)
            .expect("events")
            .lines()
            .count(),
        original_lines
    );
    assert!(directory.join("events.dedupe/index.ready").exists());

    append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("new-source"))]),
        Some("new-key"),
    )
    .expect("append after migration");
    assert_eq!(
        std::fs::read_to_string(events)
            .expect("events")
            .lines()
            .count(),
        original_lines + 1
    );
}

#[test]
fn oversized_legacy_event_without_dedupe_identity_does_not_block_new_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("oversized legacy event", "").expect("group");
    let directory = store
        .state_dir(&group.group_id)
        .expect("state")
        .join("headless");
    std::fs::create_dir_all(&directory).expect("headless dir");
    let events = directory.join("events.jsonl");
    let mut file = std::fs::File::create(&events).expect("events");
    writeln!(
        file,
        "{}",
        json!({
            "id":"legacy-large",
            "group_id":group.group_id,
            "actor_id":"codex",
            "type":"headless.item.completed",
            "data":{"item":"x".repeat(1024 * 1024 + 32)}
        })
    )
    .expect("legacy event");
    file.sync_all().expect("sync legacy log");

    append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.turn.started",
        Map::from_iter([("event_id".into(), json!("new-source"))]),
        Some("deepseek.turn.started:new-source"),
    )
    .expect("oversized unrelated legacy event must be skipped");

    assert_eq!(
        std::fs::read_to_string(events)
            .expect("events")
            .lines()
            .count(),
        2
    );
    assert!(directory.join("events.dedupe/index.ready").exists());
}

#[test]
fn oversized_event_claiming_dedupe_identity_still_fails_closed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("oversized dedupe event", "").expect("group");
    let directory = store
        .state_dir(&group.group_id)
        .expect("state")
        .join("headless");
    std::fs::create_dir_all(&directory).expect("headless dir");
    let events = directory.join("events.jsonl");
    let mut file = std::fs::File::create(&events).expect("events");
    writeln!(
        file,
        "{}",
        json!({
            "id":"legacy-large",
            "group_id":group.group_id,
            "actor_id":"deepseek",
            "type":"headless.message.delta",
            "dedupe_key":"legacy-key",
            "data":{"delta":"x".repeat(1024 * 1024 + 32)}
        })
    )
    .expect("legacy event");
    file.sync_all().expect("sync legacy log");

    let error = append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("legacy-source"))]),
        Some("legacy-key"),
    )
    .expect_err("oversized dedupe-bearing event must fail closed");

    assert!(error.to_string().contains("has dedupe identity"));
    assert!(!directory.join("events.dedupe/index.ready").exists());
}
