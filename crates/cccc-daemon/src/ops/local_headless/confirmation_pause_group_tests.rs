use super::{ensure_allowed, pause, retry};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::json;

#[test]
fn deleting_or_resetting_a_group_retires_only_its_confirmation_pauses() {
    for operation in ["group_delete", "group_reset"] {
        let temp = tempfile::tempdir().expect("isolated home");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let old = store.create("Retired", "").expect("old group");
        let other = store.create("Retained", "").expect("other group");
        let old_key = (old.group_id.clone(), "claude".into());
        let other_key = (other.group_id.clone(), "claude".into());
        pause(old_key.clone()).expect("old refusal");
        pause(other_key.clone()).expect("other refusal");
        let invoke = |by| {
            crate::dispatch::dispatch(
                &home,
                &DaemonRequest {
                    v: 1,
                    op: operation.into(),
                    args: json!({"group_id":old.group_id,"confirm":old.group_id,"by":by})
                        .as_object()
                        .expect("arguments")
                        .clone(),
                },
            )
        };
        assert!(
            !invoke("missing-actor").ok,
            "unauthorized deletion is rejected"
        );
        assert!(
            ensure_allowed(&old_key).is_err(),
            "failed deletion preserves refusal"
        );
        let deleted = invoke("user");
        assert!(deleted.ok, "{operation}: {:?}", deleted.error);
        ensure_allowed(&old_key).expect("deleted group must not retain refusal state");
        assert_eq!(
            ensure_allowed(&other_key)
                .expect_err("other group remains paused")
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        retry(&other_key).expect("fixture cleanup");
    }
}
