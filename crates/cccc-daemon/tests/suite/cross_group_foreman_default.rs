use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value, json};

#[test]
fn cross_group_default_targets_the_unique_available_foreman() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let source = create_group(&home, "source");
    let destination = create_group(&home, "destination");
    add_actor(&home, &destination, "lead");
    add_actor(&home, &destination, "peer");

    let sent = call(
        &home,
        "send_cross_group",
        json!({
            "group_id":source,"dst_group_id":destination,"by":"user","text":"hello",
            "message_mode":"send"
        }),
    );

    assert!(sent.ok, "{:?}", sent.error);
    assert_eq!(sent.result["src_event"]["data"]["to"], json!(["user"]));
    assert_eq!(sent.result["src_event"]["data"]["dst_to"], json!(["lead"]));
    assert_eq!(sent.result["dst_event"]["data"]["to"], json!(["lead"]));
}

#[test]
fn explicit_cross_group_recipient_overrides_the_default() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let source = create_group(&home, "source");
    let destination = create_group(&home, "destination");
    add_actor(&home, &destination, "lead");
    add_actor(&home, &destination, "peer");

    let sent = call(
        &home,
        "send_cross_group",
        json!({
            "group_id":source,"dst_group_id":destination,"by":"user",
            "to":["@peer"],"text":"hello","message_mode":"send"
        }),
    );

    assert!(sent.ok, "{:?}", sent.error);
    assert_eq!(sent.result["src_event"]["data"]["to"], json!(["user"]));
    assert_eq!(sent.result["src_event"]["data"]["dst_to"], json!(["peer"]));
    assert_eq!(sent.result["dst_event"]["data"]["to"], json!(["peer"]));
}

#[test]
fn unavailable_or_non_unique_foreman_fails_before_either_ledger_write() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let source = create_group(&home, "source");
    let destination = create_group(&home, "destination");
    add_actor(&home, &destination, "lead");
    let store = GroupStore::new(home.clone()).expect("store");
    let source_ledger = store.ledger_path(&source).expect("source ledger");
    let destination_ledger = store.ledger_path(&destination).expect("destination ledger");

    let mut group = store.load(&destination).expect("destination");
    group.actors[0].enabled = false;
    store.save(&group).expect("disable foreman");
    let before_source = ledger::read_all(&source_ledger)
        .expect("source events")
        .len();
    let before_destination = ledger::read_all(&destination_ledger)
        .expect("destination events")
        .len();
    let missing = call(
        &home,
        "send_cross_group",
        json!({
            "group_id":source,"dst_group_id":destination,"by":"user","text":"hello",
            "message_mode":"send"
        }),
    );
    assert_eq!(
        missing.error.as_ref().map(|error| error.code.as_str()),
        Some("foreman_not_found")
    );

    let mut group = store.load(&destination).expect("destination");
    group.actors[0].enabled = true;
    group.actors.push(group.actors[0].clone());
    store.save(&group).expect("duplicate malformed foreman");
    let ambiguous = call(
        &home,
        "send_cross_group",
        json!({
            "group_id":source,"dst_group_id":destination,"by":"user","text":"hello",
            "message_mode":"send"
        }),
    );
    assert_eq!(
        ambiguous.error.as_ref().map(|error| error.code.as_str()),
        Some("foreman_not_unique")
    );
    assert_eq!(
        ledger::read_all(&source_ledger)
            .expect("source events")
            .len(),
        before_source
    );
    assert_eq!(
        ledger::read_all(&destination_ledger)
            .expect("destination events")
            .len(),
        before_destination
    );
}

#[test]
fn same_group_empty_recipient_materializes_the_foreman_default() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = create_group(&home, "local");
    add_actor(&home, &group, "lead");
    add_actor(&home, &group, "peer");

    let sent = call(
        &home,
        "send",
        json!({"group_id":group,"by":"peer","to":[],"text":"hello","message_mode":"send"}),
    );

    assert!(sent.ok, "{:?}", sent.error);
    assert_eq!(sent.result["event"]["data"]["to"], json!(["@foreman"]));
}

fn create_group(home: &HomeLayout, title: &str) -> String {
    call(home, "group_create", json!({"title":title,"by":"user"})).result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned()
}

fn add_actor(home: &HomeLayout, group_id: &str, actor_id: &str) {
    let response = call(
        home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
    );
    assert!(response.ok, "{:?}", response.error);
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
