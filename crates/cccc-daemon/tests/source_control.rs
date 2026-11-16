//! Public dispatcher, persisted native metadata and real Group authorization.
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest};
use cccc_core::{GroupStore, HomeLayout, actors, local_sessions};
use serde_json::{Value, json};

#[path = "source_control/failures.rs"]
mod failures;

fn source(home: &HomeLayout, id: &str) -> local_sessions::NativeSession {
    let host = local_sessions::NativeHost::capture(std::process::id()).expect("owned host");
    let source = serde_json::from_value(json!({"source_id":format!("codex:{id}"),"runtime":"codex","native_session_id":id,"host":host,"cwd":home.root(),"title":id,"revision":format!("revision-{id}"),"codex_can_receive":true})).expect("native fixture");
    local_sessions::write(home, source).expect("registration")
}

fn call(home: &HomeLayout, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().expect("args").clone(),
        },
    )
}

#[test]
fn remove_fences_hooks_and_membership_and_restore_does_not_reopen_groups() {
    let temp = tempfile::tempdir().expect("isolated Home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("Home");
    home.initialize().expect("initialize");
    let host = local_sessions::NativeHost::capture(std::process::id()).expect("owned host");
    let source = |id: &str| {
        serde_json::from_value::<local_sessions::NativeSession>(json!({
        "source_id":format!("codex:{id}"),"runtime":"codex","native_session_id":id,
        "host":host,"cwd":temp.path(),"title":id,"revision":format!("revision-{id}"),"codex_can_receive":true
    })).expect("native fixture")
    };
    let a = local_sessions::write(&home, source("a")).expect("a");
    let b = local_sessions::write(&home, source("b")).expect("b");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Existing history", "").expect("group");
    let mut actor = Actor::new("native-a");
    actor.runtime = ActorRuntime::Codex;
    actor.external_session_id = a.source_id.clone();
    actors::add(&mut group, actor).expect("member");
    store.save(&group).expect("save");
    let binding = cccc_contracts::local_session::LocalSessionBinding {
        group_id: group.group_id.clone(),
        actor_id: group.actors[0].id.clone(),
        actor_generation: actors::generation_identity(&group.actors[0]),
        source_id: a.source_id.clone(),
        session_revision: a.revision.clone(),
    };
    local_sessions::validate_binding(&home, &binding).expect("initial access");
    let removed = call(
        &home,
        "local_session_control",
        json!({"source_id":a.source_id,"action":"remove","expected_revision":"0","by":"user"}),
    );
    assert!(removed.ok, "{:?}", removed.error);
    assert_eq!(removed.result["access_blocked"], true);
    assert_eq!(removed.result["pause_complete"], true);
    assert!(local_sessions::read(&home, &a.source_id).is_err());
    assert!(
        local_sessions::write(&home, a.clone()).is_err(),
        "a real ordinary registration must not resurrect removal"
    );
    assert!(local_sessions::read(&home, &b.source_id).is_ok());
    assert!(local_sessions::validate_binding(&home, &binding).is_err());
    assert!(!store.load(&group.group_id).expect("retained group").actors[0].enabled);
    let denied = call(
        &home,
        "message_send",
        json!({"group_id":group.group_id,"by":binding.actor_id,"_cccc_native_binding":binding,"to":["user"],"text":"must not be written","message_mode":"send"}),
    );
    assert!(!denied.ok);
    assert!(
        !cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
            .expect("ledger history")
            .iter()
            .any(|event| event.data["text"] == "must not be written")
    );
    assert!(
        local_sessions::read_record(&home, &a.source_id)
            .expect("retained lifecycle")
            .retired
            .is_none(),
        "remove is not SessionEnd"
    );
    let revision = removed.result["management_revision"].clone();
    let restored = call(
        &home,
        "local_session_control",
        json!({"source_id":a.source_id,"action":"restore","expected_revision":revision,"by":"user"}),
    );
    assert!(restored.ok, "{:?}", restored.error);
    assert_eq!(restored.result["access_blocked"], false);
    assert!(local_sessions::read(&home, &a.source_id).is_ok());
    assert!(
        local_sessions::validate_binding(&home, &binding).is_err(),
        "restore must not enable the old member"
    );
    assert!(!store.load(&group.group_id).expect("retained actor").actors[0].enabled);
    let enabled = call(
        &home,
        "actor_start",
        json!({"group_id":group.group_id,"actor_id":binding.actor_id,"by":"user"}),
    );
    assert!(enabled.ok, "{:?}", enabled.error);
    local_sessions::validate_binding(&home, &binding)
        .expect("explicit user permission can work again");
    assert!(a.host.alive());
}
