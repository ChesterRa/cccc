use cccc_core::{GroupStore, HomeLayout, inbox, integration_state};
use serde_json::{Value, json};

use super::runtime_completion::{call, completion_args, next_turn, setup};

const RUNTIME_STATES: &str = "runtime_states";

#[test]
fn replay_repairs_projection_only_while_the_same_turn_is_active() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = setup(&home);
    let turn = next_turn(&home, &group_id);
    let args = completion_args(&group_id, &turn, "delivery-a");
    call(&home, "web_model_runtime_complete_turn", args.clone());
    set_runtime(
        &home,
        &group_id,
        "working",
        turn["turn_id"].as_str().expect("turn id must be a string"),
    );

    let replay = call(&home, "web_model_runtime_complete_turn", args);

    assert_eq!(replay.result["delivery_id"], "delivery-a");
    let state = runtime(&home, &group_id);
    assert_eq!(state["status"], "waiting");
    assert_eq!(state["active_turn_id"], "");
}

#[test]
fn replay_of_a_does_not_modify_newer_active_turn_b_or_its_cursor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = setup(&home);
    let turn_a = next_turn(&home, &group_id);
    let args_a = completion_args(&group_id, &turn_a, "delivery-a");
    call(&home, "web_model_runtime_complete_turn", args_a.clone());
    call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["web1"],"text":"turn b"}),
    );
    let turn_b = next_turn(&home, &group_id);
    let state_before = runtime(&home, &group_id);
    let cursor_before = cursor(&home, &group_id);

    let replay = call(&home, "web_model_runtime_complete_turn", args_a);

    assert_eq!(replay.result["delivery_id"], "delivery-a");
    assert_eq!(runtime(&home, &group_id), state_before);
    assert_eq!(
        runtime(&home, &group_id)["active_turn_id"],
        turn_b["turn_id"]
    );
    assert_eq!(cursor(&home, &group_id), cursor_before);
}

#[test]
fn replay_does_not_revive_a_stopped_or_failed_runtime() {
    for status in ["stopped", "failed", "idle"] {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group_id = setup(&home);
        let turn = next_turn(&home, &group_id);
        let args = completion_args(&group_id, &turn, "delivery-a");
        call(&home, "web_model_runtime_complete_turn", args.clone());
        set_runtime(&home, &group_id, status, "");
        let state_before = runtime(&home, &group_id);
        let cursor_before = cursor(&home, &group_id);

        call(&home, "web_model_runtime_complete_turn", args);

        assert_eq!(runtime(&home, &group_id), state_before, "{status}");
        assert_eq!(cursor(&home, &group_id), cursor_before, "{status}");
    }
}

fn set_runtime(home: &HomeLayout, group_id: &str, status: &str, active_turn_id: &str) {
    let store = GroupStore::new(home.clone()).expect("store");
    integration_state::group_update(&store, group_id, RUNTIME_STATES, |states| {
        states["web1"]["status"] = json!(status);
        states["web1"]["active_turn_id"] = json!(active_turn_id);
        Ok(())
    })
    .expect("set runtime");
}

fn runtime(home: &HomeLayout, group_id: &str) -> Value {
    call(
        home,
        "headless_status",
        json!({"group_id":group_id,"actor_id":"web1"}),
    )
    .result["state"]
        .clone()
}

fn cursor(home: &HomeLayout, group_id: &str) -> Option<String> {
    inbox::cursor(home, group_id, "web1").expect("cursor")
}
