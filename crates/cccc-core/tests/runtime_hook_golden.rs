use cccc_core::codex_hook_state::CodexHookState;
use cccc_core::runtime_activity::RuntimeActivityEvent;
use serde_json::Value;

#[test]
fn python_and_rust_share_exact_runtime_hook_wire_schema() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/runtime_hooks/v3_state_and_activity.json"
    ))
    .expect("shared runtime hook golden must be valid JSON");

    let state: CodexHookState =
        serde_json::from_value(fixture["state"].clone()).expect("decode v3 state");
    let activity: RuntimeActivityEvent =
        serde_json::from_value(fixture["activity"].clone()).expect("decode v1 activity");

    assert_eq!(
        serde_json::to_value(state).expect("encode v3 state"),
        fixture["state"]
    );
    assert_eq!(
        serde_json::to_value(activity).expect("encode v1 activity"),
        fixture["activity"]
    );
}
