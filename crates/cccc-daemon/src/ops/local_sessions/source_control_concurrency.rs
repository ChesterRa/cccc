//! The real server response function owns the actual dispatch permit.
use super::*;
use crate::dispatch_concurrency::DispatchLocks;
use cccc_contracts::{Actor, ActorRuntime};
use std::time::Duration;
#[path = "source_control_test_gate.rs"]
mod gate;
pub(super) fn after_pause(home: &HomeLayout) {
    gate::after_pause(home);
}
fn request(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("args").clone(),
    }
}

fn fixture() -> (tempfile::TempDir, HomeLayout, cccc_core::GroupDoc, String) {
    let temp = tempfile::tempdir().expect("Home");
    let home = HomeLayout::from_path(temp.path()).expect("Home");
    home.initialize().expect("init");
    let source:local_sessions::NativeSession=serde_json::from_value(json!({"source_id":"codex:controlled","runtime":"codex","native_session_id":"controlled","host":local_sessions::NativeHost::capture(std::process::id()).expect("host"),"cwd":temp.path(),"title":"controlled","revision":"original","codex_can_receive":true})).expect("source");
    let id = source.source_id.clone();
    local_sessions::write(&home, source).expect("register");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("existing", "").expect("group");
    let mut actor = Actor::new("member");
    actor.runtime = ActorRuntime::Codex;
    actor.external_session_id = id.clone();
    cccc_core::actors::add(&mut group, actor).expect("actor");
    store.save(&group).expect("save");
    (temp, home, group, id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restore_fences_actual_actor_update_and_join_after_scan_until_unblock_commits() {
    let (_temp, home, group, id) = fixture();
    let locks = DispatchLocks::default();
    let (shutdown, _) = tokio::sync::watch::channel(false);
    let removed = crate::server_connection::response(
        &home,
        request(
            "local_session_control",
            json!({"source_id":id,"action":"remove","expected_revision":"0","by":"user"}),
        ),
        &shutdown,
        &locks,
    )
    .await;
    assert!(removed.ok);
    let store = GroupStore::new(home.clone()).expect("store");
    let joining = store.create("explicit join", "").expect("joining Group");
    let (mut paused, reached) = gate::install(&home);
    let task_home = home.clone();
    let task_locks = locks.clone();
    let task_shutdown = shutdown.clone();
    let restore = request(
        "local_session_control",
        json!({"source_id":id,"action":"restore","expected_revision":removed.result["management_revision"],"by":"user"}),
    );
    let mut restoring = gate::Task::new(tokio::spawn(async move {
        crate::server_connection::response(&task_home, restore, &task_shutdown, &task_locks).await
    }));
    tokio::task::spawn_blocking(move || reached.recv_timeout(Duration::from_secs(5)))
        .await
        .expect("gate observer")
        .expect("scan completed gate reached");
    // This fails deterministically for a ResourceOwned mutant before any timing
    // assertion: the real server must still own its global write permit here.
    assert!(
        locks.try_global_write().is_none(),
        "control lost global ownership between membership scan and unblock"
    );
    assert!(stored::blocked(&home, &id).expect("barrier"));
    assert!(
        !store
            .load(&group.group_id)
            .expect("paused old actor")
            .actors[0]
            .enabled
    );
    let update_request = request(
        "actor_update",
        json!({"group_id":group.group_id,"actor_id":group.actors[0].id,"patch":{"enabled":true},"by":"user"}),
    );
    let updating = crate::server_connection::response(&home, update_request, &shutdown, &locks);
    tokio::pin!(updating);
    // Poll the production server response into acquire_for(). It is now waiting
    // on the held dispatch lock, rather than merely spawned and unscheduled.
    assert!(
        tokio::time::timeout(Duration::from_millis(40), &mut updating)
            .await
            .is_err()
    );
    let join_request = request(
        "local_session_join",
        json!({"group_id":joining.group_id,"source_ids":[id],"by":"user"}),
    );
    let joining_response =
        crate::server_connection::response(&home, join_request, &shutdown, &locks);
    tokio::pin!(joining_response);
    assert!(
        tokio::time::timeout(Duration::from_millis(40), &mut joining_response)
            .await
            .is_err()
    );
    assert!(!store.load(&group.group_id).expect("still paused").actors[0].enabled);
    assert!(
        store
            .load(&joining.group_id)
            .expect("not admitted")
            .actors
            .is_empty()
    );
    paused.release();
    let restored = restoring.finish().await;
    assert!(restored.ok, "{:?}", restored.error);
    assert_eq!(restored.result["access_blocked"], false);
    assert!(
        !store
            .load(&group.group_id)
            .expect("restore commits paused")
            .actors[0]
            .enabled
    );
    let updated = tokio::time::timeout(Duration::from_secs(5), updating)
        .await
        .expect("explicit update completes");
    assert!(updated.ok, "{:?}", updated.error);
    assert!(
        store
            .load(&group.group_id)
            .expect("explicitly enabled")
            .actors[0]
            .enabled
    );
    let joined = tokio::time::timeout(Duration::from_secs(5), joining_response)
        .await
        .expect("explicit join completes");
    assert!(joined.ok, "{:?}", joined.error);
    assert!(
        store
            .load(&joining.group_id)
            .expect("joined")
            .actors
            .iter()
            .any(|actor| actor.external_session_id == id && actor.enabled)
    );
}

#[tokio::test]
async fn control_waits_for_an_already_admitted_group_writer() {
    let (_temp, home, group, id) = fixture();
    let locks = DispatchLocks::default();
    let (shutdown, _) = tokio::sync::watch::channel(false);
    let membership = request(
        "actor_update",
        json!({"group_id":group.group_id,"actor_id":group.actors[0].id,"patch":{"enabled":true},"by":"user"}),
    );
    let writer = locks.acquire_for(&home, &membership).await;
    let removing = crate::server_connection::response(
        &home,
        request(
            "local_session_control",
            json!({"source_id":id,"action":"remove","expected_revision":"0","by":"user"}),
        ),
        &shutdown,
        &locks,
    );
    tokio::pin!(removing);
    assert!(
        tokio::time::timeout(Duration::from_millis(40), &mut removing)
            .await
            .is_err()
    );
    assert!(!stored::blocked(&home, &id).expect("not applied before admission"));
    drop(writer);
    let removed = tokio::time::timeout(Duration::from_secs(5), removing)
        .await
        .expect("control completes");
    assert!(removed.ok, "{:?}", removed.error);
    assert!(stored::blocked(&home, &id).expect("blocked"));
    assert!(
        !GroupStore::new(home.clone())
            .expect("store")
            .load(&group.group_id)
            .expect("paused")
            .actors[0]
            .enabled
    );
}
